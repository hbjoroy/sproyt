use super::*;
use crate::{
    agent::{AgentRepository, AgentService},
    db::{PostgresChatRepository, SqliteChatRepository},
    process::{ProcessRepository, ProcessService, StartedProcess},
    web::assets::{CONNECTION_SOURCE, NAVIGATION_SOURCE, SESSION_SOURCE},
};
use futures_util::{SinkExt, StreamExt};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as ClientMessage,
};

type TestSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct BrowserClient;

impl BrowserClient {
    fn contains(&self, needle: &str) -> bool {
        APP_SOURCE.contains(needle)
            || APP_BUNDLE.contains(needle)
            || NAVIGATION_SOURCE.contains(needle)
            || INDEX_HTML.contains(needle)
    }
}

const BROWSER_CLIENT: BrowserClient = BrowserClient;

#[test]
fn media_signatures_override_untrusted_declared_types() {
    assert_eq!(
        detected_media_type(b"\x89PNG\r\n\x1a\nrest", "text/html").as_deref(),
        Some("image/png")
    );
    assert_eq!(detected_media_type(b"not media", "image/png"), None);
    assert_eq!(
        detected_media_type(b"\0\0\0\x18ftypheicrest", "application/octet-stream").as_deref(),
        Some("image/heic")
    );
    assert_eq!(
        detected_media_type(b"\0\0\0\x14ftypqt  rest", "video/quicktime").as_deref(),
        Some("video/quicktime")
    );
}

#[test]
fn browser_exposes_paste_upload_and_safe_media_rendering() {
    assert!(BROWSER_CLIENT.contains("bodyInput.addEventListener(\"paste\""));
    assert!(BROWSER_CLIENT.contains("accept=\"image/*,video/*,.heic,.heif,.mov\""));
    assert!(BROWSER_CLIENT.contains("/api/v1/channels/${activeChannelId}/media"));
    assert!(
        BROWSER_CLIENT.contains("response.status === 401 && await sessionController.refresh(true)")
    );
    assert!(BROWSER_CLIENT.contains("element.loading = \"lazy\""));
    assert!(BROWSER_CLIENT.contains("element.controls = true"));
    assert!(BROWSER_CLIENT.contains("/api/v1/media/${media.id}/preview"));
    assert!(BROWSER_CLIENT.contains("function openMediaLightbox(url, name)"));
    assert!(BROWSER_CLIENT.contains("mediaLightbox.showModal()"));
    assert!(BROWSER_CLIENT.contains("max-height: min(48dvh, 420px)"));
    assert!(BROWSER_CLIENT.contains("max-width: calc(100vw - 24px)"));
    assert!(BROWSER_CLIENT.contains("id=\"upload-status\""));
    assert!(BROWSER_CLIENT.contains("request.upload.addEventListener(\"progress\""));
    assert!(BROWSER_CLIENT.contains("Behandlar fila"));
    assert!(BROWSER_CLIENT.contains("className = \"media-preview-remove\""));
    assert!(
        BROWSER_CLIENT
            .contains("remove.setAttribute(\"aria-label\", `Fjern ${media.original_filename}`)")
    );
    assert!(
        BROWSER_CLIENT.contains(
            "pendingMedia = pendingMedia.filter((candidate) => candidate.id !== media.id)"
        )
    );
    assert!(BROWSER_CLIENT.contains("if (pendingMessages.size > 0) return"));
    assert!(BROWSER_CLIENT.contains("bodyInput.focus({ preventScroll: true })"));
}

#[test]
fn browser_uses_a_compact_composer_with_safe_keyboard_semantics() {
    assert!(BROWSER_CLIENT.contains("--composer-rest-height: 44px"));
    assert!(BROWSER_CLIENT.contains("--composer-max-height: 126px"));
    assert!(BROWSER_CLIENT.contains("height: 44px; min-width: 44px; min-height: 44px"));
    assert!(BROWSER_CLIENT.contains("resize: none; overflow-y: hidden"));
    assert!(BROWSER_CLIENT.contains("function resizeComposer()"));
    assert!(BROWSER_CLIENT.contains("bodyInput.value.length === 0\n          ? minimum"));
    assert!(
        BROWSER_CLIENT.contains("bodyInput.value.length > 0 && bodyInput.scrollHeight > maximum")
    );
    assert!(BROWSER_CLIENT.contains("form.send.is-expanded #media-previews:not(:empty)"));
    assert!(BROWSER_CLIENT.contains("form.send.is-expanded #upload-status:not(:empty)"));
    assert!(BROWSER_CLIENT.contains("min-width: 44px; min-height: 44px"));
    assert!(BROWSER_CLIENT.contains("composer-icon\" id=\"attach-media\""));
    assert!(BROWSER_CLIENT.contains("id=\"composer-tools\" aria-label=\"Meldingsverktøy\" hidden"));
    assert!(BROWSER_CLIENT.contains("composerTools.hidden = !composerHasFocus"));
    assert!(BROWSER_CLIENT.contains("sendForm.addEventListener(\"focusin\""));
    assert!(
        BROWSER_CLIENT.contains(
            "sendForm.addEventListener(\"focusout\", closeComposerToolsAfterFocusLeaves)"
        )
    );
    assert!(BROWSER_CLIENT.contains("document.addEventListener(\"pointerdown\""));
    assert!(
        BROWSER_CLIENT.contains(
            "if (event.target instanceof Node && sendForm.contains(event.target)) return"
        )
    );
    assert!(BROWSER_CLIENT.contains("if (sendForm.contains(document.activeElement)) return"));
    assert!(BROWSER_CLIENT.contains("messageEmojiPicker.open = false"));
    assert!(BROWSER_CLIENT.contains("aria-label=\"Send melding\" title=\"Send melding\""));
    assert!(!BROWSER_CLIENT.contains(">Send</button>"));
    assert!(BROWSER_CLIENT.contains("compositionstart"));
    assert!(BROWSER_CLIENT.contains("compositionend"));
    assert!(BROWSER_CLIENT.contains("event.keyCode !== 229"));
    assert!(BROWSER_CLIENT.contains("!event.isComposing"));
    assert!(BROWSER_CLIENT.contains("usesDesktopComposerKeys.matches"));
    assert!(BROWSER_CLIENT.contains("sendForm.requestSubmit()"));
    assert!(BROWSER_CLIENT.contains("@media (prefers-reduced-motion: no-preference)"));
    assert!(BROWSER_CLIENT.contains("attachMediaButton.disabled = !writableChannel"));
    assert!(BROWSER_CLIENT.contains("syncComposerState();"));
}

#[test]
fn browser_keeps_compact_status_controls_and_saves_the_complete_draft() {
    assert!(BROWSER_CLIENT.contains("class=\"status-fields\""));
    assert!(BROWSER_CLIENT.contains("class=\"status-emoji-options\""));
    assert!(BROWSER_CLIENT.contains("class=\"secondary-button\" id=\"clear-status\""));
    assert!(BROWSER_CLIENT.contains(">Nullstill</button>"));
    assert!(BROWSER_CLIENT.contains(">Slå på varsling</button>"));
    assert!(BROWSER_CLIENT.contains("class=\"logout-link\""));
    assert!(BROWSER_CLIENT.contains("class=\"logout-icon\" aria-hidden=\"true\""));
    assert!(BROWSER_CLIENT.contains(".inbox-icon { display: grid; width: 26px; height: 26px"));
    assert!(BROWSER_CLIENT.contains("letter-spacing: .01em"));
    assert!(
        BROWSER_CLIENT.contains("const statusDraft = { emoji: \"\", text: \"\", dirty: false }")
    );
    assert!(BROWSER_CLIENT.contains("statusDraft.emoji = statusEmoji.value"));
    assert!(BROWSER_CLIENT.contains("statusDraft.text = statusText.value"));
    assert!(BROWSER_CLIENT.contains(
            "sendCommand(\"set_status\", { text: statusDraft.text, emoji: statusDraft.emoji, expires_at: null })"
        ));
    assert!(BROWSER_CLIENT.contains("if (!statusDraft.dirty)"));
    assert!(BROWSER_CLIENT.contains(
        "if (event.payload.profile.id === currentParticipantId) statusDraft.dirty = false"
    ));
}

#[test]
fn browser_keeps_desktop_sidebar_controls_compact_and_reachable() {
    assert!(BROWSER_CLIENT.contains("id=\"desktop-sidebar-toggle\""));
    assert!(BROWSER_CLIENT.contains("sproyt.desktop-sidebar-collapsed.v1"));
    assert!(
        BROWSER_CLIENT.contains("main.desktop-sidebar-collapsed { grid-template-columns: 56px")
    );
    assert!(
        BROWSER_CLIENT.contains("main.desktop-sidebar-expanded { grid-template-columns: 280px")
    );
    assert!(BROWSER_CLIENT.contains("id=\"desktop-advanced-entry\""));
    assert!(
        BROWSER_CLIENT.contains(
            ".advanced-tools button:not([disabled]), .advanced-tools input:not([disabled])"
        )
    );
    assert!(BROWSER_CLIENT.contains("processTitle.tabIndex = -1"));
    assert!(BROWSER_CLIENT.contains("[data-tooltip]:hover::after, .sidebar.desktop-collapsed [data-tooltip]:focus-visible::after"));
    assert!(BROWSER_CLIENT.contains("data-tooltip=\"Kollaps menyen\""));
    assert!(BROWSER_CLIENT.contains("data-tooltip=\"Set status\""));
    assert!(BROWSER_CLIENT.contains("data-tooltip=\"Varsel\""));
    assert!(BROWSER_CLIENT.contains("data-tooltip=\"Ulest\""));
    assert!(BROWSER_CLIENT.contains("data-tooltip=\"Omtalar\""));
    assert!(BROWSER_CLIENT.contains("data-tooltip=\"Oppgåver\""));
    assert!(BROWSER_CLIENT.contains("button.dataset.tooltip = buttonLabel"));
    assert!(BROWSER_CLIENT.contains("currentStatus.dataset.tooltip = statusLabel"));
    assert!(BROWSER_CLIENT.contains("notificationSummary.dataset.tooltip = notificationLabel"));
}

#[test]
fn browser_is_an_installable_pwa_with_bounded_offline_caching() {
    let manifest: serde_json::Value = serde_json::from_str(PWA_MANIFEST).unwrap();
    assert_eq!(manifest["name"], "Sprøyt");
    assert_eq!(manifest["display"], "standalone");
    assert_eq!(manifest["start_url"], "/");
    assert!(
        manifest["icons"]
            .as_array()
            .is_some_and(|icons| icons.len() >= 3)
    );
    assert!(BROWSER_CLIENT.contains("rel=\"manifest\" href=\"/manifest.webmanifest\""));
    assert!(BROWSER_CLIENT.contains("navigator.serviceWorker.register"));
    assert!(BROWSER_CLIENT.contains("/assets/sproyt-wave.svg"));
    assert!(BROWSER_CLIENT.contains("viewport-fit=cover"));
    assert!(BROWSER_CLIENT.contains("--app-height: 100dvh"));
    assert!(BROWSER_CLIENT.contains("env(safe-area-inset-bottom)"));
    assert!(BROWSER_CLIENT.contains("const height = viewport?.height || window.innerHeight"));
    assert!(BROWSER_CLIENT.contains("--app-offset-top: 0px"));
    assert!(BROWSER_CLIENT.contains("height: var(--app-height)"));
    assert!(!BROWSER_CLIENT.contains("width: min(1120px, 100%)"));
    assert!(!BROWSER_CLIENT.contains("height: min(760px, calc(100dvh - 48px));"));
    assert!(BROWSER_CLIENT.contains("overflow-y: auto;\n        overscroll-behavior: contain;\n        scrollbar-gutter: stable;"));
    assert!(SERVICE_WORKER.contains("request.mode === \"navigate\""));
    assert!(SERVICE_WORKER.contains("url.pathname.startsWith(\"/api/\")"));
    assert!(SERVICE_WORKER.contains("url.pathname.startsWith(\"/auth/\")"));
    assert_eq!(&WAVE_LOGO_192[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&WAVE_LOGO_512[..8], b"\x89PNG\r\n\x1a\n");
}

#[tokio::test]
async fn image_upload_creates_a_bounded_preview_and_rejects_truncation() {
    use image::GenericImageView;

    let source = image::DynamicImage::new_rgb8(1_440, 900);
    let mut encoded = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 90)
        .encode_image(&source)
        .unwrap();
    let (_, dimensions, preview) = prepare_uploaded_media(encoded, "image/jpeg").await.unwrap();
    assert_eq!(dimensions, Some((1_440, 900)));
    let preview = preview.unwrap();
    assert_eq!(preview.content_type, "image/jpeg");
    let decoded = image::load_from_memory(&preview.content).unwrap();
    assert_eq!(decoded.dimensions(), (720, 450));

    let portrait_pixels = image::DynamicImage::new_rgb8(1_440, 900);
    let mut iphone_jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut iphone_jpeg, 90)
        .encode_image(&portrait_pixels)
        .unwrap();
    let exif_orientation_6 = [
        0xff, 0xe1, 0x00, 0x22, b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 0x2a, 0, 8, 0, 0, 0, 1,
        0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
    ];
    iphone_jpeg.splice(2..2, exif_orientation_6);
    let (normalized, dimensions, preview) = prepare_uploaded_media(iphone_jpeg, "image/jpeg")
        .await
        .unwrap();
    assert_eq!(dimensions, Some((900, 1_440)));
    let decoded = image::load_from_memory(&preview.unwrap().content).unwrap();
    assert_eq!(decoded.dimensions(), (450, 720));
    let normalized = image::load_from_memory(&normalized).unwrap();
    assert_eq!(normalized.dimensions(), (900, 1_440));

    let small_portrait_pixels = image::DynamicImage::new_rgb8(640, 480);
    let mut small_samsung_jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut small_samsung_jpeg, 90)
        .encode_image(&small_portrait_pixels)
        .unwrap();
    small_samsung_jpeg.splice(2..2, exif_orientation_6);
    let (normalized, dimensions, preview) =
        prepare_uploaded_media(small_samsung_jpeg, "image/jpeg")
            .await
            .unwrap();
    assert_eq!(dimensions, Some((480, 640)));
    assert!(preview.is_none());
    let normalized = image::load_from_memory(&normalized).unwrap();
    assert_eq!(normalized.dimensions(), (480, 640));

    let source = image::DynamicImage::new_rgb8(32, 24);
    let mut motion_photo = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut motion_photo, 90)
        .encode_image(&source)
        .unwrap();
    motion_photo.extend_from_slice(b"appended Android motion-photo payload");
    let (_, dimensions, preview) = prepare_uploaded_media(motion_photo, "image/jpeg")
        .await
        .unwrap();
    assert_eq!(dimensions, Some((32, 24)));
    assert!(preview.is_none());

    let truncated = prepare_uploaded_media(vec![0xff, 0xd8, 0xff], "image/jpeg").await;
    assert!(matches!(
        truncated,
        Err(MediaPreparationError::InvalidImage)
    ));
}

#[test]
fn browser_exposes_circle_scoped_mention_autocomplete() {
    assert!(BROWSER_CLIENT.contains("id=\"mention-suggestions\""));
    assert!(BROWSER_CLIENT.contains("aria-autocomplete=\"list\""));
    assert!(BROWSER_CLIENT.contains("sendCommand(\"list_circle_users\""));
    assert!(BROWSER_CLIENT.contains("knownCircleUsers.get(channel.circle_id)"));
    assert!(BROWSER_CLIENT.contains("mentionHandle(user).startsWith(query)"));
    assert!(BROWSER_CLIENT.contains("event.key === \"ArrowDown\""));
    assert!(BROWSER_CLIENT.contains("event.key === \"Enter\""));
    assert!(BROWSER_CLIENT.contains("selectMention(selectedMentionIndex)"));
}

#[test]
fn browser_exposes_durable_reaction_badges() {
    assert!(BROWSER_CLIENT.contains("className = \"reaction-badge\""));
    assert!(BROWSER_CLIENT.contains("`${emoji} ${reaction.count}`"));
    assert!(BROWSER_CLIENT.contains("aria-pressed"));
    assert!(BROWSER_CLIENT.contains("sendCommand(\"toggle_message_reaction\""));
    assert!(BROWSER_CLIENT.contains("sendCommand(\"list_channel_reactions\""));
    assert!(BROWSER_CLIENT.contains("event.type === \"message_reaction_changed\""));
    assert!(BROWSER_CLIENT.contains("chatEvent.type === \"message_reaction_changed\""));
    assert!(BROWSER_CLIENT.contains("className = \"message-reaction-details\""));
    assert!(BROWSER_CLIENT.contains("reactionHeading.textContent = \"Reaksjonar\""));
    assert!(BROWSER_CLIENT.contains("reaction.user_ids || []"));
    assert!(BROWSER_CLIENT.contains("activeProfile(userId)?.display_name"));
    assert!(BROWSER_CLIENT.contains("id=\"reaction-emoji-catalog\""));
    assert!(BROWSER_CLIENT.contains("Søk eller lim inn Unicode-emoji"));
    assert!(BROWSER_CLIENT.contains("submitCustomReaction"));
}

#[test]
fn browser_patches_only_affected_reaction_card_with_timeline_fallback() {
    assert!(BROWSER_CLIENT.contains("function patchMessageReactions(messageId)"));
    assert!(BROWSER_CLIENT.contains("|| [...threadReplies.values()].flat().find"));
    assert!(BROWSER_CLIENT.contains("for (const container of [messagesEl, threadMessages])"));
    assert!(
        BROWSER_CLIENT
            .contains("const nextReactions = renderMessageReactions(message, (open) => {")
    );
    assert!(BROWSER_CLIENT.contains("card.classList.toggle(\"reaction-picker-requested\", open)"));
    assert!(BROWSER_CLIENT.contains("const thread = reactions.querySelector(\".thread-link\");"));
    assert!(BROWSER_CLIENT.contains("const menu = card.querySelector(\".message-menu\");"));
    assert!(
        BROWSER_CLIENT.contains("if (thread instanceof HTMLElement) nextReactions.append(thread);")
    );
    assert!(
        BROWSER_CLIENT
            .contains("if (menu instanceof HTMLElement) placeMessageMenu(card, nextReactions, menu, thread instanceof HTMLElement, messageId);")
    );
    assert!(BROWSER_CLIENT.contains("reactions.replaceWith(nextReactions);"));
    assert!(!BROWSER_CLIENT.contains("reactions.replaceWith(renderMessageReactions(message));"));
    assert!(BROWSER_CLIENT.contains("if (!card || !(reactions instanceof HTMLElement)) continue;"));

    let patch = APP_BUNDLE
        .split("function patchMessageReactions(messageId) {")
        .nth(1)
        .and_then(|value| value.split("\n      function appendMessage").next())
        .expect("keyed reaction patch helper");
    let capture = patch
        .find("const interaction = captureMessageInteraction(container);")
        .expect("capture interaction before patch");
    let replace = patch
        .find("reactions.replaceWith(nextReactions);")
        .expect("replace reaction footer");
    let restore = patch
        .find("restoreMessageInteraction(container, interaction);")
        .expect("restore interaction after patch");
    assert!(capture < replace && replace < restore);

    for reaction_event in [
        "if (event.type === \"message_reaction_changed\") {\n          if (event.payload.change.channel_id === activeChannelId) {\n            applyReactionChange(event.payload.change);\n            if (!patchMessageReactions(event.payload.change.message_id)) {\n              renderTimeline({ preserveScroll: true });\n            }",
        "} else if (chatEvent.type === \"message_reaction_changed\") {\n            if (chatEvent.change.channel_id === activeChannelId) {\n              applyReactionChange(chatEvent.change);\n              if (!patchMessageReactions(chatEvent.change.message_id)) {\n                renderTimeline({ preserveScroll: true });\n              }",
    ] {
        assert!(BROWSER_CLIENT.contains(reaction_event));
    }
}

#[test]
fn browser_keepalive_does_not_rebuild_interactive_message_views() {
    let heartbeat = CONNECTION_SOURCE
        .split("state.heartbeatTimer = dependencies.setInterval(() => {")
        .nth(1)
        .and_then(|value| value.split("}, 20_000);").next())
        .expect("heartbeat block");
    assert!(heartbeat.contains("send(\"ping\")"));
    assert!(!heartbeat.contains("list_users"));
    assert!(!heartbeat.contains("list_my_channels"));
    assert!(!heartbeat.contains("list_mentions"));
    assert!(!heartbeat.contains("list_tasks"));
    assert!(BROWSER_CLIENT.contains("function refreshVisibleProfileStatuses(userId = null)"));
    assert!(BROWSER_CLIENT.contains("senderLabel.dataset.profileUserId = message.sender_id"));
    assert!(BROWSER_CLIENT.contains("refreshVisibleProfileStatuses(event.payload.profile.id)"));
    assert!(BROWSER_CLIENT.contains("const interaction = captureMessageInteraction(messagesEl)"));
    assert!(BROWSER_CLIENT.contains("restoreMessageInteraction(messagesEl, interaction)"));
    assert!(BROWSER_CLIENT.contains(".reaction-picker[open]"));
    assert!(BROWSER_CLIENT.contains("focus({ preventScroll: true })"));
}

#[test]
fn browser_rotates_sockets_only_for_real_session_changes() {
    assert!(SESSION_SOURCE.contains("value.type !== \"session_rotated\""));
    assert!(SESSION_SOURCE.contains("type: \"session_rotated\""));
    assert!(CONNECTION_SOURCE.contains("if (state.socketHandoff !== null) return"));
    assert!(CONNECTION_SOURCE.contains("expectedChannelId: state.desiredChannelId"));
    assert!(CONNECTION_SOURCE.contains("expectedGeneration: state.subscriptionGeneration"));
    assert!(CONNECTION_SOURCE.contains("expectedSubscriptionRequestId"));
    assert!(CONNECTION_SOURCE.contains("}, 10_000);"));
    assert!(CONNECTION_SOURCE.contains("session handoff timed out"));
    assert!(
        CONNECTION_SOURCE.contains(
            "if (handoff.timeoutId !== null) dependencies.clearTimeout(handoff.timeoutId)"
        )
    );
    assert!(
        !CONNECTION_SOURCE
            .contains("state.subscribedChannelId = null;\n          if (previousSocket.readyState")
    );
    assert!(APP_SOURCE.contains("setConnectionStatus(\"Gjenopprettar samtalen …\")"));
    // A candidate only becomes active after the typed readiness event for the
    // current desired channel and generation, plus the previous socket's
    // correlated requests. The supervisor owns that transition.
    assert!(CONNECTION_SOURCE.contains("tryCommitHandoff()"));
    assert!(CONNECTION_SOURCE.contains("matchesExpectedSubscription"));
    assert!(
        CONNECTION_SOURCE
            .contains("serverEvent.request_id === handoff.expectedSubscriptionRequestId")
    );
    assert!(CONNECTION_SOURCE.contains("state.socketHandoff?.nextSocket === nextSocket"));
    assert!(CONNECTION_SOURCE.contains("state.socket = nextSocket"));
    assert!(!BROWSER_CLIENT.contains("}, 500);"));
    assert!(APP_SOURCE.contains("recoverConnection(false)"));
    assert!(BROWSER_CLIENT.contains(
            ".catch(() => connectionSupervisor.scheduleReconnect(1006, \"kunne ikkje gjenopprette sambandet\"))"
        ));
    assert!(!BROWSER_CLIENT.contains(
            "recoverConnection(true).catch(() => scheduleReconnect(1006, \"kunne ikkje gjenopprette sambandet\"))"
        ));
}

#[test]
fn browser_routes_session_connection_and_events_through_supervisors() {
    assert!(BROWSER_CLIENT.contains("sessionController = createSessionController({"));
    assert!(BROWSER_CLIENT.contains("const connectionSupervisor = createConnectionController({"));
    assert!(INDEX_HTML.contains("{{APP_URL}}"));
    assert!(!INDEX_HTML.contains("{{CLIENT_STORE_URL}}"));
    assert!(BROWSER_CLIENT.contains("const applicationStore = createApplicationStore();"));
    assert!(CLIENT_STORE.contains("function createApplicationStore()"));
    assert!(CLIENT_STORE.contains("updateSession(patch)"));
    assert!(CLIENT_STORE.contains("updateConnection(patch)"));
    assert!(CLIENT_STORE.contains("reduceServerEvent(event)"));
    assert!(CLIENT_STORE.contains("function createServerEventMailbox({"));
    assert!(
        CLIENT_STORE
            .contains("export {\n  createApplicationStore,\n  createServerEventMailbox\n};")
    );
    assert!(
        CLIENT_STORE
            .contains("const nextEvent = queue.shift();\n          if (nextEvent === void 0) break;\n          deliver(reduce(nextEvent));")
    );
    let mailbox = CLIENT_STORE
        .split("function createServerEventMailbox({")
        .nth(1)
        .expect("serialized mailbox factory");
    let queued = mailbox.find("queue.push(event);").expect("enqueue event");
    let reduce_then_deliver = mailbox
        .find("if (nextEvent === void 0) break;\n          deliver(reduce(nextEvent));")
        .expect("reduce before delivery");
    assert!(queued < reduce_then_deliver);
    assert!(BROWSER_CLIENT.contains("const serverEventMailbox = createServerEventMailbox({"));
    assert!(BROWSER_CLIENT.contains("reduce: applicationStore.reduceServerEvent,"));
    assert!(BROWSER_CLIENT.contains("deliver: renderServerEvent"));
    assert!(!BROWSER_CLIENT.contains("const applicationStore = (() => {"));
    assert!(!BROWSER_CLIENT.contains("const serverEventMailbox = (() => {"));
    assert!(!BROWSER_CLIENT.contains("let sessionRefreshTimer"));
    assert!(!BROWSER_CLIENT.contains("let sessionRefreshPromise"));
    assert!(!BROWSER_CLIENT.contains("let authenticationRecoveryPromise"));
    assert!(!BROWSER_CLIENT.contains("let connectionRecoveryPromise"));
    assert!(!BROWSER_CLIENT.contains("let reconnectTimer"));
    assert!(!BROWSER_CLIENT.contains("let reconnectAttempt"));
    assert!(!BROWSER_CLIENT.contains("let heartbeatTimer"));
    assert!(!BROWSER_CLIENT.contains("let stableConnectionTimer"));
    assert!(!BROWSER_CLIENT.contains("let socket = null"));
    assert!(!BROWSER_CLIENT.contains("let socketHandoff = null"));
    assert!(CONNECTION_SOURCE.contains("recoveryPromise: null"));
    assert!(CONNECTION_SOURCE.contains("reconnectTimer: null"));
    assert!(CONNECTION_SOURCE.contains("reconnectAttempt: 0"));
    assert!(CONNECTION_SOURCE.contains("heartbeatTimer: null"));
    assert!(CONNECTION_SOURCE.contains("stableConnectionTimer: null"));
    assert!(CONNECTION_SOURCE.contains("socket: null"));
    assert!(CONNECTION_SOURCE.contains("socketHandoff: null"));
    assert!(
        BROWSER_CLIENT
            .contains("sessionController.start().catch(() => sessionController.schedule(30));")
    );
    assert!(BROWSER_CLIENT.contains("connectionSupervisor.start();"));
    assert!(SESSION_SOURCE.contains("schedule(message.refreshAfterSeconds)"));
    assert!(BROWSER_CLIENT.contains("connectionSupervisor.replaceAfterSessionRefresh()"));
    assert!(CONNECTION_SOURCE.contains("const serverEvent = parseSocketEvent(event.data)"));
    assert!(CONNECTION_SOURCE.contains("dependencies.onEvent(serverEvent)"));
    assert!(CONNECTION_SOURCE.contains("export function parseSocketEvent(data: unknown)"));
    assert!(CONNECTION_SOURCE.contains("return asWireEvent(JSON.parse(data));"));
    assert!(!APP_SOURCE.contains("asWireEvent(event.data)"));
    assert!(!BROWSER_CLIENT.contains("serverEventMailbox.enqueue(JSON.parse(event.data))"));
    assert!(!BROWSER_CLIENT.contains("renderServerEvent(JSON.parse(event.data))"));
}

#[test]
fn client_store_fingerprint_uses_safe_revisions_or_asset_hashes() {
    assert_eq!(
        client_store_fingerprint("a1b2c3d", b"first asset"),
        "a1b2c3d"
    );
    assert_eq!(
        client_store_fingerprint(&"a".repeat(64), b"first asset"),
        "a".repeat(64)
    );

    let unknown = client_store_fingerprint("unknown", b"first asset");
    assert_eq!(unknown.len(), 64);
    assert_ne!(unknown, "unknown");
    assert_ne!(
        unknown,
        client_store_fingerprint("unknown", b"second asset")
    );
    assert_ne!(
        client_store_fingerprint("ABCDEF0", b"first asset"),
        "ABCDEF0"
    );
}

#[test]
fn browser_exposes_author_owned_message_editing() {
    assert!(BROWSER_CLIENT.contains("sendCommand(\"edit_message\""));
    assert!(BROWSER_CLIENT.contains("message.sender_id === currentParticipantId"));
    assert!(BROWSER_CLIENT.contains("chatEvent.type === \"message_edited\""));
    assert!(BROWSER_CLIENT.contains("event.type === \"message_edited\""));
    assert!(BROWSER_CLIENT.contains("· redigert"));
    assert!(BROWSER_CLIENT.contains("className = \"message-editor\""));
    assert!(
        BROWSER_CLIENT.contains("const mediaTokens = message.body.match(mediaTokenPattern) || []")
    );
}

#[test]
fn browser_exposes_author_owned_soft_deletion() {
    assert!(BROWSER_CLIENT.contains("sendCommand(\"delete_message\""));
    assert!(BROWSER_CLIENT.contains("chatEvent.type === \"message_deleted\""));
    assert!(BROWSER_CLIENT.contains("event.type === \"message_deleted\""));
    assert!(BROWSER_CLIENT.contains("Meldinga er sletta."));
    assert!(BROWSER_CLIENT.contains("window.confirm(\"Vil du slette meldinga?"));
    assert!(BROWSER_CLIENT.contains(
        "if (!message.deleted_at) {\n          footer = renderMessageReactions(message, (open) => {"
    ));
    assert!(BROWSER_CLIENT.contains(
        "if (!message.deleted_at) {\n          const menu = document.createElement(\"details\");"
    ));
}

#[test]
fn browser_exposes_compact_durable_message_threads() {
    assert!(BROWSER_CLIENT.contains("id=\"thread-panel\""));
    assert!(BROWSER_CLIENT.contains("grid-template-rows: auto minmax(0, 1fr) auto"));
    assert!(BROWSER_CLIENT.contains("height: min(760px, calc(var(--app-height) - 24px))"));
    assert!(BROWSER_CLIENT.contains(".thread-messages { display: grid; min-height: 0;"));
    assert!(BROWSER_CLIENT.contains(
        ".thread-form { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-self: end;"
    ));
    assert!(BROWSER_CLIENT.contains("parent_message_id: activeThreadRootId"));
    assert!(BROWSER_CLIENT.contains("function openThread(messageId)"));
    assert!(BROWSER_CLIENT.contains("id=\"thread-emoji-picker\""));
    assert!(BROWSER_CLIENT.contains("#thread-emoji-options [data-emoji]"));
    assert!(BROWSER_CLIENT.contains("insertEmoji(threadBody, emoji)"));
    assert!(BROWSER_CLIENT.contains("if (event.key === \"Escape\" && threadEmojiPicker.open)"));
    assert!(
        BROWSER_CLIENT.contains(
            "threadEmojiPicker.querySelector(\"summary\")?.focus({ preventScroll: true })"
        )
    );
    assert!(
        BROWSER_CLIENT.contains("if (threadPanel.open && !threadEmojiPicker.contains(target))")
    );
    assert!(BROWSER_CLIENT.contains("function settleThreadAtBottom()"));
    assert!(BROWSER_CLIENT.contains("threadMessages.scrollTop = threadMessages.scrollHeight"));
    assert!(!BROWSER_CLIENT.contains(
        "sendCommand(\"load_thread\", { root_message_id: messageId });\n        threadBody.focus();"
    ));
    assert!(BROWSER_CLIENT.contains("const threadReplies = new Map<string, ChatMessage[]>()"));
    assert!(NAVIGATION_SOURCE.contains("const threadDraftPrefix = \"sproyt.thread-draft.v1.\""));
    assert!(BROWSER_CLIENT.contains(
        "function persistThreadDraft(rootId = activeThreadRootId, channelId = activeChannelId)"
    ));
    assert!(BROWSER_CLIENT.contains("function restoreThreadDraft(rootId, channelId)"));
    assert!(BROWSER_CLIENT.contains("clearThreadDraft(pending.rootId, pending.channelId)"));
    assert!(!BROWSER_CLIENT.contains(
        "localStorage.setItem(threadDraftKey(channelId, rootId), JSON.stringify(state.media))"
    ));
    assert!(BROWSER_CLIENT.contains("thread.textContent = replyCount === 0 ? \"🧵\""));
    assert!(BROWSER_CLIENT.contains("footer.append(thread);"));
    assert!(BROWSER_CLIENT.contains("message.parent_message_id"));
    assert!(BROWSER_CLIENT.contains(".thread-panel { width: 100vw"));
    assert!(BROWSER_CLIENT.contains("sendCommand(\"load_thread\""));
    assert!(BROWSER_CLIENT.contains("sendCommand(\"list_thread_summaries\""));
    assert!(BROWSER_CLIENT.contains("sendCommand(\"mark_thread_read\""));
    assert!(BROWSER_CLIENT.contains("event.type === \"thread_loaded\""));
    assert!(BROWSER_CLIENT.contains("summary?.unread_count"));
    assert!(BROWSER_CLIENT.contains("pendingThreadToOpen = mention.message.parent_message_id"));
}

#[test]
fn browser_uses_compact_accessible_mobile_conversation_bar() {
    assert!(BROWSER_CLIENT.contains(
        "<div class=\"mobile-app-mark\"><img src=\"/assets/sproyt-wave.svg\" alt=\"\"></div>"
    ));
    assert!(
        BROWSER_CLIENT.contains("class=\"conversation-circle\" id=\"conversation-circle\" hidden")
    );
    assert!(
        BROWSER_CLIENT
            .contains("class=\"conversation-context\" id=\"conversation-context\" hidden")
    );
    assert!(BROWSER_CLIENT.contains(
            "id=\"connection-status-toggle\" type=\"button\" aria-expanded=\"false\" aria-controls=\"status\""
        ));
    assert!(BROWSER_CLIENT.contains("aria-label=\"Opne menyen\""));
    assert!(BROWSER_CLIENT.contains("grid-template-rows: 52px minmax(0, 1fr) auto;"));
    assert!(BROWSER_CLIENT.contains(
        ".composer-area { position: relative; z-index: 4; grid-column: 2; grid-row: 3; }"
    ));
    assert!(BROWSER_CLIENT.contains(".composer-area { grid-column: 1; grid-row: 3; }"));
    assert!(BROWSER_CLIENT.contains(".sidebar.mobile-open { position: absolute; top: 52px;"));
    assert!(BROWSER_CLIENT.contains(".conversation-header { position: sticky; top: 0;"));
    assert!(BROWSER_CLIENT.contains("grid-template-columns: 32px minmax(0, 1fr) 44px 44px 44px;"));
    assert!(BROWSER_CLIENT.contains("width: 44px; min-width: 44px; min-height: 44px;"));
    assert!(BROWSER_CLIENT.contains("connectionStatusToggle.setAttribute(\"aria-label\", `Sambandsstatus: ${connection.status}`)"));
    assert!(BROWSER_CLIENT.contains("conversationCircle.textContent = channel.circle_id"));
    assert!(
        BROWSER_CLIENT.contains("connectionStatusDot.dataset.reconnecting = String(reconnecting)")
    );
    assert!(BROWSER_CLIENT.contains(".connection-status-dot[data-reconnecting=\"true\"]"));
    assert!(BROWSER_CLIENT.contains("(channel.direct_user_id ? \"Direktemelding\" : \"Felles\")"));
    assert!(BROWSER_CLIENT.contains("sidebar.setAttribute(\"aria-label\", \"Sprøyt-meny\")"));
    assert!(BROWSER_CLIENT.contains("firstControl?.focus()"));
    assert!(
        BROWSER_CLIENT
            .contains("event.key === \"Tab\" && sidebar.classList.contains(\"mobile-open\")")
    );
    assert!(BROWSER_CLIENT.contains("messagesEl.inert = open"));
}

#[test]
fn browser_keeps_conversation_dense_with_accessible_message_actions() {
    assert!(BROWSER_CLIENT.contains(
            ".conversation-header { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 50px; padding: 6px 12px; }"
        ));
    assert!(BROWSER_CLIENT.contains(".messages {\n        align-content: start;\n        display: grid;\n        gap: 8px;\n        padding: 12px;"));
    assert!(BROWSER_CLIENT.contains("padding: 7px 9px;"));
    assert!(BROWSER_CLIENT.contains(".rendered {\n        display: grid;\n        gap: 7px;"));
    assert!(BROWSER_CLIENT.contains(
            ".message-menu > summary,\n        .message-menu button,\n        .thread-link,\n        .reaction-badge,\n        .reaction-picker summary { min-height: 44px; }"
        ));
    assert!(BROWSER_CLIENT.contains("className = \"message-menu\""));
    assert!(
        BROWSER_CLIENT.contains("function placeMessageMenu(card, footer, menu, thread, messageId)")
    );
    assert!(BROWSER_CLIENT.contains("menu.classList.add(\"footer-menu\")"));
    assert!(BROWSER_CLIENT.contains("footer.insertBefore(menu, null)"));
    assert!(
        BROWSER_CLIENT.contains(".message-menu.footer-menu + .thread-link { margin-left: 0; }")
    );
    assert!(BROWSER_CLIENT.contains("Fleire handlingar for meldinga"));
    assert!(BROWSER_CLIENT.contains("Legg til reaksjon"));
    assert!(BROWSER_CLIENT.contains("message.sender_id === currentParticipantId"));
    assert!(BROWSER_CLIENT.contains("reaction-picker-requested"));
}

#[test]
fn browser_exposes_channel_members_and_owner_managed_markdown_description() {
    assert!(BROWSER_CLIENT.contains("id=\"channel-people\""));
    assert!(BROWSER_CLIENT.contains(".conversation-header .channel-people { order: 3; }"));
    assert!(
        BROWSER_CLIENT
            .contains(".channel-people { width: 36px; min-width: 36px; min-height: 36px;")
    );
    assert!(BROWSER_CLIENT.contains(".channel-people { width: 44px; min-width: 44px;"));
    assert!(BROWSER_CLIENT.contains(".channel-details-dialog > header { display: flex; align-items: center; justify-content: space-between;"));
    assert!(BROWSER_CLIENT.contains(".channel-details-dialog > header button { width: 40px; min-width: 40px; min-height: 40px; padding: 0;"));
    assert!(BROWSER_CLIENT.contains(".channel-details-dialog-body { display: grid; gap: 14px; padding: 14px; overflow-y: auto; }"));
    assert!(BROWSER_CLIENT.contains("id=\"channel-member-search\" type=\"search\""));
    assert!(BROWSER_CLIENT.contains("max-height: min(454px, 45dvh)"));
    assert!(BROWSER_CLIENT.contains("overscroll-behavior: contain"));
    assert!(BROWSER_CLIENT.contains("channelMemberSearch.addEventListener(\"input\""));
    assert!(BROWSER_CLIENT.contains(".normalize(\"NFKD\")"));
    assert!(BROWSER_CLIENT.contains("`Viser ${visibleUsers.length} av ${users.length}`"));
    assert!(BROWSER_CLIENT.contains("function requestChannelMembers(channelId)"));
    assert!(
        BROWSER_CLIENT.contains("sendCommand(\"list_channel_users\", { channel_id: channelId })")
    );
    assert!(BROWSER_CLIENT.contains("showChannelMemberLoadError(channelId"));
    assert!(BROWSER_CLIENT.contains("retry.textContent = \"Prøv igjen\""));
    assert!(BROWSER_CLIENT.contains("requestChannelMembers(channel.id)"));
    assert!(BROWSER_CLIENT.contains("event.type === \"channel_users_listed\""));
    assert!(BROWSER_CLIENT.contains("id=\"channel-member-add\" hidden"));
    assert!(BROWSER_CLIENT.contains("<strong>Legg til i kanalen</strong>"));
    assert!(
        BROWSER_CLIENT
            .contains("id=\"invite-channel-member\" type=\"button\" disabled>Inviter</button>")
    );
    assert!(
        BROWSER_CLIENT
            .contains("const pendingChannelInvitationRecipients = new Map<string, string>()")
    );
    assert!(BROWSER_CLIENT.contains("pendingDirectInvitationMessages.set(directRequestId"));
    assert!(BROWSER_CLIENT.contains(
        "sendCommand(\"send_message\", { channel_id: channel.id, body: directInvitationMessage })"
    ));
    assert!(BROWSER_CLIENT.contains("`[[invite:${event.payload.invitation.token}]]`"));
    assert!(
        BROWSER_CLIENT.contains(
            "channelMemberAdd.hidden = ![\"owner\", \"moderator\"].includes(channel.role)"
        )
    );
    assert!(BROWSER_CLIENT.contains("function refreshChannelMemberOptions(channelId)"));
    assert!(BROWSER_CLIENT.contains(
            "const eligibleUsers = channel?.circle_id ? (knownCircleUsers.get(channel.circle_id) || []) : knownUsers"
        ));
    assert!(BROWSER_CLIENT.contains(
            "if (channel.circle_id) sendCommand(\"list_circle_users\", { circle_id: channel.circle_id })"
        ));
    assert!(BROWSER_CLIENT.contains("!memberIds.has(user.id)"));
    assert!(BROWSER_CLIENT.contains("channelDetailsDialog.dataset.channelId"));
    assert!(!BROWSER_CLIENT.contains("Bli med i kanal"));
    assert!(!BROWSER_CLIENT.contains("Legg til i vald kanal"));
    assert!(BROWSER_CLIENT.contains("channelDescriptionForm.hidden = channel.role !== \"owner\""));
    assert!(BROWSER_CLIENT.contains("sendCommand(\"update_channel_description\""));
    assert!(BROWSER_CLIENT.contains("renderMarkdown(channel.description, conversationContext)"));
    assert!(BROWSER_CLIENT.contains("maxlength=\"2000\""));
}

#[test]
fn browser_uses_one_complete_theme_contract_for_dark_mode_controls() {
    assert!(BROWSER_CLIENT.contains(
        "<meta name=\"theme-color\" content=\"#111613\" media=\"(prefers-color-scheme: dark)\">"
    ));
    assert!(BROWSER_CLIENT.contains("color-scheme: light dark;"));
    assert!(BROWSER_CLIENT.contains("accent-color: var(--accent);"));
    assert!(
        BROWSER_CLIENT.contains("input,\n      textarea,\n      select {\n        width: 100%;")
    );
    assert!(BROWSER_CLIENT.contains(
            "select option,\n      select optgroup {\n        background-color: var(--control);\n        color: var(--ink);"
        ));
    assert!(BROWSER_CLIENT.contains("@media (prefers-color-scheme: dark)"));
    assert!(BROWSER_CLIENT.contains("--control: #111713;"));
    assert!(BROWSER_CLIENT.contains("--ink: #f2f6f2;"));
    assert!(BROWSER_CLIENT.contains(
            ".bottom-navigation-list button[aria-current=\"page\"] { background: var(--surface-hover); color: var(--ink); }"
        ));
    assert!(BROWSER_CLIENT.contains(
            ".channel-button:hover, .channel-button[aria-current=\"page\"] { background: var(--surface-hover); color: var(--ink); }"
        ));
}

#[test]
fn browser_refreshes_unread_summaries_when_a_background_tab_returns() {
    assert!(BROWSER_CLIENT.contains(
            "if (document.visibilityState !== \"visible\") return;\n        resumeAfterBackground();\n        sendCommand(\"list_my_channels\");"
        ));
}

#[test]
fn browser_linkifies_safe_web_urls_without_expanding_messages() {
    assert!(BROWSER_CLIENT.contains("function appendLinkedText(parent, text)"));
    assert!(BROWSER_CLIENT.contains("const urlPattern = /https?:\\/\\/[^\\s<>]+/gi"));
    assert!(BROWSER_CLIENT.contains("link.rel = \"noopener noreferrer\""));
    assert!(BROWSER_CLIENT.contains("link.referrerPolicy = \"no-referrer\""));
    assert!(BROWSER_CLIENT.contains("function readableLinkLabel(href)"));
    assert!(
        BROWSER_CLIENT.contains(".rendered a { overflow-wrap: anywhere; word-break: break-word; }")
    );
    assert!(BROWSER_CLIENT.contains("min-width: 0;\n        max-width: 100%;"));
}

async fn start_test_server(
    repository: Arc<SqliteChatRepository>,
    websocket_idle_timeout: Duration,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let (address, server, _) =
        start_test_server_with_state(repository, websocket_idle_timeout).await;
    (address, server)
}

async fn start_test_server_with_state(
    repository: Arc<SqliteChatRepository>,
    websocket_idle_timeout: Duration,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>, AppState) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let chat_repository: Arc<dyn crate::domain::ChatRepository> = repository.clone();
    let process_repository: Arc<dyn ProcessRepository> = repository.clone();
    let agent_repository: Arc<dyn AgentRepository> = repository;
    let operations = OperationalState::default();
    operations.set_ready(true);
    let state = AppState {
        auth: AuthService::development(),
        chat: ChatEngine::start(chat_repository),
        operations: operations.clone(),
        processes: ProcessService::start(process_repository, None),
        agents: AgentService::new(agent_repository),
        notifications: NotificationService::test(),
        websocket_idle_timeout,
        advanced_ui_enabled: false,
        agent_ui_enabled: false,
    };
    let app = build_router(state.clone(), operations);
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (address, server, state)
}

async fn start_postgres_test_server(
    repository: Arc<PostgresChatRepository>,
    websocket_idle_timeout: Duration,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let chat_repository: Arc<dyn crate::domain::ChatRepository> = repository.clone();
    let process_repository: Arc<dyn ProcessRepository> = repository.clone();
    let agent_repository: Arc<dyn AgentRepository> = repository;
    let operations = OperationalState::default();
    operations.set_ready(true);
    let state = AppState {
        auth: AuthService::development(),
        chat: ChatEngine::start(chat_repository),
        operations: operations.clone(),
        processes: ProcessService::start(process_repository, None),
        agents: AgentService::new(agent_repository),
        notifications: NotificationService::test(),
        websocket_idle_timeout,
        advanced_ui_enabled: false,
        agent_ui_enabled: false,
    };
    let app = build_router(state, operations);
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (address, server)
}

async fn start_test_server_with_gateway(
    repository: Arc<SqliteChatRepository>,
    gateway: SharedProcessGateway,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let chat_repository: Arc<dyn crate::domain::ChatRepository> = repository.clone();
    let process_repository: Arc<dyn ProcessRepository> = repository.clone();
    let agent_repository: Arc<dyn AgentRepository> = repository;
    let operations = OperationalState::default();
    operations.set_ready(true);
    let state = AppState {
        auth: AuthService::development(),
        chat: ChatEngine::start(chat_repository),
        operations: operations.clone(),
        processes: ProcessService::start(process_repository, Some(gateway)),
        agents: AgentService::new(agent_repository),
        notifications: NotificationService::test(),
        websocket_idle_timeout: Duration::from_secs(60),
        advanced_ui_enabled: false,
        agent_ui_enabled: false,
    };
    let app = build_router(state, operations);
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (address, server)
}

#[derive(Clone)]
struct RecoverableHeartState {
    available: Arc<AtomicBool>,
    starts: Arc<AtomicUsize>,
    instance_id: uuid::Uuid,
}

async fn recoverable_heart_start(State(state): State<RecoverableHeartState>) -> impl IntoResponse {
    state.starts.fetch_add(1, Ordering::SeqCst);
    if !state.available.load(Ordering::SeqCst) {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"temporarily unavailable"})),
        );
    }
    (
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({"instance_id":state.instance_id})),
    )
}

async fn recoverable_heart_gateway() -> (SharedProcessGateway, RecoverableHeartState) {
    let state = RecoverableHeartState {
        available: Arc::new(AtomicBool::new(false)),
        starts: Arc::new(AtomicUsize::new(0)),
        instance_id: uuid::Uuid::now_v7(),
    };
    let app = Router::new()
        .route("/api/v1/instances", post(recoverable_heart_start))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let gateway =
        HeartGateway::new(format!("http://{address}"), Duration::from_secs(1), 0).unwrap();
    (Arc::new(gateway), state)
}

async fn connect(address: std::net::SocketAddr) -> TestSocket {
    connect_as(address, "capacity-user").await
}

async fn connect_as(address: std::net::SocketAddr, participant: &str) -> TestSocket {
    let url = format!("ws://{address}/ws?participant={participant}");
    connect_async(url).await.unwrap().0
}

#[tokio::test]
async fn owner_revokes_agent_and_existing_mcp_credential_immediately_fails() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server, state) =
        start_test_server_with_state(repository, Duration::from_secs(60)).await;
    let owner_principal = state
        .auth
        .authenticate_request(Some("agent-owner".to_owned()), None)
        .await
        .unwrap();
    state.chat.ensure_user(owner_principal.user).await.unwrap();

    let client = reqwest::Client::new();
    let created = client
        .post(format!(
            "http://{address}/api/v1/agents?participant=agent-owner"
        ))
        .json(&serde_json::json!({
            "display_name":"Revocable agent",
            "provider":"contract",
            "service_identity":"revocable-agent",
            "purpose":"revocation route contract",
            "rate_limit_per_minute":60,
            "expires_at":null
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), axum::http::StatusCode::CREATED);
    assert_eq!(created.headers()["cache-control"], "no-store");
    let created: serde_json::Value = created.json().await.unwrap();
    let agent_id = created["agent_id"].as_str().unwrap();
    let credential = created["credential"].as_str().unwrap();
    let mcp_request = serde_json::json!({
        "jsonrpc":"2.0",
        "id":"initialize",
        "method":"initialize",
        "params":{"protocolVersion":MCP_PROTOCOL_VERSION,"capabilities":{},"clientInfo":{"name":"revocation-test","version":"1"}}
    });
    let before = client
        .post(format!("http://{address}/mcp"))
        .bearer_auth(credential)
        .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
        .header("accept", "application/json, text/event-stream")
        .json(&mcp_request)
        .send()
        .await
        .unwrap();
    assert_eq!(before.status(), axum::http::StatusCode::OK);

    let revoked = client
        .post(format!(
            "http://{address}/api/v1/agents/{agent_id}/revoke?participant=agent-owner"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(revoked.status(), axum::http::StatusCode::NO_CONTENT);
    let after = client
        .post(format!("http://{address}/mcp"))
        .bearer_auth(credential)
        .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
        .header("accept", "application/json, text/event-stream")
        .json(&mcp_request)
        .send()
        .await
        .unwrap();
    assert_eq!(after.status(), axum::http::StatusCode::UNAUTHORIZED);
    server.abort();
}

#[tokio::test]
async fn browser_entrypoint_uses_per_response_csp_and_security_headers() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;

    let first = reqwest::get(format!("http://{address}/")).await.unwrap();
    assert_eq!(first.status(), reqwest::StatusCode::OK);
    let headers = first.headers().clone();
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert_eq!(headers["x-frame-options"], "DENY");
    assert_eq!(headers["referrer-policy"], "no-referrer");
    assert_eq!(headers["cross-origin-opener-policy"], "same-origin");
    assert_eq!(headers["cache-control"], "no-store");
    let policy = headers["content-security-policy"].to_str().unwrap();
    assert!(policy.contains("object-src 'none'"));
    assert!(policy.contains("frame-ancestors 'none'"));
    assert!(policy.contains("script-src 'self' 'nonce-"));
    assert!(policy.contains("worker-src 'self'"));
    let nonce = policy
        .split("script-src 'self' 'nonce-")
        .nth(1)
        .unwrap()
        .split('\'')
        .next()
        .unwrap()
        .to_owned();
    let body = first.text().await.unwrap();
    let app_fingerprint = app_bundle_fingerprint(BUILD_REVISION, APP_BUNDLE.as_bytes());
    assert!(body.contains(&format!(
        "<script type=\"module\" nonce=\"{nonce}\" src=\"/assets/app/{app_fingerprint}/app.js\"></script>"
    )));
    assert_eq!(body.matches("<script").count(), 1);
    assert!(
        INDEX_HTML
            .contains("<script type=\"module\" nonce=\"{{NONCE}}\" src=\"{{APP_URL}}\"></script>")
    );
    assert!(!body.contains("function syncAppViewportHeight() {"));
    assert!(body.contains(&format!("<style nonce=\"{nonce}\">")));
    assert!(
        BROWSER_CLIENT
            .contains("https://cdn.jsdelivr.net/npm/mermaid@11.16.0/dist/mermaid.esm.min.mjs")
    );
    assert!(!BROWSER_CLIENT.contains("import mermaid from"));
    assert!(BROWSER_CLIENT.contains("mermaidPromise = import("));
    assert!(!BROWSER_CLIENT.contains("npm/mermaid@11/dist/"));
    assert!(!body.contains("{{NONCE}}"));
    assert!(!body.contains("{{APP_URL}}"));
    assert!(!body.contains("{{DISPLAY_NAME}}"));
    assert!(!body.contains("{{AGENT_HIDDEN}}"));
    assert!(body.contains("Innlogga som <strong>guest</strong>"));
    assert!(!body.contains("id=\"participant\""));
    assert!(
        BROWSER_CLIENT.contains("const url = new URL(`${protocol}://${window.location.host}/ws`)")
    );
    assert!(
        CONNECTION_SOURCE.contains("const nextSocket = createSocket(dependencies.websocketUrl())")
    );
    assert!(!BROWSER_CLIENT.contains("let subscribedChannelId = null"));
    assert!(
        BROWSER_CLIENT
            .contains("connectionSupervisor.snapshot().subscribedChannelId === activeChannelId")
    );
    assert!(BROWSER_CLIENT.contains("channel.id === activeChannelId && channel.id === connectionSupervisor.snapshot().subscribedChannelId"));
    assert!(BROWSER_CLIENT.contains("payload.channel_id !== activeChannelId"));
    assert!(BROWSER_CLIENT.contains("const pendingMessages = new Map<string, PendingMessage>()"));

    let service_worker = reqwest::get(format!("http://{address}/service-worker.js"))
        .await
        .unwrap();
    assert_eq!(service_worker.status(), reqwest::StatusCode::OK);
    assert_eq!(service_worker.headers()["cache-control"], "no-cache");
    let worker_policy = service_worker.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(worker_policy.contains("default-src 'none'"));
    assert!(worker_policy.contains("connect-src 'self'"));

    let app_url = format!("http://{address}/assets/app/{app_fingerprint}/app.js");
    let app = reqwest::get(&app_url).await.unwrap();
    assert_eq!(app.status(), reqwest::StatusCode::OK);
    assert_eq!(
        app.headers()["content-type"],
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        app.headers()["cache-control"],
        "public, max-age=31536000, immutable"
    );
    assert!(
        app.text()
            .await
            .unwrap()
            .contains("function syncAppViewportHeight()")
    );
    let stale_app = reqwest::get(format!("http://{address}/assets/app/stale-revision/app.js"))
        .await
        .unwrap();
    assert_eq!(stale_app.status(), reqwest::StatusCode::NOT_FOUND);

    let client_store_fingerprint =
        client_store_fingerprint(BUILD_REVISION, CLIENT_STORE.as_bytes());
    let client_store_url =
        format!("http://{address}/assets/client-store/{client_store_fingerprint}/client-store.js");
    let client_store = reqwest::get(&client_store_url).await.unwrap();
    assert_eq!(client_store.status(), reqwest::StatusCode::OK);
    assert_eq!(
        client_store.headers()["content-type"],
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        client_store.headers()["cache-control"],
        "public, max-age=31536000, immutable"
    );
    let client_store_body = client_store.text().await.unwrap();
    assert!(client_store_body.contains("function createApplicationStore()"));
    assert!(client_store_body.contains("function createServerEventMailbox({"));
    assert!(
        client_store_body
            .contains("export {\n  createApplicationStore,\n  createServerEventMailbox\n};")
    );
    let legacy_client_store = reqwest::get(format!("http://{address}/assets/client-store.js"))
        .await
        .unwrap();
    assert_eq!(legacy_client_store.status(), reqwest::StatusCode::OK);
    assert_eq!(legacy_client_store.headers()["cache-control"], "no-cache");
    assert_eq!(
        legacy_client_store.headers()["content-type"],
        "text/javascript; charset=utf-8"
    );
    let stale_client_store = reqwest::get(format!(
        "http://{address}/assets/client-store/stale-revision/client-store.js"
    ))
    .await
    .unwrap();
    assert_eq!(stale_client_store.status(), reqwest::StatusCode::NOT_FOUND);
    assert!(!BROWSER_CLIENT.contains("id=\"channel-kind\""));
    assert!(!BROWSER_CLIENT.contains("id=\"create-circle-channel\""));
    assert!(!BROWSER_CLIENT.contains("id=\"create-channel-invitation\""));
    assert!(BROWSER_CLIENT.contains("id=\"circle-joinable-list\""));
    assert!(!BROWSER_CLIENT.contains("id=\"joinable-channel\""));
    assert!(BROWSER_CLIENT.contains("id=\"add-channel-member\""));
    assert!(BROWSER_CLIENT.contains("function scopedCircleChannelSlug(circleId, value)"));
    assert!(BROWSER_CLIENT.contains("scopedCircleChannelSlug(managedCircleId, name)"));
    assert!(BROWSER_CLIENT.contains("scopedCircleChannelSlug(event.payload.circle.id, \"prat\")"));
    assert!(BROWSER_CLIENT.contains(
        "knownCircles.set(event.payload.circle.id, { ...event.payload.circle, role: \"owner\" })"
    ));
    assert!(BROWSER_CLIENT.contains("const activeCircleKey = \"sproyt.active-circle.v1\""));
    assert!(BROWSER_CLIENT.contains("function restoreActiveCircle()"));
    assert!(NAVIGATION_SOURCE.contains("const candidate = [this.#activeCircleId"));
    assert!(NAVIGATION_SOURCE.contains("writeStorage(this.storage, activeCircleKey, circleId)"));
    assert!(BROWSER_CLIENT.contains("const restoredCircle = restoreActiveCircle();"));
    assert!(BROWSER_CLIENT.contains("if (restoredCircle) sendCommand(\"list_joinable_channels\", { circle_id: restoredCircle });"));
    assert!(BROWSER_CLIENT.contains("clearActiveCircle(deletedCircleId);"));
    assert!(BROWSER_CLIENT.contains("clearActiveCircle(departedCircleId);"));
    assert!(NAVIGATION_SOURCE.contains(
        "if (channel.circle_id) { this.setActiveCircle(channel.circle_id); this.rememberCircleChannel(channel); }"
    ));
    assert!(BROWSER_CLIENT.contains("Kanalen kunne ikkje opprettast."));
    assert!(BROWSER_CLIENT.contains("sendCommand(\"list_joinable_channels\""));
    assert!(BROWSER_CLIENT.contains("sendCommand(\"add_channel_member\""));
    assert!(BROWSER_CLIENT.contains("const browserSessionId = `browser-${crypto.randomUUID()}`"));
    assert!(BROWSER_CLIENT.contains("return `${browserSessionId}-${requestNumber}`"));
    assert!(BROWSER_CLIENT.contains(
        "if (command.type === \"list_my_channels\") latestChannelListRequestId = requestId;"
    ));
    assert!(
        BROWSER_CLIENT.contains("if (event.request_id !== latestChannelListRequestId) return;")
    );
    assert!(BROWSER_CLIENT.contains("if (event.request_id !== latestCircleListRequestId) return;"));
    assert!(!BROWSER_CLIENT.contains("return `browser-${requestNumber}`"));
    assert!(BROWSER_CLIENT.contains("if (event.type === \"message_accepted\")"));
    assert!(
        BROWSER_CLIENT.contains("finishPendingMessage(event.request_id, event.payload.message)")
    );
    assert!(BROWSER_CLIENT.contains("message?.channel_id !== pending.channelId"));
    assert!(BROWSER_CLIENT.contains("message?.body !== pending.body"));
    assert!(BROWSER_CLIENT.contains("failPendingMessage(event.request_id"));
    assert!(BROWSER_CLIENT.contains(
            "pendingMessages.set(requestId, { body, draft, mediaIds: channelMedia.map((media) => media.id), channelId: activeChannelId });\n        bodyInput.value = \"\";"
        ));
    assert!(BROWSER_CLIENT.contains("bodyInput.value = pending.draft"));
    assert!(BROWSER_CLIENT.contains("const channelDraftPrefix = \"sproyt.channel-draft.v1.\""));
    assert!(BROWSER_CLIENT.contains("function persistActiveDraft()"));
    assert!(BROWSER_CLIENT.contains("function restoreActiveDraft()"));
    assert!(NAVIGATION_SOURCE.contains("writeStorage(this.storage, key, draft)"));
    assert!(BROWSER_CLIENT.contains(
            "if (channel.id === activeChannelId && channel.id === connectionSupervisor.snapshot().subscribedChannelId) return;\n        persistActiveDraft();"
        ));
    assert!(BROWSER_CLIENT.contains(
        "navigation.setActiveChannel(channel);\n        syncRenderedNavigation();\n        restoreActiveDraft();"
    ));
    assert!(body.contains("class=\"advanced-tools\" hidden"));
    assert!(body.contains("<details class=\"agent-access\" hidden>"));
    assert!(BROWSER_CLIENT.contains(
        "<summary data-tooltip=\"Agenttilgang\" title=\"Agenttilgang\">Agenttilgang</summary>"
    ));
    assert!(BROWSER_CLIENT.contains("id=\"create-agent-access\""));
    assert!(BROWSER_CLIENT.contains("function createTemporaryAgentAccess()"));
    assert!(BROWSER_CLIENT.contains("[\"read_history\", \"send_messages\"]"));
    assert!(BROWSER_CLIENT.contains("function revokeTemporaryAgentAccess()"));
    assert!(
        BROWSER_CLIENT.contains("channel?.role === \"owner\" || channel?.role === \"moderator\"")
    );
    assert!(BROWSER_CLIENT.contains(
            "agentAccessNotice.textContent = \"Klar til å lage kortliva agenttilgang for denne samtalen.\""
        ));
    assert!(BROWSER_CLIENT.contains(
            "updateAgentAccessControls();\n          agentAccessNotice.textContent = \"Agenttilgangen er trekt tilbake.\";"
        ));
    assert!(CONNECTION_SOURCE.contains("start: (): void => connectSocket()"));
    assert!(CONNECTION_SOURCE.contains("scheduleReconnect:"));
    assert!(CONNECTION_SOURCE.contains("state.stableConnectionTimer = dependencies.setTimeout"));
    assert!(CONNECTION_SOURCE.contains("closeEvent.code === 1008"));
    assert!(CONNECTION_SOURCE.contains("dependencies.onAuthenticationFailure().catch"));
    assert!(BROWSER_CLIENT.contains("async function recoverConnection(replaceOpenSocket = false)"));
    assert!(BROWSER_CLIENT.contains("response.status === 401"));
    assert!(BROWSER_CLIENT.contains("connectionSupervisor.connect(true, true)"));
    assert!(
        CONNECTION_SOURCE
            .contains("dependencies.recover().catch(() => controller.scheduleReconnect")
    );
    assert!(BROWSER_CLIENT.contains("fetch(\"/auth/session\""));
    assert!(BROWSER_CLIENT.contains("sessionController.start().catch"));
    assert!(SESSION_SOURCE.contains("state.refreshDueAt = dependencies.now() + delay"));
    assert!(
        BROWSER_CLIENT.contains("window.addEventListener(\"pageshow\", resumeAfterBackground)")
    );
    assert!(BROWSER_CLIENT.contains("window.addEventListener(\"online\", resumeAfterBackground)"));
    assert!(
        BROWSER_CLIENT
            .contains("onSessionRotated: () => connectionSupervisor.replaceAfterSessionRefresh()")
    );
    assert!(SESSION_SOURCE.contains("dependencies.onStatus(\"Fornyar økta …\")"));
    assert!(BROWSER_CLIENT.contains("let lastUserActivityAt = Date.now()"));
    assert!(BROWSER_CLIENT.contains("function noteUserActivity()"));
    assert!(BROWSER_CLIENT.contains("window.addEventListener(\"pointerdown\", noteUserActivity"));
    assert!(SESSION_SOURCE.contains("if (await useCurrentSession())"));
    assert!(
        SESSION_SOURCE.contains("dependencies.now() - dependencies.lastUserActivityAt() < 120_000")
    );
    assert!(SESSION_SOURCE.contains("vi ventar så du ikkje mistar arbeidet ditt"));
    assert!(CONNECTION_SOURCE.contains("connectSocket(true)"));
    assert!(CONNECTION_SOURCE.contains("connectSocket(true, current)"));
    assert!(
        BROWSER_CLIENT.contains(
            "const next = invited || current || restored || requested || knownChannels[0]"
        )
    );
    assert!(BROWSER_CLIENT.contains("[[invite:${event.payload.invitation.token}]]"));
    assert!(BROWSER_CLIENT.contains("function renderInvitationCard(token, target)"));
    assert!(
        BROWSER_CLIENT
            .contains("const invitationInspectionCache = new Map<string, InvitationCache>()")
    );
    assert!(BROWSER_CLIENT.contains("if (cached?.status === \"pending\")"));
    assert!(
        BROWSER_CLIENT
            .contains("if (cached?.status === \"missing\" || cached?.status === \"failed\")")
    );
    assert!(BROWSER_CLIENT.contains("pendingInvitationInspections.set(requestId, token)"));
    assert!(BROWSER_CLIENT.contains("if (requestedCommand === \"inspect_invitation\")"));
    assert!(BROWSER_CLIENT.contains("showInvitationError(inspectedInvitationToken, message)"));
    assert!(
        BROWSER_CLIENT.contains(
            "respondToInvitation(token, \"accept_invitation\", \"Godtek invitasjonen …\")"
        )
    );
    assert!(!BROWSER_CLIENT.contains("sendCommand(\"accept_circle_invitation\", { token })"));
    assert!(BROWSER_CLIENT.contains(
        "respondToInvitation(token, \"decline_invitation\", \"Avviser invitasjonen …\")"
    ));
    assert!(
        BROWSER_CLIENT
            .contains("const authoredByMe = invitation.invited_by === currentParticipantId")
    );
    assert!(BROWSER_CLIENT.contains("Du må først vere medlem i vennekretsen"));
    assert!(
        BROWSER_CLIENT
            .contains("window.addEventListener(\"focus\", refreshVisibleInvitationCards)")
    );
    assert!(BROWSER_CLIENT.contains(
        "historyHasMore = false;\n            console.error(\"Kunne ikkje laste eldre meldingar\""
    ));
    // Navigation persistence is now owned by the typed controller rather than
    // ad-hoc DOM code, while retaining the durable active-channel behaviour.
    assert!(APP_SOURCE.contains("navigation.setActiveChannel(channel)"));
    assert!(NAVIGATION_SOURCE.contains("writeStorage(this.storage, activeChannelKey, channel.id)"));
    assert!(APP_SOURCE.contains("let reconnectScrollOffset: number | null = null"));
    assert!(BROWSER_CLIENT.contains("restoreConversationScrollOffset(scrollOffset)"));
    assert!(CONNECTION_SOURCE.contains("previousSocket.close(4000, \"session refreshed\")"));
    assert!(!BROWSER_CLIENT.contains("sessionRefreshReconnect"));
    assert!(
        !BROWSER_CLIENT
            .contains("if (response.status === 401) {\n          window.location.assign")
    );
    assert!(!BROWSER_CLIENT.contains("window.location.reload()"));
    assert!(CONNECTION_SOURCE.contains("Fråkopla (${detail})"));
    assert!(BROWSER_CLIENT.contains("function acknowledgeLatest(channelId, messages)"));
    assert!(BROWSER_CLIENT.contains("function loadOlderHistory()"));
    assert!(BROWSER_CLIENT.contains("before: oldest.sequence"));
    assert!(BROWSER_CLIENT.contains("renderTimeline({ preserveScroll: true })"));
    assert!(
        BROWSER_CLIENT.contains(
            "renderTimeline({ forceBottom: scrollOffset === null || scrollOffset < 80 })"
        )
    );
    assert!(BROWSER_CLIENT.contains("function settleConversationAtBottom()"));
    assert!(!BROWSER_CLIENT.contains("sendForm.scrollIntoView"));
    assert!(BROWSER_CLIENT.contains("const offsetTop = viewport?.offsetTop || 0"));
    assert!(
        BROWSER_CLIENT
            .contains("window.visualViewport?.addEventListener(\"scroll\", syncAppViewportHeight")
    );
    assert!(BROWSER_CLIENT.contains("transform: translateY(var(--app-offset-top))"));
    assert!(
        APP_SOURCE.contains(
            "function formatMessageTimestamp(sentAt: Date, now: Date = new Date()): string"
        )
    );
    assert!(BROWSER_CLIENT.contains("dateStyle: \"full\", timeStyle: \"short\""));
    assert!(BROWSER_CLIENT.contains("appendProfileStatus(senderLabel, message.sender_id)"));
    assert!(BROWSER_CLIENT.contains("channel.direct_user_id"));
    assert!(BROWSER_CLIENT.contains("function approximateUnreadCount(count)"));
    assert!(BROWSER_CLIENT.contains("if (count < 50) return \"25+\""));
    assert!(BROWSER_CLIENT.contains("if (count < 100) return \"50+\""));
    assert!(BROWSER_CLIENT.contains("button.classList.add(\"has-unread\")"));
    assert!(BROWSER_CLIENT.contains("if (unreadCount > 0) {"));
    assert!(BROWSER_CLIENT.contains("class=\"inbox-navigation\""));
    assert!(BROWSER_CLIENT.contains("id=\"unread-count\""));
    assert!(BROWSER_CLIENT.contains("id=\"mention-count\""));
    assert!(BROWSER_CLIENT.contains("id=\"task-count\""));
    assert!(BROWSER_CLIENT.contains("activeInboxKind = kind"));
    assert!(BROWSER_CLIENT.contains("className = \"unread-inbox\""));
    assert!(BROWSER_CLIENT.contains("className = \"unread-card\""));
    assert!(BROWSER_CLIENT.contains("function openChannelManagement(circleId)"));
    assert!(!BROWSER_CLIENT.contains("Samtalar og vennekretsar"));
    assert!(!BROWSER_CLIENT.contains("id=\"channel-list\""));
    assert!(BROWSER_CLIENT.contains("leave.textContent = `Forlat # ${activeChannel.name}`"));
    assert!(BROWSER_CLIENT.contains("sendCommand(\"leave_channel\""));
    assert!(BROWSER_CLIENT.contains("event.type === \"membership_left\""));
    assert!(BROWSER_CLIENT.contains("activeChannel.name.trim().toLocaleLowerCase() !== \"prat\""));
    assert!(BROWSER_CLIENT.contains("id=\"circle-channel-dialog\""));
    assert!(BROWSER_CLIENT.contains("function renderManagedJoinableChannels(channels)"));
    assert!(BROWSER_CLIENT.contains("+ Finn fleire kanalar"));
    assert!(BROWSER_CLIENT.contains("className = \"joinable-channel-description\""));
    assert!(BROWSER_CLIENT.contains("renderMarkdown(channel.description, description)"));
    assert!(BROWSER_CLIENT.contains("sendCommand(\"leave_circle\""));
    assert!(BROWSER_CLIENT.contains("event.type === \"circle_left\""));
    assert!(BROWSER_CLIENT.contains("circle.role === \"owner\""));
    assert!(BROWSER_CLIENT.contains("document.addEventListener(\"visibilitychange\""));
    assert!(BROWSER_CLIENT.contains(":focus-visible"));
    assert!(BROWSER_CLIENT.contains("id=\"mobile-navigation-toggle\""));
    assert!(BROWSER_CLIENT.contains("aria-controls=\"mobile-navigation\""));
    assert!(BROWSER_CLIENT.contains(
        "id=\"view-mode-toggle\" type=\"button\" role=\"switch\" aria-checked=\"false\""
    ));
    assert!(BROWSER_CLIENT.contains("class=\"view-mode-switch-icon\" aria-hidden=\"true\"><svg"));
    assert!(BROWSER_CLIENT.contains("setRenderMode(renderMode === \"raw\" ? \"view\" : \"raw\")"));
    assert!(
        BROWSER_CLIENT
            .contains("viewModeToggle.setAttribute(\"aria-checked\", String(showsSource))")
    );
    assert!(BROWSER_CLIENT.contains(".conversation-header .view-controls { display: none; }"));
    assert!(!BROWSER_CLIENT.contains("id=\"view-mode\""));
    assert!(!BROWSER_CLIENT.contains("id=\"raw-mode\""));
    assert!(
        BROWSER_CLIENT
            .contains("class=\"bottom-navigation\" aria-label=\"Område- og kanalveljar\"")
    );
    assert!(BROWSER_CLIENT.contains("</form>\n        <nav class=\"bottom-navigation\""));
    assert!(BROWSER_CLIENT.contains("id=\"bottom-channel-panel\""));
    assert!(BROWSER_CLIENT.contains("id=\"bottom-circle-panel\""));
    assert!(
        BROWSER_CLIENT.contains(".bottom-navigation-panel { position: relative; min-width: 0; }")
    );
    assert!(BROWSER_CLIENT.contains("bottom: calc(100% + 5px);"));
    assert!(BROWSER_CLIENT.contains(
        "if (event.target instanceof Node && bottomNavigation.contains(event.target)) return;"
    ));
    assert!(
        BROWSER_CLIENT
            .contains("bottomChannelPanel.open = false;\n        bottomCirclePanel.open = false;")
    );
    assert!(BROWSER_CLIENT.contains("function pendingMessageToReveal(message, requestId = null)"));
    assert!(BROWSER_CLIENT.contains("message.sender_id !== currentParticipantId"));
    assert!(BROWSER_CLIENT.contains(
        "renderTimeline({ revealMessageId: revealOwnMessage ? event.payload.message.id : null })"
    ));
    assert!(BROWSER_CLIENT.contains(
        "renderTimeline({ revealMessageId: revealOwnMessage ? chatEvent.message.id : null })"
    ));
    assert!(BROWSER_CLIENT.contains("function revealTimelineMessage(messageId)"));
    assert!(BROWSER_CLIENT.contains("const cardRect = card.getBoundingClientRect()"));
    assert!(BROWSER_CLIENT.contains("const viewportRect = messagesEl.getBoundingClientRect()"));
    assert!(BROWSER_CLIENT.contains("if (delta > 0) messagesEl.scrollTop += delta"));
    assert!(BROWSER_CLIENT.contains(
        "aria-label=\"Vel kanal\"><span class=\"bottom-navigation-label\"># Kanal</span>"
    ));
    assert!(BROWSER_CLIENT.contains(
        "aria-label=\"Vel område\"><span class=\"bottom-navigation-label\">◎ Felles</span>"
    ));
    assert!(BROWSER_CLIENT.contains("height: 40px;\n        min-height: 40px"));
    assert!(BROWSER_CLIENT.contains("function renderBottomNavigation()"));
    assert!(BROWSER_CLIENT.contains("const channelLabel = activeChannel"));
    assert!(BROWSER_CLIENT.contains(
        "const bottomCircleLabel = bottomCircleToggle.querySelector(\".bottom-navigation-label\");"
    ));
    assert!(BROWSER_CLIENT.contains("if (!(bottomCircleLabel instanceof HTMLElement)) throw new Error(\"Manglar områdemerke i botnnavigasjonen\");"));
    assert!(BROWSER_CLIENT.contains("bottomCircleLabel.textContent = `◎ ${circleLabel}`;"));
    assert!(BROWSER_CLIENT.contains(".message-menu:not([open]) > div { display: none; }"));
    assert!(BROWSER_CLIENT.contains(
        ".message .reaction-picker[open] { visibility: visible; pointer-events: auto; }"
    ));
    assert!(BROWSER_CLIENT.contains("card.classList.add(\"reaction-picker-requested\")"));
    assert!(BROWSER_CLIENT.contains("const scopedChannels = activeCircleId"));
    assert!(!BROWSER_CLIENT.contains("sharedButton.textContent = \"(Felles)\""));
    assert!(!BROWSER_CLIENT.contains("directButton.textContent = \"(Direkte)\""));
    assert!(BROWSER_CLIENT.contains("class=\"circle-tool-rail\" role=\"toolbar\""));
    assert!(BROWSER_CLIENT.contains(".circle-tool-rail { display: grid; justify-items: center;"));
    assert!(BROWSER_CLIENT.contains("font-size: 1.22rem; line-height: 1; text-align: center;"));
    assert!(BROWSER_CLIENT.contains("content: attr(aria-label)"));
    assert!(
        BROWSER_CLIENT.contains(
            ".circle-tool-button:hover::before, .circle-tool-button:focus-visible::before"
        )
    );
    assert!(!BROWSER_CLIENT.contains("id=\"circle-tool-circles\""));
    let shared_tool = body.find("id=\"circle-tool-shared\"").unwrap();
    let direct_tool = body.find("id=\"circle-tool-direct\"").unwrap();
    let settings_tool = body.find("id=\"circle-tool-settings\"").unwrap();
    assert!(shared_tool < direct_tool && direct_tool < settings_tool);
    assert!(BROWSER_CLIENT.contains("aria-label=\"Direkte samtalar\""));
    assert!(BROWSER_CLIENT.contains("aria-label=\"Felles\""));
    assert!(BROWSER_CLIENT.contains("id=\"circle-tool-settings\""));
    assert!(!BROWSER_CLIENT.contains("circleToolMode"));
    assert!(!BROWSER_CLIENT.contains("function setCircleToolMode(mode)"));
    assert!(BROWSER_CLIENT.contains("function activateRootScope(scope)"));
    assert!(BROWSER_CLIENT.contains(
        "circleToolDirect.addEventListener(\"click\", () => activateRootScope(\"direct\"))"
    ));
    assert!(BROWSER_CLIENT.contains(
        "circleToolShared.addEventListener(\"click\", () => activateRootScope(\"shared\"))"
    ));
    assert!(BROWSER_CLIENT.contains(
            "circleToolShared.setAttribute(\"aria-pressed\", String(!activeCircleId && activeRootScope === \"shared\"))"
        ));
    assert!(BROWSER_CLIENT.contains("if (circleAdminDialog.open) return"));
    assert!(BROWSER_CLIENT.contains("id=\"circle-admin-dialog\""));
    assert!(BROWSER_CLIENT.contains("if (!circleAdminDialog.open) circleAdminDialog.showModal()"));
    assert!(BROWSER_CLIENT.contains(
        "const directChannels = knownChannels.filter((channel) => channel.direct_user_id)"
    ));
    assert!(!BROWSER_CLIENT.contains("const primaryChannels = knownChannels.filter"));
    assert!(
        NAVIGATION_SOURCE
            .contains("const circleChannelHistoryKey = \"sproyt.active-channel-by-circle.v1\"")
    );
    assert!(NAVIGATION_SOURCE.contains("rememberCircleChannel(channel: NavigationChannel)"));
    assert!(
        BROWSER_CLIENT
            .contains("function preferredCircleChannel(circleId: string, channels: Channel[] = knownChannels): Channel | undefined")
    );
    assert!(NAVIGATION_SOURCE.contains("const remembered = available.find"));
    assert!(NAVIGATION_SOURCE.contains("channel.name.trim().toLocaleLowerCase() === \"prat\""));
    assert!(NAVIGATION_SOURCE.contains("return remembered ?? primary ?? available[0];"));
    assert!(
        BROWSER_CLIENT
            .contains("const preferredChannel = preferredCircleChannel(circleId, channels)")
    );
    assert!(BROWSER_CLIENT.contains("if (preferredChannel) selectChannel(preferredChannel)"));
    assert!(APP_SOURCE.contains("navigation.rememberCircleChannel(channel)"));
    assert!(BROWSER_CLIENT.contains("forgetCircleChannel(departedCircleId)"));
    assert!(
        BROWSER_CLIENT
            .contains("activeRootScope = channel.direct_user_id ? \"direct\" : \"shared\"")
    );
    assert!(BROWSER_CLIENT.contains("id=\"direct-message-dialog\""));
    assert!(
        BROWSER_CLIENT
            .contains("id=\"direct-message-status\" role=\"status\" aria-live=\"polite\"")
    );
    assert!(BROWSER_CLIENT.contains("startDirect.textContent = \"+ Ny samtale …\""));
    assert!(BROWSER_CLIENT.contains("function openDirectMessageDialog()"));
    assert!(
        BROWSER_CLIENT.contains("directMessageStatus.textContent = \"Hentar fersk personliste …\"")
    );
    assert!(BROWSER_CLIENT.contains("if (!sendCommand(\"list_users\"))"));
    assert!(BROWSER_CLIENT.contains("if (requestedCommand === \"open_direct_channel\")"));
    assert!(BROWSER_CLIENT.contains("Brukaren finst ikkje lenger. Lukk dialogen og prøv på nytt."));
    assert!(BROWSER_CLIENT.contains("activeProfile(channel?.direct_user_id)?.display_name"));
    assert!(BROWSER_CLIENT.contains("if (knownChannels.length > 0) renderChannels()"));
    assert!(!BROWSER_CLIENT.contains("heading.textContent = \"Andre samtalar\""));
    assert!(BROWSER_CLIENT.contains(
        "button.setAttribute(\"aria-current\", circleId === activeCircleId ? \"page\" : \"false\")"
    ));
    assert!(BROWSER_CLIENT.contains("button.classList.add(\"has-unread\")"));
    assert!(BROWSER_CLIENT.contains("function closeBottomNavigation(panel, toggle)"));
    assert!(BROWSER_CLIENT.contains("if (event.key === \"Escape\" && bottomChannelPanel.open)"));
    assert!(BROWSER_CLIENT.contains("padding-bottom: 6px;"));
    assert!(!BROWSER_CLIENT.contains("<summary>Administrer kretsar</summary>"));
    assert!(BROWSER_CLIENT.contains("<h2 id=\"circle-admin-title\">Administrer vennekretsar</h2>"));
    assert!(
        BROWSER_CLIENT.contains(".sidebar.mobile-open nav, .sidebar.mobile-open .agent-access")
    );
    assert!(BROWSER_CLIENT.contains(".sidebar.mobile-open .identity { display: grid;"));
    assert!(BROWSER_CLIENT.contains(".sidebar.mobile-open { position: absolute; top: 52px;"));
    assert!(BROWSER_CLIENT.contains("overflow-y: auto; overscroll-behavior: contain;"));
    assert!(BROWSER_CLIENT.contains("grid-template-rows: 52px minmax(0, 1fr) auto;"));
    assert!(BROWSER_CLIENT.contains("form.send { grid-template-columns: minmax(0, 1fr) auto"));
    assert!(BROWSER_CLIENT.contains(".connection-status-toggle[aria-expanded=\"true\"] + .status"));
    assert!(BROWSER_CLIENT.contains("setConnectionStatus(\"Tilkopla\")"));
    assert!(
        BROWSER_CLIENT
            .contains("mobileNavigationToggle.setAttribute(\"aria-expanded\", String(open))")
    );
    assert!(BROWSER_CLIENT.contains("event.key === \"Escape\""));
    assert!(BROWSER_CLIENT.contains("message.sender_display_name || \"Ein ven\""));
    assert!(BROWSER_CLIENT.contains("Invitasjonslenkje"));
    assert!(BROWSER_CLIENT.contains("Invitasjonen finst ikkje eller er ikkje gyldig lenger"));

    let second = reqwest::get(format!("http://{address}/")).await.unwrap();
    let second_policy = second.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(!second_policy.contains(&format!("'nonce-{nonce}'")));
    server.abort();
}

#[tokio::test]
async fn authenticated_client_events_are_bounded_and_exported_without_payload_data() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;
    let client = reqwest::Client::new();

    let accepted = client
        .post(format!(
            "http://{address}/api/v1/client-events?participant=telemetry-user"
        ))
        .json(&serde_json::json!({"event":"session_refresh_failed"}))
        .send()
        .await
        .unwrap();
    assert_eq!(accepted.status(), axum::http::StatusCode::NO_CONTENT);

    let rejected = client
        .post(format!(
            "http://{address}/api/v1/client-events?participant=telemetry-user"
        ))
        .json(&serde_json::json!({
            "event":"arbitrary_event",
            "message":"private text must never become a metric"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        rejected.status(),
        axum::http::StatusCode::UNPROCESSABLE_ENTITY
    );

    let metrics = client
        .get(format!("http://{address}/metrics"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(metrics.contains("sproyt_client_events_total{event=\"session_refresh_failed\"} 1"));
    assert!(!metrics.contains("private text"));
    assert!(!metrics.contains("telemetry-user"));
    server.abort();
}

#[tokio::test]
async fn websocket_upgrade_is_not_decorated_as_a_document_response() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;

    let url = format!("ws://{address}/ws?participant=upgrade-policy-user");
    let (mut socket, response) = connect_async(url).await.unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::SWITCHING_PROTOCOLS
    );
    assert!(!response.headers().contains_key("content-security-policy"));
    assert!(
        !response
            .headers()
            .contains_key("cross-origin-opener-policy")
    );

    socket
        .send(ClientMessage::Text(
            serde_json::json!({
                "protocol":"sproyt.chat.v1",
                "request_id":"upgrade-hello",
                "type":"hello"
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("hello response timed out")
        .expect("socket closed after upgrade")
        .unwrap();
    assert!(response.into_text().unwrap().contains("\"type\":\"hello\""));
    socket.close(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn security_headers_cover_operational_oidc_and_not_found_responses() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    for path in ["/healthz", "/versionz", "/auth/login", "/does-not-exist"] {
        let response = client
            .get(format!("http://{address}{path}"))
            .send()
            .await
            .unwrap();
        let headers = response.headers();
        assert_eq!(headers["x-content-type-options"], "nosniff", "{path}");
        assert_eq!(headers["x-frame-options"], "DENY", "{path}");
        assert_eq!(headers["referrer-policy"], "no-referrer", "{path}");
        assert_eq!(
            headers["cross-origin-opener-policy"], "same-origin",
            "{path}"
        );
        assert_eq!(headers["cache-control"], "no-store", "{path}");
        assert!(
            headers["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("default-src 'none'"),
            "{path}"
        );
    }
    let version: serde_json::Value = client
        .get(format!("http://{address}/versionz"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(version["service"], "sproyt");
    assert_eq!(version["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(version["revision"], BUILD_REVISION);
    server.abort();
}

#[test]
fn invitation_return_path_accepts_only_bounded_url_safe_tokens() {
    assert!(is_safe_invitation_token(
        "WGp_FxqwngrypwMMIvAh1CMLGC0OTkIY-FIwjPElISU"
    ));
    assert!(!is_safe_invitation_token("short"));
    assert!(!is_safe_invitation_token(
        "valid-length-but-has-a-query&next=https://evil.invalid"
    ));
    assert!(!is_safe_invitation_token(&"a".repeat(513)));
}

#[tokio::test]
async fn portable_export_is_private_complete_and_not_cacheable() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server, _) =
        start_test_server_with_state(repository, Duration::from_secs(60)).await;

    let mut owner = connect_as(address, "export-owner").await;
    let channel = command(
        &mut owner,
        "export-channel",
        "create_channel",
        serde_json::json!({"slug":"export-visible","name":"Export visible","kind":"private"}),
    )
    .await;
    let channel_id = channel["payload"]["channel"]["id"].clone();
    command(
        &mut owner,
        "export-message",
        "send_message",
        serde_json::json!({"channel_id":channel_id,"body":"portable visible body"}),
    )
    .await;

    let mut outsider = connect_as(address, "export-outsider").await;
    let hidden = command(
        &mut outsider,
        "hidden-channel",
        "create_channel",
        serde_json::json!({"slug":"export-hidden","name":"Export hidden","kind":"private"}),
    )
    .await;
    command(
        &mut outsider,
        "hidden-message",
        "send_message",
        serde_json::json!({"channel_id":hidden["payload"]["channel"]["id"],"body":"must not leak"}),
    )
    .await;

    let response = reqwest::get(format!(
        "http://{address}/api/v1/me/export?participant=export-owner"
    ))
    .await
    .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment; filename=\"sproyt-export-")
    );
    let export: serde_json::Value = response.json().await.unwrap();
    assert_eq!(export["format"], crate::domain::PORTABLE_USER_EXPORT_FORMAT);
    let exported_channel = export["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["channel"]["id"] == channel_id)
        .expect("the user's private channel must be exported alongside general");
    assert_eq!(
        exported_channel["messages"][0]["body"],
        "portable visible body"
    );
    assert!(!export.to_string().contains("must not leak"));
    server.abort();
}

async fn command(
    socket: &mut TestSocket,
    request_id: &str,
    command_type: &str,
    payload: serde_json::Value,
) -> serde_json::Value {
    let response = command_response(socket, request_id, command_type, payload).await;
    assert_ne!(response["type"], "error", "{response}");
    response
}

async fn command_response(
    socket: &mut TestSocket,
    request_id: &str,
    command_type: &str,
    payload: serde_json::Value,
) -> serde_json::Value {
    let mut envelope = serde_json::json!({
        "protocol": crate::protocol::PROTOCOL_ID,
        "request_id": request_id,
        "type": command_type,
    });
    if !payload.is_null() {
        envelope["payload"] = payload;
    }
    socket
        .send(ClientMessage::Text(envelope.to_string().into()))
        .await
        .unwrap();
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap_or_else(|_| {
                panic!("protocol response exceeded five seconds for {command_type}/{request_id}")
            })
            .expect("server closed before protocol response")
            .unwrap();
        if let ClientMessage::Text(text) = frame {
            let response: serde_json::Value = serde_json::from_str(&text).unwrap();
            if response.get("request_id").and_then(|id| id.as_str()) == Some(request_id) {
                return response;
            }
        }
    }
}

async fn wait_for_chat_body(socket: &mut TestSocket, expected_body: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = socket
                .next()
                .await
                .expect("server closed before cross-replica chat event")
                .unwrap();
            if let ClientMessage::Text(text) = frame {
                let event: serde_json::Value = serde_json::from_str(&text).unwrap();
                if event["type"] == "chat"
                    && event["payload"]["event"]["type"] == "message_accepted"
                    && event["payload"]["event"]["message"]["body"] == expected_body
                {
                    return;
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("cross-replica message {expected_body:?} was not delivered"));
}

async fn mcp_tool(
    state: &AppState,
    headers: &HeaderMap,
    id: &str,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let response = mcp_handler(
        State(state.clone()),
        headers.clone(),
        Json(McpRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::json!(id),
            method: "tools/call".to_owned(),
            params: serde_json::json!({"name":name,"arguments":arguments}),
        }),
    )
    .await;
    let body = axum::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .unwrap();
    let response: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(response.get("result").is_some(), "{response}");
    response["result"]["structuredContent"].clone()
}

#[tokio::test]
async fn websocket_and_mcp_adapters_have_identical_chat_outcomes() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server, state) =
        start_test_server_with_state(repository, Duration::from_secs(60)).await;
    let mut browser = connect_as(address, "adapter-owner").await;
    let created = command(
            &mut browser,
            "adapter-create",
            "create_channel",
            serde_json::json!({"slug":"adapter-contract","name":"Adapter contract","kind":"private","circle_id":null}),
        )
        .await;
    let channel_id = created["payload"]["channel"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let owner = state
        .auth
        .authenticate_development(Some("adapter-owner".to_owned()))
        .unwrap()
        .user
        .id;
    let agent = state
        .agents
        .create(CreateAgent {
            actor: owner.clone(),
            owner_id: owner.clone(),
            display_name: "Adapter agent".to_owned(),
            provider: "contract".to_owned(),
            service_identity: "adapter-agent".to_owned(),
            purpose: "Adapter conformance".to_owned(),
            rate_limit_per_minute: 60,
            expires_at: None,
        })
        .await
        .unwrap();
    for scope in [AgentScope::ReadHistory, AgentScope::SendMessages] {
        state
            .agents
            .grant(GrantAgent {
                actor: owner.clone(),
                agent_id: agent.agent_id.clone(),
                circle_id: None,
                channel_id: Some(ChannelId::new(channel_id.clone()).unwrap()),
                scope,
                expires_at: None,
            })
            .await
            .unwrap();
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", agent.credential)).unwrap(),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(
        HeaderName::from_static("mcp-protocol-version"),
        HeaderValue::from_static(MCP_PROTOCOL_VERSION),
    );

    let browser_channels = command(
        &mut browser,
        "browser-list",
        "list_my_channels",
        serde_json::Value::Null,
    )
    .await;
    let agent_channels = mcp_tool(
        &state,
        &headers,
        "agent-list",
        "list_channels",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(browser_channels["payload"]["channels"][0]["id"], channel_id);
    assert_eq!(agent_channels[0]["id"], channel_id);

    let browser_message = command(
        &mut browser,
        "browser-send",
        "send_message",
        serde_json::json!({"channel_id":channel_id,"body":"from browser"}),
    )
    .await;
    let browser_replay = command(
        &mut browser,
        "browser-send",
        "send_message",
        serde_json::json!({"channel_id":channel_id,"body":"from browser"}),
    )
    .await;
    assert_eq!(
        browser_message["payload"]["message"]["id"],
        browser_replay["payload"]["message"]["id"]
    );
    let agent_message = mcp_tool(
            &state,
            &headers,
            "agent-send",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":"from agent","request_id":"agent-domain-send"}),
        )
        .await;
    let agent_replay = mcp_tool(
            &state,
            &headers,
            "agent-send-replay",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":"from agent","request_id":"agent-domain-send"}),
        )
        .await;
    assert_eq!(
        agent_message["message"]["id"],
        agent_replay["message"]["id"]
    );

    let browser_history = command(
        &mut browser,
        "browser-read",
        "load_recent_messages",
        serde_json::json!({"channel_id":channel_id,"limit":50,"after":0}),
    )
    .await;
    let agent_history = mcp_tool(
        &state,
        &headers,
        "agent-read",
        "read_messages",
        serde_json::json!({"channel_id":channel_id,"limit":50,"after_sequence":0}),
    )
    .await;
    assert_eq!(browser_history["payload"]["messages"], agent_history);
    assert_eq!(agent_history.as_array().unwrap().len(), 2);

    let browser_read = command(
        &mut browser,
        "browser-mark-read",
        "mark_read",
        serde_json::json!({"channel_id":channel_id,"sequence":2}),
    )
    .await;
    let agent_read = mcp_tool(
        &state,
        &headers,
        "agent-mark-read",
        "mark_read",
        serde_json::json!({"channel_id":channel_id,"sequence":2}),
    )
    .await;
    assert_eq!(
        browser_read["payload"]["membership"]["last_read_sequence"],
        agent_read["last_read_sequence"]
    );
    assert_eq!(agent_read["last_read_sequence"], 2);

    browser.close(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn browser_process_pilot_exposes_durable_status_and_idempotent_inspect() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server) = start_test_server(repository.clone(), Duration::from_secs(60)).await;
    let mut owner = connect_as(address, "process-browser-owner").await;
    let circle = command(
        &mut owner,
        "process-circle",
        "create_circle",
        serde_json::json!({"slug":"process-circle","name":"Process circle"}),
    )
    .await;
    let circle_id = circle["payload"]["circle"]["id"].as_str().unwrap();
    let channel = command(
            &mut owner,
            "process-channel",
            "create_channel",
            serde_json::json!({"slug":"process-channel","name":"Process channel","kind":"private","circle_id":circle_id}),
        )
        .await;
    let channel_id = channel["payload"]["channel"]["id"].as_str().unwrap();
    let client = reqwest::Client::new();
    let base = format!("http://{address}");
    let feature = client
            .post(format!("{base}/api/v1/circles/{circle_id}/features/heart-event-planning?participant=process-browser-owner"))
            .json(&serde_json::json!({"enabled":true}))
            .send()
            .await
            .unwrap();
    assert_eq!(feature.status(), reqwest::StatusCode::NO_CONTENT);
    let started = client
        .post(format!(
            "{base}/api/v1/processes?participant=process-browser-owner"
        ))
        .json(&serde_json::json!({
            "channel_id":channel_id,
            "request_id":"browser-process-start",
            "namespace":"sproyt",
            "definition_name":"event-planning",
            "definition_version":"1",
            "metadata":{"title":"Dinner"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(started.status(), reqwest::StatusCode::ACCEPTED);
    let started: serde_json::Value = started.json().await.unwrap();
    let process_id = started["process_link_id"].as_str().unwrap();
    let job = repository
        .lease_next(Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    repository
        .complete_start(
            job,
            StartedProcess {
                instance_id: uuid::Uuid::now_v7(),
            },
        )
        .await
        .unwrap();

    let view = client
        .get(format!(
            "{base}/api/v1/processes/{process_id}?participant=process-browser-owner"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(view.status(), reqwest::StatusCode::OK);
    let view: serde_json::Value = view.json().await.unwrap();
    assert_eq!(view["process"]["status"], "active");
    assert_eq!(view["events"][0]["event_type"], "process.started");

    let inspect_url =
        format!("{base}/api/v1/processes/{process_id}/inspect?participant=process-browser-owner");
    let inspect = client
        .post(&inspect_url)
        .json(&serde_json::json!({"request_id":"browser-inspect"}))
        .send()
        .await
        .unwrap();
    assert_eq!(inspect.status(), reqwest::StatusCode::ACCEPTED);
    let inspect: serde_json::Value = inspect.json().await.unwrap();
    let replay = client
        .post(inspect_url)
        .json(&serde_json::json!({"request_id":"browser-inspect"}))
        .send()
        .await
        .unwrap();
    let replay: serde_json::Value = replay.json().await.unwrap();
    assert_eq!(inspect["outbox_id"], replay["outbox_id"]);

    let denied = client
        .get(format!(
            "{base}/api/v1/processes/{process_id}?participant=process-outsider"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);
    owner.close(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn heart_unavailable_does_not_interrupt_chat_and_recovers_once() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (gateway, heart) = recoverable_heart_gateway().await;
    let (address, server) = start_test_server_with_gateway(repository, gateway).await;
    let mut owner = connect_as(address, "heart-isolation-owner").await;
    let circle = command(
        &mut owner,
        "isolation-circle",
        "create_circle",
        serde_json::json!({"slug":"heart-isolation","name":"Heart isolation"}),
    )
    .await;
    let circle_id = circle["payload"]["circle"]["id"].as_str().unwrap();
    let channel = command(
            &mut owner,
            "isolation-channel",
            "create_channel",
            serde_json::json!({"slug":"heart-isolation-chat","name":"Heart isolation chat","kind":"private","circle_id":circle_id}),
        )
        .await;
    let channel_id = channel["payload"]["channel"]["id"].as_str().unwrap();
    let client = reqwest::Client::new();
    let base = format!("http://{address}");
    let feature = client
            .post(format!(
                "{base}/api/v1/circles/{circle_id}/features/heart-event-planning?participant=heart-isolation-owner"
            ))
            .json(&serde_json::json!({"enabled":true}))
            .send()
            .await
            .unwrap();
    assert_eq!(feature.status(), reqwest::StatusCode::NO_CONTENT);
    let started = client
        .post(format!(
            "{base}/api/v1/processes?participant=heart-isolation-owner"
        ))
        .json(&serde_json::json!({
            "channel_id":channel_id,
            "request_id":"heart-isolation-start",
            "namespace":"sproyt",
            "definition_name":"event-planning",
            "definition_version":"1",
            "metadata":{"title":"Resilient dinner"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(started.status(), reqwest::StatusCode::ACCEPTED);
    let process_id = started.json::<serde_json::Value>().await.unwrap()["process_link_id"]
        .as_str()
        .unwrap()
        .to_owned();

    tokio::time::timeout(Duration::from_secs(3), async {
        while heart.starts.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("outbox did not attempt Heart while it was unavailable");

    let chat_body = "ordinary chat remains available without Heart";
    command(
        &mut owner,
        "chat-during-heart-outage",
        "send_message",
        serde_json::json!({"channel_id":channel_id,"body":chat_body}),
    )
    .await;
    let loaded = command(
        &mut owner,
        "chat-during-heart-outage-read",
        "load_recent_messages",
        serde_json::json!({"channel_id":channel_id,"limit":20,"after":0}),
    )
    .await;
    assert!(
        loaded["payload"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["body"] == chat_body),
        "ordinary chat must persist and read while Heart is unavailable"
    );

    heart.available.store(true, Ordering::SeqCst);
    let recovered_view = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let response = client
                .get(format!(
                    "{base}/api/v1/processes/{process_id}?participant=heart-isolation-owner"
                ))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            let view = response.json::<serde_json::Value>().await.unwrap();
            if view["process"]["status"] == "active" {
                break view;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("queued process did not recover after Heart returned");
    assert!(heart.starts.load(Ordering::SeqCst) >= 2);
    assert_eq!(
        recovered_view["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["event_type"] == "process.started")
            .count(),
        1,
        "Heart recovery must complete the durable start exactly once"
    );

    owner.close(None).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn websocket_capacity_reconnect_and_service_restart_gate() {
    const MESSAGE_COUNT: usize = 40;
    const CURSOR: u64 = 20;
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, first_server) =
        start_test_server(repository.clone(), Duration::from_secs(60)).await;
    let mut socket = connect(address).await;
    let created = command(
            &mut socket,
            "create",
            "create_channel",
            serde_json::json!({"slug":"capacity-gate","name":"Capacity gate","kind":"private","circle_id":null}),
        )
        .await;
    let channel_id = created["payload"]["channel"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let mut latencies = Vec::with_capacity(MESSAGE_COUNT);
    let mut first_message_id = None;
    for index in 1..=MESSAGE_COUNT {
        let started = Instant::now();
        let accepted = command(
            &mut socket,
            &format!("send-{index}"),
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":format!("capacity-{index}")}),
        )
        .await;
        latencies.push(started.elapsed());
        assert_eq!(
            accepted["payload"]["message"]["sequence"].as_u64(),
            Some(index as u64)
        );
        if index == 1 {
            first_message_id = accepted["payload"]["message"]["id"]
                .as_str()
                .map(str::to_owned);
        }
    }
    let mismatch = command_response(
        &mut socket,
        "send-1",
        "send_message",
        serde_json::json!({"channel_id":channel_id,"body":"must not replace the accepted body"}),
    )
    .await;
    assert_eq!(mismatch["type"], "error");
    assert_eq!(mismatch["payload"]["code"], "conflict");
    let replay = command(
        &mut socket,
        "send-1",
        "send_message",
        serde_json::json!({"channel_id":channel_id,"body":"capacity-1"}),
    )
    .await;
    assert_eq!(
        replay["payload"]["message"]["id"].as_str(),
        first_message_id.as_deref()
    );
    assert_eq!(replay["payload"]["message"]["sequence"].as_u64(), Some(1));
    assert_eq!(
        replay["payload"]["message"]["body"].as_str(),
        Some("capacity-1")
    );
    latencies.sort_unstable();
    let p99_index = ((MESSAGE_COUNT * 99).div_ceil(100)).saturating_sub(1);
    assert!(
        latencies[p99_index] < Duration::from_millis(750),
        "p99 send latency was {:?}",
        latencies[p99_index]
    );

    socket.close(None).await.unwrap();
    first_server.abort();
    let _ = first_server.await;

    let (restart_address, restarted_server) =
        start_test_server(repository, Duration::from_secs(60)).await;
    let reconnect_started = Instant::now();
    let mut reconnected = connect(restart_address).await;
    let loaded = command(
        &mut reconnected,
        "catch-up",
        "load_recent_messages",
        serde_json::json!({"channel_id":channel_id,"limit":MESSAGE_COUNT,"after":CURSOR}),
    )
    .await;
    assert!(reconnect_started.elapsed() < Duration::from_secs(5));
    let messages = loaded["payload"]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), MESSAGE_COUNT - CURSOR as usize);
    assert_eq!(
        messages.first().unwrap()["sequence"].as_u64(),
        Some(CURSOR + 1)
    );
    assert_eq!(
        messages.last().unwrap()["sequence"].as_u64(),
        Some(MESSAGE_COUNT as u64)
    );

    reconnected.close(None).await.unwrap();
    restarted_server.abort();
}

#[tokio::test]
async fn postgres_two_replica_realtime_and_restart_catch_up_gate() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let suffix = uuid::Uuid::now_v7().simple().to_string();
    let alice_name = format!("replica-alice-{suffix}");
    let bob_name = format!("replica-bob-{suffix}");
    let first_repository = Arc::new(PostgresChatRepository::connect(&url).await.unwrap());
    first_repository.migrate().await.unwrap();
    let second_repository = Arc::new(PostgresChatRepository::connect(&url).await.unwrap());
    let (first_address, first_server) =
        start_postgres_test_server(first_repository, Duration::from_secs(60)).await;
    let (second_address, second_server) =
        start_postgres_test_server(second_repository, Duration::from_secs(60)).await;
    let mut alice = connect_as(first_address, &alice_name).await;
    let mut bob = connect_as(second_address, &bob_name).await;

    let alice_channels = command(
        &mut alice,
        "alice-channels",
        "list_my_channels",
        serde_json::Value::Null,
    )
    .await;
    let channel_id = alice_channels["payload"]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|channel| channel["slug"] == "general")
        .expect("global general channel must be available to every authenticated user")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    command(
        &mut alice,
        "alice-subscribe",
        "subscribe_channel",
        serde_json::json!({"channel_id":channel_id}),
    )
    .await;
    command(
        &mut bob,
        "bob-subscribe",
        "subscribe_channel",
        serde_json::json!({"channel_id":channel_id}),
    )
    .await;

    let first_body = format!("cross-replica-a-{suffix}");
    let accepted = command(
        &mut alice,
        "alice-send",
        "send_message",
        serde_json::json!({"channel_id":channel_id,"body":first_body}),
    )
    .await;
    let first_sequence = accepted["payload"]["message"]["sequence"].as_u64().unwrap();
    wait_for_chat_body(&mut bob, &first_body).await;

    alice.close(None).await.unwrap();
    first_server.abort();
    let missed_body = format!("restart-catch-up-{suffix}");
    command(
        &mut bob,
        "bob-send",
        "send_message",
        serde_json::json!({"channel_id":channel_id,"body":missed_body}),
    )
    .await;

    let replacement_repository = Arc::new(PostgresChatRepository::connect(&url).await.unwrap());
    let (replacement_address, replacement_server) =
        start_postgres_test_server(replacement_repository, Duration::from_secs(60)).await;
    let reconnect_started = Instant::now();
    let mut reconnected = connect_as(replacement_address, &alice_name).await;
    let loaded = command(
        &mut reconnected,
        "alice-catch-up",
        "load_recent_messages",
        serde_json::json!({"channel_id":channel_id,"limit":20,"after":first_sequence}),
    )
    .await;
    assert!(reconnect_started.elapsed() < Duration::from_secs(5));
    assert!(
        loaded["payload"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["body"] == missed_body),
        "a restarted replica must catch up messages accepted while it was unavailable"
    );

    reconnected.close(None).await.unwrap();
    bob.close(None).await.unwrap();
    second_server.abort();
    replacement_server.abort();
}

#[tokio::test]
async fn idle_websocket_is_closed_with_a_stable_reason() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server) = start_test_server(repository, Duration::from_millis(100)).await;
    let mut socket = connect(address).await;

    let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("idle close frame exceeded test deadline")
        .expect("server closed without a WebSocket close frame")
        .unwrap();
    match frame {
        ClientMessage::Close(Some(frame)) => {
            assert_eq!(frame.reason, "idle timeout");
        }
        other => panic!("expected idle close frame, received {other:?}"),
    }
    server.abort();
}

#[tokio::test]
async fn websocket_reports_authorization_and_unknown_command_errors() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;
    let mut owner = connect_as(address, "wire-owner").await;
    let created = command(
            &mut owner,
            "create-private",
            "create_channel",
            serde_json::json!({"slug":"wire-private","name":"Wire private","kind":"private","circle_id":null}),
        )
        .await;
    let channel_id = created["payload"]["channel"]["id"].clone();

    let mut outsider = connect_as(address, "wire-outsider").await;
    let denied = command_response(
        &mut outsider,
        "unauthorized-load",
        "load_recent_messages",
        serde_json::json!({"channel_id":channel_id,"limit":50,"after":0}),
    )
    .await;
    assert_eq!(denied["type"], "error");
    assert_eq!(denied["payload"]["code"], "permission_denied");

    outsider
            .send(ClientMessage::Text(
                serde_json::json!({"protocol":"sproyt.chat.v1","request_id":"unknown","type":"future_command"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
    let unknown = tokio::time::timeout(Duration::from_secs(2), outsider.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let ClientMessage::Text(unknown) = unknown else {
        panic!("expected structured unknown-command error")
    };
    let unknown: serde_json::Value = serde_json::from_str(&unknown).unwrap();
    assert_eq!(unknown["type"], "error");
    assert_eq!(unknown["payload"]["code"], "invalid_envelope");

    outsider
            .send(ClientMessage::Text(
                serde_json::json!({"protocol":"sproyt.chat.v1","request_id":"future-field","type":"ping","future_extension":{"version":2}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
    loop {
        let frame = outsider.next().await.unwrap().unwrap();
        if let ClientMessage::Text(text) = frame {
            let response: serde_json::Value = serde_json::from_str(&text).unwrap();
            if response["request_id"] == "future-field" {
                assert_eq!(response["type"], "pong");
                break;
            }
        }
    }
    server.abort();
}

#[tokio::test]
async fn two_users_complete_private_circle_slice_with_unread_reconnect() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;
    let mut owner = connect_as(address, "circle-owner").await;
    let circle = command(
        &mut owner,
        "circle-create",
        "create_circle",
        serde_json::json!({"slug":"friends","name":"Friends"}),
    )
    .await;
    let circle_id = circle["payload"]["circle"]["id"].clone();
    let invitation = command(
        &mut owner,
        "circle-invite",
        "create_circle_invitation",
        serde_json::json!({"circle_id":circle_id}),
    )
    .await;
    let token = invitation["payload"]["invitation"]["token"].clone();
    let channel = command(
            &mut owner,
            "circle-channel",
            "create_channel",
            serde_json::json!({"slug":"friends-chat","name":"Friends chat","kind":"private","circle_id":circle_id}),
        )
        .await;
    let channel_id = channel["payload"]["channel"]["id"].clone();

    let mut member = connect_as(address, "circle-member").await;
    let member_hello = command(
        &mut member,
        "member-hello",
        "hello",
        serde_json::Value::Null,
    )
    .await;
    let member_id = member_hello["payload"]["participant_id"].clone();
    let denied = command_response(
        &mut member,
        "join-before-invite",
        "join_channel",
        serde_json::json!({"channel":{"type":"id","value":channel_id}}),
    )
    .await;
    assert_eq!(denied["payload"]["code"], "permission_denied");
    command(
        &mut member,
        "accept-invite",
        "accept_circle_invitation",
        serde_json::json!({"token":token}),
    )
    .await;
    let not_inherited = command(
        &mut member,
        "channels-after-invite",
        "list_my_channels",
        serde_json::Value::Null,
    )
    .await;
    assert!(
        !not_inherited["payload"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|channel| channel["id"] == channel_id),
        "private channels must not be inherited with circle membership"
    );
    let denied = command_response(
        &mut member,
        "private-self-join",
        "join_channel",
        serde_json::json!({"channel":{"type":"id","value":channel_id}}),
    )
    .await;
    assert_eq!(denied["payload"]["code"], "permission_denied");
    command(
        &mut owner,
        "invite-private-member",
        "add_channel_member",
        serde_json::json!({"channel_id":channel_id,"user_id":member_id}),
    )
    .await;

    for sequence in 1..=2 {
        command(
                &mut owner,
                &format!("circle-send-{sequence}"),
                "send_message",
                serde_json::json!({"channel_id":channel_id,"body":format!("friend-message-{sequence}")}),
            )
            .await;
    }
    member.close(None).await.unwrap();
    let mut member = connect_as(address, "circle-member").await;
    let listed = command(
        &mut member,
        "list-unread",
        "list_my_channels",
        serde_json::Value::Null,
    )
    .await;
    let summary = listed["payload"]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|summary| summary["id"] == channel_id)
        .unwrap();
    assert_eq!(summary["last_read_sequence"].as_u64(), Some(0));
    assert_eq!(summary["latest_sequence"].as_u64(), Some(2));
    let loaded = command(
        &mut member,
        "load-unread",
        "load_recent_messages",
        serde_json::json!({"channel_id":channel_id,"limit":50,"after":0}),
    )
    .await;
    let loaded_messages = loaded["payload"]["messages"].as_array().unwrap();
    assert_eq!(loaded_messages.len(), 2);
    assert!(
        loaded_messages
            .iter()
            .all(|message| message["sender_display_name"] == "circle-owner")
    );
    let older_page = command(
        &mut member,
        "load-older-page",
        "load_recent_messages",
        serde_json::json!({"channel_id":channel_id,"limit":50,"before":2}),
    )
    .await;
    let older_messages = older_page["payload"]["messages"].as_array().unwrap();
    assert_eq!(older_messages.len(), 1);
    assert_eq!(older_messages[0]["sequence"].as_u64(), Some(1));
    command(
        &mut member,
        "mark-all-read",
        "mark_read",
        serde_json::json!({"channel_id":channel_id,"sequence":2}),
    )
    .await;
    member.close(None).await.unwrap();
    let mut member = connect_as(address, "circle-member").await;
    let listed = command(
        &mut member,
        "list-read",
        "list_my_channels",
        serde_json::Value::Null,
    )
    .await;
    let summary = listed["payload"]["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|summary| summary["id"] == channel_id)
        .unwrap();
    assert_eq!(summary["last_read_sequence"].as_u64(), Some(2));
    assert_eq!(summary["latest_sequence"].as_u64(), Some(2));

    let denied = command_response(
        &mut member,
        "member-delete-circle",
        "delete_circle",
        serde_json::json!({"circle_id":circle_id}),
    )
    .await;
    assert_eq!(denied["payload"]["code"], "permission_denied");
    let deleted = command(
        &mut owner,
        "owner-delete-circle",
        "delete_circle",
        serde_json::json!({"circle_id":circle_id}),
    )
    .await;
    assert_eq!(deleted["type"], "circle_deleted");

    let channels = command(
        &mut member,
        "list-after-delete",
        "list_my_channels",
        serde_json::Value::Null,
    )
    .await;
    assert!(
        channels["payload"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .all(|summary| summary["id"] != channel_id)
    );
    let circles = command(
        &mut member,
        "list-circles-after-delete",
        "list_my_circles",
        serde_json::Value::Null,
    )
    .await;
    assert!(
        circles["payload"]["circles"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry[0]["id"] != circle_id)
    );
    server.abort();
}

#[tokio::test]
async fn leaving_circle_disconnects_inaccessible_websocket_channels() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;
    let mut owner = connect_as(address, "leave-circle-owner").await;
    let circle = command(
        &mut owner,
        "leave-circle-create",
        "create_circle",
        serde_json::json!({"slug":"leave-circle","name":"Leave circle"}),
    )
    .await;
    let circle_id = circle["payload"]["circle"]["id"].clone();
    let channel = command(
            &mut owner,
            "leave-circle-channel",
            "create_channel",
            serde_json::json!({"slug":"leave-circle-chat","name":"Leave circle chat","kind":"local","circle_id":circle_id}),
        )
        .await;
    let channel_id = channel["payload"]["channel"]["id"].clone();
    let invitation = command(
        &mut owner,
        "leave-circle-invite",
        "create_circle_invitation",
        serde_json::json!({"circle_id":circle_id}),
    )
    .await;

    let mut member = connect_as(address, "leave-circle-member").await;
    let member_id = command(
        &mut member,
        "leave-circle-member-hello",
        "hello",
        serde_json::Value::Null,
    )
    .await["payload"]["participant_id"]
        .clone();
    command(
        &mut member,
        "leave-circle-accept",
        "accept_circle_invitation",
        serde_json::json!({"token":invitation["payload"]["invitation"]["token"]}),
    )
    .await;
    command(
        &mut member,
        "leave-circle-join-channel",
        "join_channel",
        serde_json::json!({"channel":{"type":"id","value":channel_id}}),
    )
    .await;
    command(
        &mut owner,
        "leave-circle-owner-subscribe",
        "subscribe_channel",
        serde_json::json!({"channel_id":channel_id}),
    )
    .await;
    command(
        &mut member,
        "leave-circle-member-subscribe",
        "subscribe_channel",
        serde_json::json!({"channel_id":channel_id}),
    )
    .await;

    command(
        &mut member,
        "leave-circle",
        "leave_circle",
        serde_json::json!({"circle_id":circle_id}),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = owner.next().await.unwrap().unwrap();
            if let ClientMessage::Text(text) = frame {
                let event: serde_json::Value = serde_json::from_str(&text).unwrap();
                if event["type"] == "chat"
                    && event["payload"]["event"]["type"] == "participant_left"
                    && event["payload"]["event"]["participant_id"] == member_id
                {
                    return;
                }
            }
        }
    })
    .await
    .expect("owner did not observe the departed member leaving presence");

    command(
        &mut owner,
        "leave-circle-send-after-leave",
        "send_message",
        serde_json::json!({"channel_id":channel_id,"body":"must not reach departed member"}),
    )
    .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(350), member.next())
            .await
            .is_err(),
        "departed member received a websocket event after leaving the circle"
    );
    server.abort();
}
