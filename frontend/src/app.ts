      import { createApplicationStore, createServerEventMailbox } from "./client-store";
      import { AgentApi, HttpClient, NotificationApi, ProcessApi, type CreatedAgent, type ProcessView } from "./api";
      import { requireElement, requireElements } from "./dom";
      import { createConnectionController, resetTransientRequestsAfterDisconnect, shouldForceResume } from "./connection";
      import { NavigationController } from "./navigation";
      import { createSessionController, fetchWithTimeout, sessionRefreshAfterSeconds, type SessionController } from "./session";
      import { isJsonObject, isRecord, mediaFromUpload } from "./types";
      import type { Channel, ChatMessage, Circle, ClientCommand, ClientCommandArguments, JsonObject, MediaObject, Mention, MermaidApi, ThreadComposerState, ThreadSummary, UploadResponse, UserProfile, UserTask, WireEvent } from "./types";

      type TimelineItem =
        | Readonly<{ type: "message"; message: ChatMessage }>
        | Readonly<{ type: "system"; text: string }>;
      type Invitation = Readonly<{ response?: "accepted" | "declined" | null; invited_by: string; invited_by_name: string; channel_name?: string | null; circle_name?: string | null; accepted_count: number; declined_count: number }>;
      type InvitationCache =
        | Readonly<{ status: "pending" }>
        | Readonly<{ status: "missing" | "failed"; message: string }>
        | Readonly<{ status: "resolved"; invitation: Invitation }>;
      type PendingInvitationResponse = Readonly<{ token: string; command: "accept_invitation" | "decline_invitation" }>;
      type PendingMessage = Readonly<{ channelId: string; body: string; draft: string; mediaIds: string[] }>;
      type MessageInteraction = Readonly<{ messageId: string; customReaction: string; focusCustomReaction: boolean; focusReactionSummary: boolean }>;
      function isMermaidApi(value: unknown): value is MermaidApi {
        return isRecord(value) && typeof value.initialize === "function" && typeof value.run === "function";
      }
      function channelFromBase(channel: Readonly<{ id: string; slug: string; name: string; kind: Channel["kind"]; circle_id: string | null }>, role: Channel["role"], description = "", directUserId: string | null = null): Channel {
        return { ...channel, role, description, direct_user_id: directUserId, latest_sequence: 0, last_read_sequence: 0 };
      }
      function errorMessage(error: unknown): string {
        return error instanceof Error ? error.message : "ukjend feil";
      }

      function syncAppViewportHeight() {
        const viewport = window.visualViewport;
        const height = viewport?.height || window.innerHeight;
        const offsetTop = viewport?.offsetTop || 0;
        document.documentElement.style.setProperty("--app-height", `${Math.round(height)}px`);
        document.documentElement.style.setProperty("--app-offset-top", `${Math.round(offsetTop)}px`);
      }
      syncAppViewportHeight();
      window.addEventListener("resize", syncAppViewportHeight, { passive: true });
      window.visualViewport?.addEventListener("resize", syncAppViewportHeight, { passive: true });
      window.visualViewport?.addEventListener("scroll", syncAppViewportHeight, { passive: true });

      const serviceWorkerReady = "serviceWorker" in navigator
        ? navigator.serviceWorker.register("/service-worker.js", { scope: "/" }).then(() => navigator.serviceWorker.ready)
        : Promise.resolve(null);
      const connectForm = requireElement("#connect-form", HTMLFormElement);
      const sendForm = requireElement("#send-form", HTMLFormElement);
      const composerTools = requireElement("#composer-tools", HTMLElement);
      const channelInput = requireElement("#channel", HTMLInputElement);
      const bodyInput = requireElement("#body", HTMLTextAreaElement);
      const mentionSuggestions = requireElement("#mention-suggestions", HTMLElement);
      const sendButton = requireElement("#send", HTMLButtonElement);
      const attachMediaButton = requireElement("#attach-media", HTMLButtonElement);
      const messageEmojiPicker = requireElement(".emoji-picker", HTMLDetailsElement);
      const statusText = requireElement("#status-text", HTMLInputElement);
      const statusEmoji = requireElement("#status-emoji", HTMLInputElement);
      const currentStatus = requireElement("#current-status", HTMLElement);
      const signupBadge = requireElement("#signup-badge", HTMLElement);
      const notificationSummary = requireElement("#notification-summary", HTMLElement);
      const notificationMode = requireElement("#notification-mode", HTMLSelectElement);
      const notificationDirect = requireElement("#notification-direct", HTMLInputElement);
      const notificationMentions = requireElement("#notification-mentions", HTMLInputElement);
      const notificationNotice = requireElement("#notification-notice", HTMLElement);
      const enableNotifications = requireElement("#enable-notifications", HTMLButtonElement);
      const mediaInput = requireElement("#media-input", HTMLInputElement);
      const mediaPreviews = requireElement("#media-previews", HTMLElement);
      const uploadStatus = requireElement("#upload-status", HTMLElement);
      const mediaLightbox = requireElement("#media-lightbox", HTMLDialogElement);
      const mediaLightboxImage = requireElement("#media-lightbox-image", HTMLImageElement);
      const mediaLightboxCaption = requireElement("#media-lightbox-caption", HTMLElement);
      const threadPanel = requireElement("#thread-panel", HTMLDialogElement);
      const threadMessages = requireElement("#thread-messages", HTMLElement);
      const threadForm = requireElement("#thread-form", HTMLFormElement);
      const threadBody = requireElement("#thread-body", HTMLTextAreaElement);
      const threadEmojiPicker = requireElement("#thread-emoji-picker", HTMLDetailsElement);
      const threadComposerTools = requireElement("#thread-composer-tools", HTMLElement);
      const threadAttachMediaButton = requireElement("#thread-attach-media", HTMLButtonElement);
      const threadMediaInput = requireElement("#thread-media-input", HTMLInputElement);
      const threadMediaPreviews = requireElement("#thread-media-previews", HTMLElement);
      const threadUploadStatus = requireElement("#thread-upload-status", HTMLElement);
      const threadSendButton = requireElement("#thread-send", HTMLButtonElement);
      const circleChannelDialog = requireElement("#circle-channel-dialog", HTMLDialogElement);
      const circleChannelTitle = requireElement("#circle-channel-title", HTMLElement);
      const circleChannelClose = requireElement("#circle-channel-close", HTMLButtonElement);
      const circleJoinableList = requireElement("#circle-joinable-list", HTMLElement);
      const circleChannelCreate = requireElement("#circle-channel-create", HTMLFormElement);
      const managedChannelName = requireElement("#managed-channel-name", HTMLInputElement);
      const managedChannelKind = requireElement("#managed-channel-kind", HTMLSelectElement);
      const leaveCircleButton = requireElement("#leave-circle", HTMLButtonElement);
      const circleMembershipNotice = requireElement("#circle-membership-notice", HTMLElement);
      const viewModeToggle = requireElement("#view-mode-toggle", HTMLButtonElement);
      const statusEl = requireElement("#status", HTMLElement);
      const connectionStatusText = requireElement("#connection-status-text", HTMLElement);
      const reauthenticateNowButton = requireElement("#reauthenticate-now", HTMLButtonElement);
      const connectionStatusToggle = requireElement("#connection-status-toggle", HTMLButtonElement);
      const connectionStatusDot = requireElement("#connection-status-dot", HTMLElement);
      const messagesEl = requireElement("#messages", HTMLElement);
      const bottomChannelPanel = requireElement("#bottom-channel-panel", HTMLDetailsElement);
      const bottomCirclePanel = requireElement("#bottom-circle-panel", HTMLDetailsElement);
      const bottomChannelToggle = requireElement("#bottom-channel-toggle", HTMLElement);
      const bottomCircleToggle = requireElement("#bottom-circle-toggle", HTMLElement);
      const bottomNavigation = requireElement(".bottom-navigation", HTMLElement);
      const bottomChannelList = requireElement("#bottom-channel-list", HTMLElement);
      const bottomCircleList = requireElement("#bottom-circle-list", HTMLElement);
      const bottomCircleContent = requireElement("#bottom-circle-content", HTMLElement);
      const circleToolDirect = requireElement("#circle-tool-direct", HTMLButtonElement);
      const circleToolShared = requireElement("#circle-tool-shared", HTMLButtonElement);
      const circleToolSettings = requireElement("#circle-tool-settings", HTMLButtonElement);
      const circleAdminDialog = requireElement("#circle-admin-dialog", HTMLDialogElement);
      const circleAdminClose = requireElement("#circle-admin-close", HTMLButtonElement);
      const directMessageDialog = requireElement("#direct-message-dialog", HTMLDialogElement);
      const directUser = requireElement("#direct-user", HTMLSelectElement);
      const directMessageStatus = requireElement("#direct-message-status", HTMLElement);
      const openDirect = requireElement("#open-direct", HTMLButtonElement);
      const conversationTitle = requireElement("#conversation-title", HTMLElement);
      const conversationCircle = requireElement("#conversation-circle", HTMLElement);
      const conversationContext = requireElement("#conversation-context", HTMLElement);
      const conversationPeerStatus = requireElement("#conversation-peer-status", HTMLElement);
      const channelPeopleButton = requireElement("#channel-people", HTMLButtonElement);
      const channelDetailsDialog = requireElement("#channel-details-dialog", HTMLDialogElement);
      const channelDetailsClose = requireElement("#channel-details-close", HTMLButtonElement);
      const channelMemberSearch = requireElement("#channel-member-search", HTMLInputElement);
      const channelMemberCount = requireElement("#channel-member-count", HTMLElement);
      const channelMemberList = requireElement("#channel-member-list", HTMLElement);
      const channelDescriptionForm = requireElement("#channel-description-form", HTMLFormElement);
      const channelDescriptionInput = requireElement("#channel-description-input", HTMLTextAreaElement);
      const channelDescriptionStatus = requireElement("#channel-description-status", HTMLElement);
      const circleSelect = requireElement("#circle-select", HTMLSelectElement);
      const circleName = requireElement("#circle-name", HTMLInputElement);
      const circleSlug = requireElement("#circle-slug", HTMLInputElement);
      const channelMemberAdd = requireElement("#channel-member-add", HTMLElement);
      const channelMember = requireElement("#channel-member", HTMLSelectElement);
      const addChannelMember = requireElement("#add-channel-member", HTMLButtonElement);
      const inviteChannelMember = requireElement("#invite-channel-member", HTMLButtonElement);
      const channelMemberStatus = requireElement("#channel-member-status", HTMLElement);
      const invitationToken = requireElement("#invitation-token", HTMLInputElement);
      const copyInvitation = requireElement("#copy-invitation", HTMLButtonElement);
      const createAgentAccessButton = requireElement("#create-agent-access", HTMLButtonElement);
      const copyAgentCredentialButton = requireElement("#copy-agent-credential", HTMLButtonElement);
      const revokeAgentAccessButton = requireElement("#revoke-agent-access", HTMLButtonElement);
      const agentCredential = requireElement("#agent-credential", HTMLTextAreaElement);
      const agentAccessNotice = requireElement("#agent-access-notice", HTMLElement);
      const onboardingNotice = requireElement("#onboarding-notice", HTMLElement);
      const createCircleButton = requireElement("#create-circle", HTMLButtonElement);
      const createCircleInvitationButton = requireElement("#create-invitation", HTMLButtonElement);
      const acceptInvitationButton = requireElement("#accept-invitation", HTMLButtonElement);
      const deleteCircleButton = requireElement("#delete-circle", HTMLButtonElement);
      const circleButtons = [createCircleButton, createCircleInvitationButton, acceptInvitationButton, deleteCircleButton];
      const exportButton = requireElement("#export-data", HTMLButtonElement);
      const processTitle = requireElement("#process-title", HTMLInputElement);
      const processId = requireElement("#process-id", HTMLInputElement);
      const processView = requireElement("#process-view", HTMLElement);
      const processButtons = ["#enable-heart", "#start-process", "#refresh-process", "#inspect-process", "#process-yes", "#process-no"].map((id) => requireElement(id, HTMLButtonElement));
      const sidebar = requireElement("#sidebar-panel", HTMLElement);
      const appMain = requireElement("main", HTMLElement);
      const desktopSidebarToggle = requireElement("#desktop-sidebar-toggle", HTMLButtonElement);
      const desktopAdvancedEntry = requireElement("#desktop-advanced-entry", HTMLButtonElement);
      const statusEditor = requireElement("#status-editor", HTMLDetailsElement);
      const notificationEditor = requireElement("#notification-editor", HTMLDetailsElement);
      const currentStatusIcon = requireElement(".status-compact-icon", HTMLElement);
      const currentStatusLabel = requireElement(".status-summary-label", HTMLElement);
      const notificationSummaryLabel = requireElement(".notification-summary-label", HTMLElement);
      const mobileNavigationToggle = requireElement("#mobile-navigation-toggle", HTMLButtonElement);
      const composerArea = requireElement(".composer-area", HTMLElement);

      let sessionController: SessionController;
      const connectionSupervisor = createConnectionController({
        websocketUrl: () => {
          const protocol = window.location.protocol === "https:" ? "wss" : "ws";
          const url = new URL(`${protocol}://${window.location.host}/ws`);
          const participant = new URLSearchParams(window.location.search).get("participant");
          if (participant) url.searchParams.set("participant", participant);
          return url.toString();
        },
        createRequestId: () => { requestNumber += 1; return `${browserSessionId}-${requestNumber}`; },
        onCommandSent: (requestId, command) => {
          pendingCommands.set(requestId, command.type);
          if (command.type === "list_my_channels") latestChannelListRequestId = requestId;
          if (command.type === "list_my_circles") latestCircleListRequestId = requestId;
        },
        onBeforeConnect: () => {
          catchUpTargets.clear();
          if (activeChannelId && messagesEl.childElementCount > 0) reconnectScrollOffset = Math.max(0, messagesEl.scrollHeight - messagesEl.scrollTop - messagesEl.clientHeight);
          if (!activeChannelId) requestedChannelSlug = (channelInput.value.trim() || "").toLowerCase().replace(/[^a-z0-9_-]+/g, "-");
        },
        onOpen: (send) => {
          reportClientEvent("websocket_connected");
          if (connectionSupervisor.snapshot().handoffActive && activeChannelId) setConnectionStatus("Gjenopprettar samtalen …");
          send("hello"); send("list_users"); send("list_my_channels"); send("list_my_circles"); send("list_mentions"); send("list_tasks");
          if (activeChannelId) send("subscribe_channel", { channel_id: activeChannelId });
        },
        onEvent: (event) => serverEventMailbox.enqueue(event),
        onUnsupportedProtocol: () => pushSystem("Serveren svarte med ein ukjend protokoll."),
        onStatus: setConnected,
        onConnected: () => {},
        onDisconnected: () => reportClientEvent("websocket_disconnected"),
        onSocketError: () => reportClientEvent("websocket_error"),
        onConnectionLost: () => {
          for (const requestId of pendingMessages.keys()) failPendingMessage(requestId, "sambandet vart brote; kontroller samtalen før du prøver igjen");
          for (const requestId of [...pendingThreadReplies.keys()]) failPendingThreadReply(requestId, "sambandet vart brote; kontroller tråden før du prøver igjen");
          failPendingPeopleDirectRequests("Sambandet vart brote. Prøv igjen.");
          resetTransientRequestsAfterDisconnect({ historyRequestIds, pendingCommands, pendingInvitationResponses, pendingInvitationInspections, pendingChannelInvitationRecipients, pendingDirectInvitationMessages }, {
            setHistoryLoading: (loading) => { historyLoading = loading; },
            failInspection: (token) => {
              const message = "Invitasjonen kunne ikkje hentast fordi sambandet vart brote. Prøv igjen.";
              invitationInspectionCache.set(token, { status: "failed", message });
              showInvitationError(token, message);
            },
            failInvitationResponse: (token) => showInvitationError(token, "Sambandet vart brote før svaret vart stadfesta. Prøv igjen."),
            failChannelInvitation: () => { channelMemberStatus.textContent = "Sambandet vart brote. Prøv invitasjonen igjen."; }
          });
        },
        onRequestsLost: (requestIds) => {
          for (const requestId of requestIds) {
            if (pendingMessages.has(requestId)) failPendingMessage(requestId, "sambandet vart brote; kontroller samtalen før du prøver igjen");
            if (pendingThreadReplies.has(requestId)) failPendingThreadReply(requestId, "sambandet vart brote; kontroller tråden før du prøver igjen");
            historyRequestIds.delete(requestId);
            pendingCommands.delete(requestId);
            failPendingPeopleDirectRequest(requestId, "Sambandet vart brote. Prøv igjen.");
          }
        },
        onUncertainRequests: (requestIds) => {
          for (const requestId of requestIds) markDeliveryUncertain(requestId);
        },
        onAuthenticationFailure: () => sessionController.recoverAuthentication(),
        recover: recoverConnection,
        onHandoffFallback: () => sessionController.schedule(30),
        reportClientEvent: (event) => reportClientEvent(event),
        now: () => Date.now(),
        isVisible: () => document.visibilityState === "visible",
        setTimeout: (callback, milliseconds) => window.setTimeout(callback, milliseconds),
        clearTimeout: (timer) => window.clearTimeout(timer),
        setInterval: (callback, milliseconds) => window.setInterval(callback, milliseconds),
        clearInterval: (timer) => window.clearInterval(timer)
      });
      function sendCommand<Type extends ClientCommand["type"]>(type: Type, ...args: ClientCommandArguments<Type>): string | null {
        return connectionSupervisor.send(type, ...args);
      }
      function resendCommand<Type extends ClientCommand["type"]>(requestId: string, type: Type, ...args: ClientCommandArguments<Type>): string | null {
        return connectionSupervisor.resend(requestId, type, ...args);
      }
      let lastBackgroundRecoveryAt = 0;
      let hiddenSince: number | null = document.visibilityState === "hidden" ? Date.now() : null;
      let lastUserActivityAt = Date.now();
      let renderMode = "view";
      let requestNumber = 0;
      const browserSessionId = `browser-${crypto.randomUUID()}`;
      let activeChannelId: string | null = null;
      let activeCircleId: string | null = null;
      let activeRootScope: "shared" | "circle" | "direct" = "shared";
      let activeInboxKind: "unread" | "mentions" | "tasks" | null = null;
      let managedCircleId: string | null = null;
      let reconnectScrollOffset: number | null = null;
      const navigation = new NavigationController(window.localStorage, window.location);
      // This is a render cache only. NavigationController is the sole state and
      // persistence owner; UI code refreshes this snapshot after an intent.
      let restoredChannelId = navigation.restoredChannelId;
      let restoredCircleId = navigation.restoredCircleId;
      let currentParticipantId: string | null = null;
      let requestedChannelSlug = "general";
      const timeline: TimelineItem[] = [];
      const threadReplies = new Map<string, ChatMessage[]>();
      const threadRoots = new Map<string, ChatMessage>();
      const threadSummaries = new Map<string, ThreadSummary>();
      const pendingThreadReplies = new Map<string, Readonly<{ rootId: string; channelId: string; body: string; draft: string; mediaIds: string[] }>>();
      let activeThreadRootId: string | null = null;
      let pendingThreadToOpen: string | null = null;
      const seenMessageIds = new Set<string>();
      const catchUpTargets = new Map<string, number>();
      const pendingCommands = new Map<string, string>();
      const pendingInvitationResponses = new Map<string, PendingInvitationResponse>();
      const pendingInvitationInspections = new Map<string, string>();
      const pendingChannelInvitationRecipients = new Map<string, string>();
      const pendingDirectInvitationMessages = new Map<string, string>();
      // Requests from the member browser are independent: a slow DM open must
      // not block another person, the composer, or the rest of the dialog.
      const pendingPeopleDirectRequests = new Map<string, string>();
      const peopleDirectStatuses = new Map<string, string>();
      const invitationInspectionCache = new Map<string, InvitationCache>();
      let latestChannelListRequestId: string | null = null;
      let latestCircleListRequestId: string | null = null;
      const pendingMessages = new Map<string, PendingMessage>();
      const uncertainMessages = new Map<string, PendingMessage>();
      const uncertainThreadReplies = new Map<string, Readonly<{ rootId: string; channelId: string; body: string; draft: string; mediaIds: string[] }>>();
      const retriedUncertainRequests = new Set<string>();
      const historyRequestIds = new Set();
      const historyPageSize = 50;
      let historyHasMore = false;
      let historyLoading = false;
      let mermaidPromise: Promise<MermaidApi> | null = null;
      let knownChannels: Channel[] = [];
      let knownUsers: UserProfile[] = [];
      const knownCircleUsers = new Map<string, UserProfile[]>();
      const knownChannelUsers = new Map<string, UserProfile[]>();
      let knownMentions: Mention[] = [];
      let knownTasks: UserTask[] = [];
      const knownCircles = new Map<string, Circle>();
      let temporaryAgentId: string | null = null;
      let pendingMedia: MediaObject[] = [];
      const threadComposerStates = new Map<string, ThreadComposerState>();
      const messageReactions = new Map<string, Map<string, ReactionSummary>>();
      const reactionEmojis = [...document.querySelectorAll("#message-emoji-options [data-emoji]")]
        .filter((button): button is HTMLButtonElement => button instanceof HTMLButtonElement)
        .map((button) => button.dataset.emoji).filter((emoji): emoji is string => emoji !== undefined);
      let mentionMatches: UserProfile[] = [];
      let selectedMentionIndex = 0;
      let activeMention: Readonly<{ start: number; end: number }> | null = null;
      let composerHasFocus = false;
      let composerComposing = false;
      const statusDraft = { emoji: "", text: "", dirty: false };
      const usesDesktopComposerKeys = window.matchMedia("(any-hover: hover) and (any-pointer: fine)");

      function syncRenderedNavigation(): void {
        const snapshot = navigation.snapshot;
        activeChannelId = snapshot.activeChannelId;
        activeCircleId = snapshot.activeCircleId;
        activeRootScope = snapshot.activeRootScope;
        restoredChannelId = snapshot.restoredChannelId;
        restoredCircleId = snapshot.restoredCircleId;
      }

      const applicationStore = createApplicationStore();
      const sessionBroadcast = typeof BroadcastChannel === "function" ? new BroadcastChannel("sproyt-session-refresh-v1") : null;
      sessionController = createSessionController({
        fetch: window.fetch.bind(window), storage: window.localStorage, broadcast: sessionBroadcast,
        now: () => Date.now(), setTimeout: (callback, milliseconds) => window.setTimeout(callback, milliseconds), clearTimeout: (timer) => window.clearTimeout(timer),
        withLock: navigator.locks ? async (wait, operation) => navigator.locks.request("sproyt-session-refresh", wait ? {} : { ifAvailable: true }, async (lock) => lock ? operation() : "busy") : null,
        visibility: () => document.visibilityState, isConnectionOpen: () => connectionSupervisor.snapshot().connected,
        lastUserActivityAt: () => lastUserActivityAt, onRefreshDueAt: (refreshDueAt) => applicationStore.updateSession({ refreshDueAt }), onStatus: setConnectionStatus,
        onSessionRotated: () => connectionSupervisor.replaceAfterSessionRefresh(), onReconnectNeeded: (reason) => connectionSupervisor.scheduleReconnect(1006, reason),
        onLoginRequired: () => window.location.assign("/auth/login"), onReauthenticationRequired: (required) => { reauthenticateNowButton.hidden = !required; }, reportClientEvent, browserSessionId
      });
      reauthenticateNowButton.addEventListener("click", () => { persistActiveDraft(); persistThreadDraft(); sessionController.reauthenticateNow(); });
      const http = new HttpClient({
        fetch: window.fetch.bind(window),
        refreshSession: () => sessionController.refresh(true),
        participant: () => new URLSearchParams(window.location.search).get("participant")
      });
      const notificationsApi = new NotificationApi(http);
      const processesApi = new ProcessApi(http);
      const agentsApi = new AgentApi(http);

      const serverEventMailbox = createServerEventMailbox({
        reduce: applicationStore.reduceServerEvent,
        deliver: renderServerEvent
      });

      function persistActiveDraft() {
        navigation.persistChannelDraft(activeChannelId, bodyInput.value);
      }

      function restoreActiveDraft() {
        bodyInput.value = navigation.restoreChannelDraft(activeChannelId);
        syncComposerState();
      }

      function persistThreadDraft(rootId: string | null = activeThreadRootId, channelId: string | null = activeChannelId) {
        if (!rootId || !channelId) return;
        const state = threadComposerStates.get(rootId);
        if (!state) return;
        navigation.persistThreadDraft(channelId, rootId, state.draft);
      }

      function restoreThreadDraft(rootId: string, channelId: string): string {
        return navigation.restoreThreadDraft(channelId, rootId);
      }

      function clearThreadDraft(rootId: string | null, channelId: string | null): void {
        if (!rootId || !channelId) return;
        navigation.clearThreadDraft(channelId, rootId);
      }

      function activeChannelMedia() {
        return pendingMedia.filter((media) => media.channel_id === activeChannelId);
      }

      function resizeComposer() {
        const styles = window.getComputedStyle(bodyInput);
        const minimum = Number.parseFloat(styles.minHeight);
        const maximum = Number.parseFloat(styles.maxHeight);
        bodyInput.style.height = "auto";
        const height = bodyInput.value.length === 0
          ? minimum
          : Math.min(bodyInput.scrollHeight, maximum);
        bodyInput.style.height = `${height}px`;
        bodyInput.style.overflowY = bodyInput.value.length > 0 && bodyInput.scrollHeight > maximum
          ? "auto"
          : "hidden";
      }

      function syncComposerState() {
        const expanded = composerHasFocus
          || bodyInput.value.length > 0
          || activeChannelMedia().length > 0
          || uploadStatus.textContent.length > 0
          || !mentionSuggestions.hidden;
        sendForm.classList.toggle("is-expanded", expanded);
        composerTools.hidden = !composerHasFocus;
        resizeComposer();
      }

      function closeComposerToolsAfterFocusLeaves() {
        window.setTimeout(() => {
          if (sendForm.contains(document.activeElement)) return;
          composerHasFocus = false;
          messageEmojiPicker.open = false;
          closeMentionSuggestions();
          syncComposerState();
        }, 0);
      }

      function setActiveCircle(circleId: string | null): void {
        if (!circleId) return;
        navigation.setActiveCircle(circleId);
        syncRenderedNavigation();
      }

      function clearActiveCircle(circleId: string | null = null): void {
        navigation.clearActiveCircle(circleId);
        syncRenderedNavigation();
      }

      function restoreActiveCircle() {
        const fallback = navigation.restoreActiveCircle(knownCircles.keys(), circleSelect.value || null);
        syncRenderedNavigation();
        if (!fallback) {
          circleSelect.value = "";
          return null;
        }
        circleSelect.value = fallback;
        return fallback;
      }

      function rememberCircleChannel(channel: Channel): void {
        navigation.rememberCircleChannel(channel);
      }

      function forgetCircleChannel(circleId: string): void {
        navigation.forgetCircleChannel(circleId);
      }

      function preferredCircleChannel(circleId: string, channels: Channel[] = knownChannels): Channel | undefined {
        return navigation.preferredCircleChannel(circleId, channels);
      }

      const lastRecoveryTelemetryAt = new Map<string, number>();
      function reportClientEvent(event: string): void {
        if (event === "resume_recovery" || event === "connect_timeout" || event === "liveness_timeout") {
          const now = Date.now();
          if (now - (lastRecoveryTelemetryAt.get(event) ?? 0) < 30_000) return;
          lastRecoveryTelemetryAt.set(event, now);
        }
        const participant = new URL(window.location.href).searchParams.get("participant");
        const query = participant ? `?participant=${encodeURIComponent(participant)}` : "";
        fetch(`/api/v1/client-events${query}`, {
          method: "POST",
          credentials: "same-origin",
          cache: "no-store",
          keepalive: true,
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ event })
        }).catch(() => {});
      }

      async function recoverConnection(replaceOpenSocket = false) {
        return connectionSupervisor.recover(async (connection) => {
          let response;
          try {
            response = await fetchWithTimeout(window.fetch.bind(window), window.setTimeout.bind(window), window.clearTimeout.bind(window), "/auth/session", {
              credentials: "same-origin",
              cache: "no-store",
              headers: { "accept": "application/json" }
            });
          } catch (_) {
            connectionSupervisor.scheduleReconnect(1006, "ventar på nett");
            return;
          }
          if (response.status === 401) {
            await sessionController.recoverAuthentication();
            return;
          }
          if (!response.ok) {
            connectionSupervisor.scheduleReconnect(response.status, "kunne ikkje kontrollere økta");
            return;
          }
          sessionController.schedule(sessionRefreshAfterSeconds(await response.json()));
          if (!connection.connected || connection.closing) {
            connectionSupervisor.connect(true);
          } else if (replaceOpenSocket) {
            connectionSupervisor.connect(true, true);
          }
        });
      }

      function resumeAfterBackground(force = false) {
        if (document.visibilityState === "hidden") return;
        const now = Date.now();
        if (!shouldForceResume(now, hiddenSince, force)) return;
        if (now - lastBackgroundRecoveryAt < 5_000) return;
        lastBackgroundRecoveryAt = now;
        // iOS may retain an OPEN WebSocket after sleep although its TCP path
        // has vanished. Do not attempt a normal token handoff in that state:
        // discard it, unlock any pending composer request, and resubscribe on
        // a fresh generation. The old socket can no longer mutate UI state.
        connectionSupervisor.recoverAfterResume();
        recoverConnection(false)
          .catch(() => connectionSupervisor.scheduleReconnect(1006, "kunne ikkje gjenopprette sambandet"));
      }

      function noteUserActivity() {
        lastUserActivityAt = Date.now();
      }

      window.addEventListener("pointerdown", noteUserActivity, { passive: true });
      window.addEventListener("keydown", noteUserActivity, { passive: true });
      window.addEventListener("input", noteUserActivity, { passive: true });

      window.addEventListener("pageshow", (event) => resumeAfterBackground(event.persisted));
      window.addEventListener("focus", () => resumeAfterBackground(false));
      window.addEventListener("online", () => resumeAfterBackground(true));
      window.addEventListener("pageshow", refreshVisibleInvitationCards);
      window.addEventListener("focus", refreshVisibleInvitationCards);

      function renderMediaPreviews() {
        mediaPreviews.replaceChildren(...activeChannelMedia().map((media) => {
          const item = document.createElement("span");
          item.className = "media-preview";
          const label = document.createElement("span");
          label.className = "media-preview-label";
          label.textContent = `${media.content_type.startsWith("video/") ? "🎬" : "🖼️"} ${media.original_filename}`;
          const remove = document.createElement("button");
          remove.type = "button";
          remove.className = "media-preview-remove";
          remove.textContent = "×";
          remove.setAttribute("aria-label", `Fjern ${media.original_filename}`);
          remove.addEventListener("click", () => {
            if (pendingMessages.size > 0) return;
            pendingMedia = pendingMedia.filter((candidate) => candidate.id !== media.id);
            renderMediaPreviews();
            setUploadStatus(`${media.original_filename} er fjerna.`);
            window.setTimeout(() => {
              if (uploadStatus.textContent === `${media.original_filename} er fjerna.`) setUploadStatus("");
            }, 2_000);
            bodyInput.focus({ preventScroll: true });
          });
          item.append(label, remove);
          return item;
        }));
        syncComposerState();
      }

      function setUploadStatus(message: string, kind: string = "progress"): void {
        uploadStatus.textContent = message;
        uploadStatus.dataset.kind = kind;
        uploadStatus.setAttribute("aria-live", kind === "error" ? "assertive" : "polite");
        syncComposerState();
      }

      async function uploadFailureMessage(response: UploadResponse, filename: string): Promise<string> {
        let detail = "";
        try { detail = (await response.text()).trim(); } catch (_) {}
        const trace = response.headers.get("cf-ray") || response.headers.get("x-request-id");
        const reason = detail && !detail.startsWith("<") ? `: ${detail}` : "";
        const reference = trace ? ` Referanse: ${trace}.` : "";
        return `Opplasting av ${filename} feila (HTTP ${response.status})${reason}.${reference}`;
      }

      function postMedia(url: string, form: FormData, filename: string, setStatus: (message: string, kind?: string) => void = setUploadStatus): Promise<UploadResponse> {
        return new Promise((resolve, reject) => {
          const request = new XMLHttpRequest();
          request.open("POST", url);
          request.withCredentials = true;
          request.setRequestHeader("accept", "application/json");
          request.upload.addEventListener("progress", (event) => {
            const progress = event.lengthComputable ? ` ${Math.min(100, Math.round(event.loaded * 100 / event.total))} %` : "";
            setStatus(`Lastar opp ${filename}${progress} …`);
          });
          request.upload.addEventListener("load", () => {
            setStatus(`Opplasting av ${filename} er ferdig. Behandlar fila …`);
          });
          request.addEventListener("load", () => resolve({
            status: request.status,
            ok: request.status >= 200 && request.status < 300,
            headers: { get: (name: string) => request.getResponseHeader(name) },
            text: async () => request.responseText,
            json: async () => JSON.parse(request.responseText)
          }));
          request.addEventListener("error", () => reject(new Error("Nettverkssambandet vart brote")));
          request.addEventListener("abort", () => reject(new Error("Opplastinga vart avbroten")));
          request.send(form);
        });
      }

      async function uploadMediaFiles(files: Iterable<File>): Promise<void> {
        if (!activeChannelId) return;
        for (const file of files) {
          if (!file.size || file.size > 35 * 1024 * 1024) {
            setUploadStatus(`${file.name || "Fila"} må vere mellom 1 byte og 35 MiB.`, "error");
            continue;
          }
          const form = new FormData();
          form.append("file", file, file.name || "clipboard-image.png");
          const filename = file.name || "bilete";
          setUploadStatus(`Gjer klar ${filename} (${(file.size / 1024 / 1024).toFixed(1)} MiB) …`);
          const participant = new URL(window.location.href).searchParams.get("participant");
          const authQuery = participant ? `?participant=${encodeURIComponent(participant)}` : "";
          const url = `/api/v1/channels/${activeChannelId}/media${authQuery}`;
          let response;
          try {
            response = await postMedia(url, form, filename);
            if (response.status === 401 && await sessionController.refresh(true)) {
              response = await postMedia(url, form, filename);
            }
          } catch (error) {
            reportClientEvent("upload_failed");
            const online = navigator.onLine ? "Nettlesaren fekk ikkje noko HTTP-svar frå tenesta" : "Eininga er fråkopla nettet";
            setUploadStatus(`Opplasting av ${file.name || "fila"} feila: ${online}. ${error instanceof Error ? error.message : "Ukjend nettverksfeil"}.`, "error");
            continue;
          }
          if (response.status === 401) {
            reportClientEvent("upload_failed");
            setUploadStatus("Opplasting feila (HTTP 401): Økta kunne ikkje fornyast. Logg inn på nytt.", "error");
            continue;
          }
          if (!response.ok) { reportClientEvent("upload_failed"); setUploadStatus(await uploadFailureMessage(response, file.name || "fila"), "error"); continue; }
          let media: MediaObject | null;
          try { media = mediaFromUpload(await response.json()); } catch { media = null; }
          if (media === null) { reportClientEvent("upload_failed"); setUploadStatus("Opplastinga var ferdig, men tenesta svarte med ugyldige mediedata. Prøv igjen.", "error"); continue; }
          pendingMedia.push(media);
          renderMediaPreviews();
          reportClientEvent("upload_succeeded");
          setUploadStatus(`${file.name || "Fila"} er behandla og klar til å sendast.`, "success");
        }
        setConnected(connectionSupervisor.snapshot().connected, "Tilkopla");
      }

      function threadComposerState(rootId = activeThreadRootId) {
        if (!rootId) return null;
        if (!threadComposerStates.has(rootId)) {
          threadComposerStates.set(rootId, { draft: "", media: [], status: "", statusKind: "progress", hasFocus: false, composing: false, uploadCount: 0 });
        }
        return threadComposerStates.get(rootId);
      }

      function hasPendingThreadReply(rootId = activeThreadRootId) {
        return [...pendingThreadReplies.values()].some((pending) => pending.rootId === rootId);
      }

      function resizeThreadComposer() {
        const styles = window.getComputedStyle(threadBody);
        const minimum = Number.parseFloat(styles.minHeight);
        const maximum = Number.parseFloat(styles.maxHeight);
        threadBody.style.height = "auto";
        const height = threadBody.value.length === 0 ? minimum : Math.min(threadBody.scrollHeight, maximum);
        threadBody.style.height = `${height}px`;
        threadBody.style.overflowY = threadBody.value.length > 0 && threadBody.scrollHeight > maximum ? "auto" : "hidden";
      }

      function syncThreadComposer() {
        const state = threadComposerState();
        if (!state) return;
        const expanded = state.hasFocus || threadBody.value.length > 0 || state.media.length > 0 || state.status.length > 0 || state.uploadCount > 0 || threadEmojiPicker.open;
        threadForm.classList.toggle("is-expanded", expanded);
        threadComposerTools.hidden = !state.hasFocus;
        const connection = connectionSupervisor.snapshot();
        const writable = Boolean(activeThreadRootId && activeChannelId && connection.subscribedChannelId === activeChannelId && connection.connected);
        threadBody.disabled = !writable;
        threadAttachMediaButton.disabled = !writable || state.uploadCount > 0;
        threadSendButton.disabled = !writable || state.uploadCount > 0 || hasPendingThreadReply();
        threadEmojiPicker.setAttribute("aria-disabled", String(!writable || state.uploadCount > 0));
        resizeThreadComposer();
      }

      function setThreadUploadStatus(message: string, kind: string = "progress", rootId: string | null = activeThreadRootId): void {
        const state = threadComposerState(rootId);
        if (!state) return;
        state.status = message;
        state.statusKind = kind;
        if (rootId === activeThreadRootId) {
          threadUploadStatus.textContent = message;
          threadUploadStatus.dataset.kind = kind;
          threadUploadStatus.setAttribute("aria-live", kind === "error" ? "assertive" : "polite");
          syncThreadComposer();
        }
      }

      function renderThreadMediaPreviews() {
        const state = threadComposerState();
        if (!state) return;
        threadMediaPreviews.replaceChildren(...state.media.map((media) => {
          const item = document.createElement("span");
          item.className = "media-preview";
          const label = document.createElement("span");
          label.className = "media-preview-label";
          label.textContent = `${media.content_type.startsWith("video/") ? "🎬" : "🖼️"} ${media.original_filename}`;
          const remove = document.createElement("button");
          remove.type = "button";
          remove.className = "media-preview-remove";
          remove.textContent = "×";
          remove.setAttribute("aria-label", `Fjern ${media.original_filename}`);
          remove.addEventListener("click", () => {
            if (state.uploadCount > 0 || hasPendingThreadReply()) return;
            state.media = state.media.filter((candidate) => candidate.id !== media.id);
            setThreadUploadStatus(`${media.original_filename} er fjerna.`);
            renderThreadMediaPreviews();
            threadBody.focus({ preventScroll: true });
          });
          item.append(label, remove);
          return item;
        }));
        syncThreadComposer();
      }

      async function uploadThreadMediaFiles(files: Iterable<File>): Promise<void> {
        const channelId = activeChannelId;
        const rootId = activeThreadRootId;
        const state = threadComposerState(rootId);
        if (!channelId || !rootId || !state) return;
        for (const file of files) {
          if (!file.size || file.size > 35 * 1024 * 1024) {
            setThreadUploadStatus(`${file.name || "Fila"} må vere mellom 1 byte og 35 MiB.`, "error", rootId);
            continue;
          }
          state.uploadCount += 1;
          syncThreadComposer();
          const form = new FormData();
          form.append("file", file, file.name || "clipboard-image.png");
          const filename = file.name || "bilete";
          const participant = new URL(window.location.href).searchParams.get("participant");
          const authQuery = participant ? `?participant=${encodeURIComponent(participant)}` : "";
          const url = `/api/v1/channels/${channelId}/media${authQuery}`;
          try {
            let response = await postMedia(url, form, filename, (message, kind) => setThreadUploadStatus(message, kind, rootId));
            if (response.status === 401 && await sessionController.refresh(true)) response = await postMedia(url, form, filename, (message, kind) => setThreadUploadStatus(message, kind, rootId));
            if (response.status === 401) throw new Error("Økta kunne ikkje fornyast. Logg inn på nytt.");
            if (!response.ok) throw new Error(await uploadFailureMessage(response, filename));
            const media = mediaFromUpload(await response.json());
            if (media === null) throw new Error("Serveren svarte med ugyldige mediedata.");
            // The upload belongs to the channel and root that were active when it started.
            state.media.push({ ...media, channel_id: channelId, parent_message_id: rootId });
            reportClientEvent("upload_succeeded");
            setThreadUploadStatus(`${filename} er behandla og klar til å sendast.`, "success", rootId);
          } catch (error) {
            reportClientEvent("upload_failed");
            setThreadUploadStatus(`Opplasting av ${filename} feila: ${error instanceof Error ? error.message : "Ukjend feil"}`, "error", rootId);
          } finally {
            state.uploadCount = Math.max(0, state.uploadCount - 1);
            if (rootId === activeThreadRootId) syncThreadComposer();
          }
        }
      }

      attachMediaButton.addEventListener("click", () => {
        if (!attachMediaButton.disabled) mediaInput.click();
      });
      mediaInput.addEventListener("change", () => {
        uploadMediaFiles(mediaInput.files ? [...mediaInput.files] : []);
        mediaInput.value = "";
        bodyInput.focus();
      });
      bodyInput.addEventListener("paste", (event) => {
        const files = [...(event.clipboardData?.files ?? [])].filter((file) => file.type.startsWith("image/") || file.type.startsWith("video/"));
        if (files.length) { event.preventDefault(); uploadMediaFiles(files); }
      });
      threadAttachMediaButton.addEventListener("click", () => {
        if (!threadAttachMediaButton.disabled) threadMediaInput.click();
      });
      threadMediaInput.addEventListener("change", () => {
        uploadThreadMediaFiles(threadMediaInput.files ? [...threadMediaInput.files] : []);
        threadMediaInput.value = "";
        threadBody.focus({ preventScroll: true });
      });
      threadBody.addEventListener("paste", (event) => {
        const files = [...(event.clipboardData?.files ?? [])].filter((file) => file.type.startsWith("image/") || file.type.startsWith("video/"));
        if (files.length) { event.preventDefault(); uploadThreadMediaFiles(files); }
      });
      requireElement("#media-lightbox-close", HTMLButtonElement).addEventListener("click", () => mediaLightbox.close());
      requireElement("#thread-close", HTMLButtonElement).addEventListener("click", () => threadPanel.close());
      threadPanel.addEventListener("close", () => {
        persistThreadDraft();
        activeThreadRootId = null;
        threadEmojiPicker.open = false;
      });
      circleChannelClose.addEventListener("click", () => circleChannelDialog.close());
      circleAdminClose.addEventListener("click", () => circleAdminDialog.close());
      circleAdminDialog.addEventListener("close", () => {
        if (bottomCirclePanel.open) circleToolSettings.focus({ preventScroll: true });
      });
      requireElement("#direct-message-close", HTMLButtonElement).addEventListener("click", () => directMessageDialog.close());
      channelPeopleButton.addEventListener("click", () => openChannelDetails(false));
      connectionStatusToggle.addEventListener("click", () => {
        connectionStatusToggle.setAttribute("aria-expanded", String(connectionStatusToggle.getAttribute("aria-expanded") !== "true"));
      });
      document.addEventListener("pointerdown", (event) => {
        const target = event.target instanceof Element ? event.target : null;
        if (!target?.closest(".connection-status")) connectionStatusToggle.setAttribute("aria-expanded", "false");
        if (threadPanel.open && !threadEmojiPicker.contains(target)) threadEmojiPicker.open = false;
      });
      channelDetailsClose.addEventListener("click", () => channelDetailsDialog.close());
      channelMemberSearch.addEventListener("input", () => {
        const channelId = channelDetailsDialog.dataset.channelId;
        if (channelId) renderChannelMembers(channelId);
      });
      channelDescriptionForm.addEventListener("submit", (event) => {
        event.preventDefault();
        const channelId = channelDetailsDialog.dataset.channelId;
        if (!channelId) return;
        channelDescriptionStatus.textContent = "Lagrar …";
        sendCommand("update_channel_description", {
          channel_id: channelId,
          description: channelDescriptionInput.value
        });
      });
      circleChannelDialog.addEventListener("close", () => { managedCircleId = null; });
      circleChannelCreate.addEventListener("submit", (event) => {
        event.preventDefault();
        const name = managedChannelName.value.trim();
        if (!managedCircleId || !name) return;
        const kind = managedChannelKind.value;
        if (kind !== "public" && kind !== "local" && kind !== "private") return;
        sendCommand("create_channel", {
          slug: scopedCircleChannelSlug(managedCircleId, name),
          name,
          kind,
          circle_id: managedCircleId
        });
      });
      leaveCircleButton.addEventListener("click", () => {
        if (!managedCircleId) return;
        const circle = knownCircles.get(managedCircleId);
        if (!circle || circle.role === "owner") return;
        if (!window.confirm(`Vil du forlate vennekretsen ${circle.name}? Du mistar tilgang til alle kanalane i kretsen.`)) return;
        sendCommand("leave_circle", { circle_id: circle.id });
      });
      threadForm.addEventListener("submit", (event) => {
        event.preventDefault();
        const rootId = activeThreadRootId;
        const channelId = activeChannelId;
        const state = threadComposerState(rootId);
        const draft = threadBody.value.trim();
        const media = state?.media || [];
        const mediaTokens = media.map((item) => `[[media:${item.id}|${item.content_type}|${encodeURIComponent(item.original_filename)}]]`).join("\n");
        const body = [draft, mediaTokens].filter(Boolean).join("\n");
        if (!body || !rootId || !channelId || !state || state.uploadCount > 0) return;
        const requestId = sendCommand("send_message", {
          channel_id: channelId,
          parent_message_id: activeThreadRootId,
          body
        });
        if (!requestId) return;
        pendingThreadReplies.set(requestId, { rootId, channelId, body, draft, mediaIds: media.map((item) => item.id) });
        threadBody.value = "";
        state.draft = "";
        threadBody.readOnly = true;
        syncThreadComposer();
      });
      threadForm.addEventListener("focusin", () => {
        const state = threadComposerState();
        if (state) state.hasFocus = true;
        syncThreadComposer();
      });
      threadForm.addEventListener("focusout", () => window.setTimeout(() => {
        if (threadForm.contains(document.activeElement)) return;
        const state = threadComposerState();
        if (state) state.hasFocus = false;
        threadEmojiPicker.open = false;
        syncThreadComposer();
      }, 0));
      threadBody.addEventListener("input", () => {
        const state = threadComposerState();
        if (state) {
          state.draft = threadBody.value;
          persistThreadDraft();
        }
        syncThreadComposer();
      });
      threadBody.addEventListener("compositionstart", () => { const state = threadComposerState(); if (state) state.composing = true; });
      threadBody.addEventListener("compositionend", () => { const state = threadComposerState(); if (state) { state.composing = false; state.draft = threadBody.value; persistThreadDraft(); } syncThreadComposer(); });
      threadBody.addEventListener("keydown", (event) => {
        const state = threadComposerState();
        if (event.key === "Enter" && !event.shiftKey && !event.isComposing && event.keyCode !== 229 && !state?.composing && usesDesktopComposerKeys.matches) {
          event.preventDefault();
          threadForm.requestSubmit();
        }
      });
      mediaLightbox.addEventListener("click", (event) => {
        if (event.target === mediaLightbox) mediaLightbox.close();
      });
      mediaLightbox.addEventListener("close", () => {
        mediaLightboxImage.removeAttribute("src");
      });

      function mentionHandle(user: UserProfile): string {
        return user.display_name.toLocaleLowerCase().replace(/[^\p{L}\p{N}_-]/gu, "");
      }

      function closeMentionSuggestions() {
        activeMention = null;
        mentionMatches = [];
        selectedMentionIndex = 0;
        mentionSuggestions.hidden = true;
        bodyInput.setAttribute("aria-expanded", "false");
        bodyInput.removeAttribute("aria-activedescendant");
        syncComposerState();
      }

      function mentionCandidates() {
        const channel = knownChannels.find((item) => item.id === activeChannelId);
        return channel?.circle_id ? (knownCircleUsers.get(channel.circle_id) || []) : knownUsers;
      }

      function selectMention(index: number): void {
        const user = mentionMatches[index];
        if (!user || !activeMention) return;
        const replacement = `@${mentionHandle(user)} `;
        bodyInput.setRangeText(replacement, activeMention.start, activeMention.end, "end");
        closeMentionSuggestions();
        bodyInput.focus();
      }

      function renderMentionSuggestions() {
        mentionSuggestions.replaceChildren(...mentionMatches.map((user, index) => {
          const button = document.createElement("button");
          button.type = "button";
          button.id = `mention-option-${user.id}`;
          button.setAttribute("role", "option");
          button.setAttribute("aria-selected", String(index === selectedMentionIndex));
          const name = document.createElement("span");
          name.textContent = user.display_name;
          const handle = document.createElement("small");
          handle.textContent = `@${mentionHandle(user)}`;
          button.append(name, handle);
          button.addEventListener("pointerdown", (event) => event.preventDefault());
          button.addEventListener("click", () => selectMention(index));
          return button;
        }));
        mentionSuggestions.hidden = mentionMatches.length === 0;
        bodyInput.setAttribute("aria-expanded", String(mentionMatches.length > 0));
        if (mentionMatches.length > 0) {
          const selected = mentionMatches[selectedMentionIndex];
          if (selected) bodyInput.setAttribute("aria-activedescendant", `mention-option-${selected.id}`);
        } else {
          bodyInput.removeAttribute("aria-activedescendant");
        }
        syncComposerState();
      }

      function updateMentionSuggestions() {
        const caret = bodyInput.selectionStart;
        if (caret === null || bodyInput.selectionEnd !== caret) {
          closeMentionSuggestions();
          return;
        }
        const match = bodyInput.value.slice(0, caret).match(/(?:^|\s)@([\p{L}\p{N}_-]*)$/u);
        if (!match) {
          closeMentionSuggestions();
          return;
        }
        const query = match[1]?.toLocaleLowerCase();
        if (query === undefined) return;
        activeMention = { start: caret - query.length - 1, end: caret };
        mentionMatches = mentionCandidates()
          .filter((user) => mentionHandle(user).startsWith(query))
          .sort((left, right) => left.display_name.localeCompare(right.display_name));
        selectedMentionIndex = Math.min(selectedMentionIndex, Math.max(0, mentionMatches.length - 1));
        renderMentionSuggestions();
      }

      bodyInput.addEventListener("input", () => {
        persistActiveDraft();
        updateMentionSuggestions();
        syncComposerState();
      });
      bodyInput.addEventListener("click", updateMentionSuggestions);
      sendForm.addEventListener("focusin", () => { composerHasFocus = true; syncComposerState(); });
      sendForm.addEventListener("focusout", closeComposerToolsAfterFocusLeaves);
      document.addEventListener("pointerdown", (event) => {
        if (event.target instanceof Node && sendForm.contains(event.target)) return;
        composerHasFocus = false;
        messageEmojiPicker.open = false;
        closeMentionSuggestions();
        syncComposerState();
      });
      bodyInput.addEventListener("compositionstart", () => { composerComposing = true; syncComposerState(); });
      bodyInput.addEventListener("compositionend", () => { composerComposing = false; persistActiveDraft(); updateMentionSuggestions(); syncComposerState(); });
      bodyInput.addEventListener("keydown", (event) => {
        if (!mentionSuggestions.hidden && mentionMatches.length > 0 && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
          event.preventDefault();
          const direction = event.key === "ArrowDown" ? 1 : -1;
          selectedMentionIndex = (selectedMentionIndex + direction + mentionMatches.length) % mentionMatches.length;
          renderMentionSuggestions();
        } else if (!mentionSuggestions.hidden && mentionMatches.length > 0 && (event.key === "Enter" || event.key === "Tab")) {
          event.preventDefault();
          selectMention(selectedMentionIndex);
        } else if (!mentionSuggestions.hidden && mentionMatches.length > 0 && event.key === "Escape") {
          event.preventDefault();
          closeMentionSuggestions();
        } else if (event.key === "Enter" && !event.shiftKey && !event.isComposing && event.keyCode !== 229 && !composerComposing && usesDesktopComposerKeys.matches) {
          event.preventDefault();
          sendForm.requestSubmit();
        }
      });

      connectForm.addEventListener("submit", (event) => {
        event.preventDefault();
        connectionSupervisor.start();
      });

      sendForm.addEventListener("submit", (event) => {
        event.preventDefault();
        const draft = bodyInput.value.trim();
        const channelMedia = pendingMedia.filter((media) => media.channel_id === activeChannelId);
        const mediaTokens = channelMedia.map((media) => `[[media:${media.id}|${media.content_type}|${encodeURIComponent(media.original_filename)}]]`).join("\n");
        const body = [draft, mediaTokens].filter(Boolean).join("\n");
        if (!connectionSupervisor.snapshot().connected || !activeChannelId || body.length === 0) {
          return;
        }
        if (connectionSupervisor.snapshot().subscribedChannelId !== activeChannelId) return;
        const requestId = sendCommand("send_message", { channel_id: activeChannelId, body });
        if (!requestId) return;
        pendingMessages.set(requestId, { body, draft, mediaIds: channelMedia.map((media) => media.id), channelId: activeChannelId });
        bodyInput.value = "";
        persistActiveDraft();
        closeMentionSuggestions();
        bodyInput.readOnly = true;
        syncComposerState();
        setConnected(true, "Sender meldinga …");
      });

      function insertEmoji(input: HTMLInputElement | HTMLTextAreaElement, emoji: string): void {
        const start = input.selectionStart ?? input.value.length;
        const end = input.selectionEnd ?? start;
        input.setRangeText(emoji, start, end, "end");
        if (input === bodyInput) { persistActiveDraft(); syncComposerState(); }
        if (input === threadBody) { const state = threadComposerState(); if (state) { state.draft = threadBody.value; persistThreadDraft(); } syncThreadComposer(); }
        input.focus();
      }

      requireElements("#message-emoji-options [data-emoji]", HTMLButtonElement).forEach((button) => {
        button.addEventListener("click", () => {
          const emoji = button.dataset.emoji;
          if (!emoji) return;
          insertEmoji(bodyInput, emoji);
          messageEmojiPicker.open = false;
        });
      });
      requireElements("#thread-emoji-options [data-emoji]", HTMLButtonElement).forEach((button) => {
        button.addEventListener("click", () => {
          const emoji = button.dataset.emoji;
          if (!emoji) return;
          insertEmoji(threadBody, emoji);
          threadEmojiPicker.open = false;
          syncThreadComposer();
        });
      });
      requireElements("#status-emoji-options [data-emoji]", HTMLButtonElement).forEach((button) => {
        button.addEventListener("click", () => {
          const emoji = button.dataset.emoji;
          if (!emoji) return;
          statusEmoji.value = emoji;
          statusDraft.emoji = statusEmoji.value;
          statusDraft.text = statusText.value;
          statusDraft.dirty = true;
          requireElements("#status-emoji-options [data-emoji]", HTMLButtonElement).forEach((option) => {
            option.setAttribute("aria-pressed", String(option === button));
          });
          statusText.focus();
        });
      });
      [statusEmoji, statusText].forEach((input) => {
        input.addEventListener("input", () => {
          statusDraft.emoji = statusEmoji.value;
          statusDraft.text = statusText.value;
          statusDraft.dirty = true;
        });
      });
      requireElement("#save-status", HTMLButtonElement).addEventListener("click", () => {
        statusDraft.emoji = statusEmoji.value;
        statusDraft.text = statusText.value;
        statusDraft.dirty = true;
        sendCommand("set_status", { text: statusDraft.text, emoji: statusDraft.emoji, expires_at: null });
      });
      requireElement("#clear-status", HTMLButtonElement).addEventListener("click", () => {
        statusText.value = "";
        statusEmoji.value = "";
        statusDraft.text = "";
        statusDraft.emoji = "";
        statusDraft.dirty = true;
        sendCommand("set_status", { text: "", emoji: "", expires_at: null });
      });

      function vapidKeyBytes(value: string): Uint8Array<ArrayBuffer> {
        const padding = "=".repeat((4 - value.length % 4) % 4);
        const raw = atob((value + padding).replace(/-/g, "+").replace(/_/g, "/"));
        return Uint8Array.from(raw, (character) => character.charCodeAt(0));
      }

      async function loadNotificationSettings() {
        try {
          const settings = await notificationsApi.get();
          notificationMode.value = settings.preferences.mode;
          notificationDirect.checked = settings.preferences.directMessages;
          notificationMentions.checked = settings.preferences.mentions;
          const notificationLabel = settings.preferences.mode === "muted" ? "Varsel: ingen" : settings.preferences.mode === "weekly" ? "Varsel: kvar veke" : "Varsel: direkte";
          notificationSummaryLabel.textContent = notificationLabel;
          notificationSummary.setAttribute("aria-label", notificationLabel);
          notificationSummary.title = notificationLabel;
          notificationSummary.dataset.tooltip = notificationLabel;
          enableNotifications.disabled = !settings.enabled || !("PushManager" in window) || !("Notification" in window) || Notification.permission === "denied";
          enableNotifications.dataset.publicKey = settings.publicKey;
          notificationNotice.textContent = !settings.enabled ? "Push er ikkje konfigurert på serveren enno." : settings.subscriptions ? `${settings.subscriptions} eining(ar) tek imot varsel.` : "Varsel er ikkje slått på på denne eininga.";
        } catch (error) {
          notificationNotice.textContent = `Kunne ikkje hente varselinnstillingar: ${errorMessage(error)}`;
        }
      }

      requireElement("#save-notifications", HTMLButtonElement).addEventListener("click", async () => {
        try {
          if (notificationMode.value !== "instant" && notificationMode.value !== "weekly" && notificationMode.value !== "muted") throw new Error("Ugyldig varselmodus.");
          await notificationsApi.save({ mode: notificationMode.value, directMessages: notificationDirect.checked, mentions: notificationMentions.checked });
          notificationNotice.textContent = "Varselinnstillingane er lagra.";
          void loadNotificationSettings();
        } catch (error) {
          notificationNotice.textContent = `Kunne ikkje lagre: ${errorMessage(error)}`;
        }
      });

      enableNotifications.addEventListener("click", async () => {
        try {
          const registration = await serviceWorkerReady;
          if (!registration) throw new Error("Service worker er ikkje tilgjengeleg");
          const permission = await Notification.requestPermission();
          if (permission !== "granted") throw new Error("Varsel vart ikkje tillate");
          let subscription = await registration.pushManager.getSubscription();
          if (!subscription) {
            const publicKey = enableNotifications.dataset.publicKey;
            if (!publicKey) throw new Error("Serveren manglar offentleg Push-nøkkel");
            subscription = await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: vapidKeyBytes(publicKey) });
          }
          await notificationsApi.registerPush(subscription.toJSON());
          notificationNotice.textContent = "Varsel er slått på på denne eininga.";
          loadNotificationSettings();
        } catch (error) {
          notificationNotice.textContent = `Kunne ikkje slå på varsel: ${errorMessage(error)}`;
        }
      });

      loadNotificationSettings();

      directUser.addEventListener("change", () => {
        openDirect.disabled = !directUser.value;
        directMessageStatus.textContent = "";
      });
      openDirect.addEventListener("click", () => {
        if (!directUser.value) return;
        const selectedName = directUser.options[directUser.selectedIndex]?.textContent || "personen";
        directMessageStatus.textContent = `Opnar samtale med ${selectedName} …`;
        openDirect.disabled = true;
        if (!sendCommand("open_direct_channel", { user_id: directUser.value })) {
          directMessageStatus.textContent = "Sprøyt er ikkje tilkopla. Vent litt og prøv igjen.";
          openDirect.disabled = false;
        }
      });
      requireElement("#show-unread", HTMLButtonElement).addEventListener("click", () => showInbox("unread"));
      requireElement("#show-mentions", HTMLButtonElement).addEventListener("click", () => showInbox("mentions"));
      requireElement("#show-tasks", HTMLButtonElement).addEventListener("click", () => showInbox("tasks"));

      const desktopSidebarStorageKey = "sproyt.desktop-sidebar-collapsed.v1";
      const compactDesktopViewport = window.matchMedia("(min-width: 641px) and (max-width: 900px)");
      function sidebarIsCollapsed() {
        return sidebar.classList.contains("desktop-collapsed");
      }
      function setDesktopSidebarCollapsed(collapsed: boolean, persist: boolean = true): void {
        const effectiveCollapsed = collapsed;
        sidebar.classList.toggle("desktop-collapsed", effectiveCollapsed);
        sidebar.classList.toggle("desktop-expanded", !effectiveCollapsed);
        appMain.classList.toggle("desktop-sidebar-collapsed", effectiveCollapsed);
        appMain.classList.toggle("desktop-sidebar-expanded", !effectiveCollapsed);
        desktopSidebarToggle.setAttribute("aria-expanded", String(!effectiveCollapsed));
        const toggleLabel = effectiveCollapsed ? "Utvid menyen" : "Kollaps menyen";
        desktopSidebarToggle.setAttribute("aria-label", toggleLabel);
        desktopSidebarToggle.title = toggleLabel;
        desktopSidebarToggle.dataset.tooltip = toggleLabel;
        desktopSidebarToggle.textContent = effectiveCollapsed ? "›" : "‹";
        if (persist) {
          storedDesktopSidebarCollapsed = collapsed;
          try { window.localStorage.setItem(desktopSidebarStorageKey, String(collapsed)); } catch (_) {}
        }
      }
      let storedDesktopSidebarCollapsed = false;
      try { storedDesktopSidebarCollapsed = window.localStorage.getItem(desktopSidebarStorageKey) === "true"; } catch (_) {}
      setDesktopSidebarCollapsed(storedDesktopSidebarCollapsed || compactDesktopViewport.matches, false);
      desktopSidebarToggle.addEventListener("click", () => setDesktopSidebarCollapsed(!sidebarIsCollapsed()));
      compactDesktopViewport.addEventListener("change", () => setDesktopSidebarCollapsed(compactDesktopViewport.matches || storedDesktopSidebarCollapsed, false));
      function expandDesktopSidebarAndFocus(control: HTMLDetailsElement): boolean {
        if (!sidebarIsCollapsed()) return false;
        setDesktopSidebarCollapsed(false, false);
        control.open = true;
        window.requestAnimationFrame(() => control.querySelector("summary")?.focus());
        return true;
      }
      statusEditor.addEventListener("click", (event) => {
        if (event.target instanceof Element && event.target.closest("summary") && expandDesktopSidebarAndFocus(statusEditor)) event.preventDefault();
      });
      notificationEditor.addEventListener("click", (event) => {
        if (event.target instanceof Element && event.target.closest("summary") && expandDesktopSidebarAndFocus(notificationEditor)) event.preventDefault();
      });
      desktopAdvancedEntry?.addEventListener("click", () => {
        setDesktopSidebarCollapsed(false, false);
        window.requestAnimationFrame(() => {
          const control = document.querySelector<HTMLElement>(".advanced-tools button:not([disabled]), .advanced-tools input:not([disabled])");
          if (control) control.focus();
          else {
            processTitle.tabIndex = -1;
            processTitle.focus();
          }
        });
      });

      viewModeToggle.addEventListener("click", () => setRenderMode(renderMode === "raw" ? "view" : "raw"));
      function setMobileNavigationOpen(open: boolean, restoreFocus: boolean = false): void {
        if (open) {
          bottomChannelPanel.open = false;
          bottomCirclePanel.open = false;
        }
        sidebar.classList.toggle("mobile-open", open);
        if (open) {
          sidebar.setAttribute("role", "dialog");
          sidebar.setAttribute("aria-modal", "true");
          sidebar.setAttribute("aria-label", "Sprøyt-meny");
        } else {
          sidebar.removeAttribute("role");
          sidebar.removeAttribute("aria-modal");
          sidebar.removeAttribute("aria-label");
        }
        messagesEl.inert = open;
        composerArea.inert = open;
        mobileNavigationToggle.setAttribute("aria-expanded", String(open));
        if (open) {
          const firstControl = sidebar.querySelector<HTMLElement>("button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary");
          firstControl?.focus();
        }
        if (restoreFocus) mobileNavigationToggle.focus();
      }
      mobileNavigationToggle.addEventListener("click", () => {
        setMobileNavigationOpen(!sidebar.classList.contains("mobile-open"));
      });
      bottomChannelPanel.addEventListener("toggle", () => {
        if (bottomChannelPanel.open) bottomCirclePanel.open = false;
      });
      bottomCirclePanel.addEventListener("toggle", () => {
        if (bottomCirclePanel.open) {
          bottomChannelPanel.open = false;
          renderBottomNavigation();
        }
      });
      circleToolDirect.addEventListener("click", () => activateRootScope("direct"));
      circleToolShared.addEventListener("click", () => activateRootScope("shared"));
      circleToolSettings.addEventListener("click", () => {
        updateOnboardingButtons();
        if (!circleAdminDialog.open) circleAdminDialog.showModal();
        window.setTimeout(() => circleAdminClose.focus(), 0);
      });
      document.addEventListener("pointerdown", (event) => {
        if (circleAdminDialog.open) return;
        if (event.target instanceof Node && bottomNavigation.contains(event.target)) return;
        bottomChannelPanel.open = false;
        bottomCirclePanel.open = false;
      });
      document.addEventListener("keydown", (event) => {
        if (event.key === "Escape" && threadEmojiPicker.open) {
          event.preventDefault();
          threadEmojiPicker.open = false;
          threadEmojiPicker.querySelector("summary")?.focus({ preventScroll: true });
          return;
        }
        if (event.key === "Escape" && threadPanel.open) {
          const threadReactionPicker = threadMessages.querySelector<HTMLDetailsElement>(".reaction-picker[open]");
          if (threadReactionPicker) {
            event.preventDefault();
            threadReactionPicker.open = false;
            threadReactionPicker.closest(".message")?.classList.remove("reaction-picker-requested");
            threadReactionPicker.querySelector("summary")?.focus({ preventScroll: true });
            return;
          }
        }
        if (event.key === "Escape" && (circleChannelDialog.open || circleAdminDialog.open || threadPanel.open || mediaLightbox.open)) {
          return;
        }
        if (event.key === "Escape" && connectionStatusToggle.getAttribute("aria-expanded") === "true") {
          event.preventDefault();
          connectionStatusToggle.setAttribute("aria-expanded", "false");
          connectionStatusToggle.focus();
          return;
        }
        if (event.key === "Escape") {
          const reactionPicker = messagesEl.querySelector<HTMLDetailsElement>(".reaction-picker[open]");
          if (reactionPicker) {
            event.preventDefault();
            reactionPicker.open = false;
            reactionPicker.closest(".message")?.classList.remove("reaction-picker-requested");
            reactionPicker.querySelector("summary")?.focus({ preventScroll: true });
            return;
          }
          const messageMenu = messagesEl.querySelector<HTMLDetailsElement>(".message-menu[open]");
          if (messageMenu) {
            event.preventDefault();
            messageMenu.open = false;
            messageMenu.querySelector("summary")?.focus({ preventScroll: true });
            return;
          }
        }
        if (event.key === "Tab" && sidebar.classList.contains("mobile-open")) {
          const controls = Array.from(sidebar.querySelectorAll<HTMLElement>("button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary"))
            .filter((control) => !control.hidden && control.offsetParent !== null);
          if (controls.length > 0) {
            const first = controls[0];
            const last = controls.at(-1);
            if (event.shiftKey && first && last && document.activeElement === first) {
              event.preventDefault();
              last.focus();
            } else if (!event.shiftKey && last && document.activeElement === last) {
              event.preventDefault();
              if (first) first.focus();
            }
          }
        }
        if (event.key === "Escape" && sidebar.classList.contains("mobile-open")) {
          event.preventDefault();
          setMobileNavigationOpen(false, true);
          return;
        }
        if (event.key === "Escape" && bottomChannelPanel.open) {
          event.preventDefault();
          closeBottomNavigation(bottomChannelPanel, bottomChannelToggle);
          return;
        }
        if (event.key === "Escape" && bottomCirclePanel.open) {
          event.preventDefault();
          closeBottomNavigation(bottomCirclePanel, bottomCircleToggle);
          return;
        }
      });
      createCircleButton.addEventListener("click", () => sendCommand("create_circle", {
        name: circleName.value.trim(), slug: slugify(circleSlug.value || circleName.value)
      }));
      circleName.addEventListener("input", updateOnboardingButtons);
      invitationToken.addEventListener("input", updateOnboardingButtons);
      circleSelect.addEventListener("change", updateOnboardingButtons);
      circleSelect.addEventListener("change", () => {
        if (circleSelect.value) {
          setActiveCircle(circleSelect.value);
          sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
        } else {
          clearActiveCircle();
        }
      });
      channelMember.addEventListener("change", updateOnboardingButtons);
      inviteChannelMember.addEventListener("click", () => {
        const channel = knownChannels.find((item) => item.id === channelDetailsDialog.dataset.channelId);
        if (!channel?.circle_id || !channelMember.value) return;
        channelMemberStatus.textContent = "Lagar invitasjonen …";
        const requestId = sendCommand("create_invitation", {
          target: { type: "channel", circle_id: channel.circle_id, channel_id: channel.id }
        });
        if (requestId) pendingChannelInvitationRecipients.set(requestId, channelMember.value);
      });
      addChannelMember.addEventListener("click", () => {
        const channelId = channelDetailsDialog.dataset.channelId;
        if (channelId && channelMember.value) {
          channelMemberStatus.textContent = "Legg brukaren til …";
          sendCommand("add_channel_member", { channel_id: channelId, user_id: channelMember.value });
        }
      });
      createCircleInvitationButton.addEventListener("click", () => {
        if (circleSelect.value) sendCommand("create_invitation", { target: { type: "circle", circle_id: circleSelect.value } });
      });
      acceptInvitationButton.addEventListener("click", () => {
        const token = invitationValueToToken(invitationToken.value);
        if (token) {
          onboardingNotice.textContent = "Kontrollerer invitasjonen …";
          sendCommand("accept_invitation", { token });
        }
      });
      copyInvitation.addEventListener("click", async () => {
        try {
          await navigator.clipboard.writeText(invitationToken.value);
          onboardingNotice.textContent = "Invitasjonslenkja er kopiert. Send henne til venen du vil invitere.";
        } catch (_) {
          invitationToken.focus();
          invitationToken.select();
          onboardingNotice.textContent = "Kopier den markerte lenkja og send henne til venen din.";
        }
      });
      createAgentAccessButton.addEventListener("click", createTemporaryAgentAccess);
      copyAgentCredentialButton.addEventListener("click", async () => {
        try {
          await navigator.clipboard.writeText(agentCredential.value);
          agentAccessNotice.textContent = "Credential er kopiert. Handsam han som eit passord.";
        } catch (_) {
          agentCredential.hidden = false;
          agentCredential.focus();
          agentCredential.select();
          agentAccessNotice.textContent = "Kopier den markerte credentialen manuelt.";
        }
      });
      revokeAgentAccessButton.addEventListener("click", revokeTemporaryAgentAccess);
      deleteCircleButton.addEventListener("click", () => {
        if (!circleSelect.value) return;
        const selected = circleSelect.options[circleSelect.selectedIndex]?.textContent || "denne vennekretsen";
        if (window.confirm(`Slett ${selected} og all chat- og prosesshistorikk permanent?`)) {
          sendCommand("delete_circle", { circle_id: circleSelect.value });
        }
      });
      messagesEl.addEventListener("scroll", () => {
        if (messagesEl.scrollTop <= 80) loadOlderHistory();
      }, { passive: true });
      exportButton.addEventListener("click", async () => {
        try {
          const response = await fetch("/api/v1/me/export", {
            credentials: "same-origin"
          });
          if (!response.ok) throw new Error(await response.text() || `HTTP ${response.status}`);
          const blob = await response.blob();
          const url = URL.createObjectURL(blob);
          const link = document.createElement("a");
          link.href = url;
          link.download = "sproyt-export.json";
          link.click();
          URL.revokeObjectURL(url);
          pushSystem("Dataeksporten er laga.");
        } catch (error) {
          pushSystem(`Kunne ikkje eksportere data: ${errorMessage(error)}`);
        }
      });
      const [enableHeartButton, startProcessButton, refreshProcessButton, inspectProcessButton, processYesButton, processNoButton] = processButtons;
      if (!enableHeartButton || !startProcessButton || !refreshProcessButton || !inspectProcessButton || !processYesButton || !processNoButton) {
        throw new Error("Manglar påkravde prosesskontrollar");
      }
      enableHeartButton.addEventListener("click", () => setHeartFeature(true));
      startProcessButton.addEventListener("click", startEventPlanning);
      refreshProcessButton.addEventListener("click", refreshProcess);
      inspectProcessButton.addEventListener("click", inspectProcess);
      processYesButton.addEventListener("click", () => answerProcess("yes"));
      processNoButton.addEventListener("click", () => answerProcess("no"));

      function slugify(value: string): string {
        return value.trim().toLowerCase().replace(/[^a-z0-9_-]+/g, "-");
      }

      function scopedCircleChannelSlug(circleId: string, value: string): string {
        const scope = circleId.replace(/-/g, "");
        const base = slugify(value).replace(/^-+|-+$/g, "") || "kanal";
        return `${scope}-${base.slice(0, 47)}`;
      }

      function invitationValueToToken(value: string): string {
        const candidate = value.trim();
        if (!candidate) return "";
        const meta = candidate.match(/^\[\[invite:([A-Za-z0-9_-]{32,128})\]\]$/);
        if (meta?.[1]) return meta[1];
        try {
          const url = new URL(candidate, window.location.origin);
          return url.searchParams.get("invite") || candidate;
        } catch (_) {
          return candidate;
        }
      }

      function markDeliveryUncertain(requestId: string): void {
        const message = pendingMessages.get(requestId);
        if (message) {
          pendingMessages.delete(requestId);
          uncertainMessages.set(requestId, message);
          navigation.persistChannelDraft(message.channelId, message.draft);
          if (activeChannelId === message.channelId) {
            bodyInput.readOnly = true;
            syncComposerState();
            setConnected(false, "Kontrollerer om meldinga kom fram …");
          }
        }
        const reply = pendingThreadReplies.get(requestId);
        if (reply) {
          pendingThreadReplies.delete(requestId);
          uncertainThreadReplies.set(requestId, reply);
          navigation.persistThreadDraft(reply.channelId, reply.rootId, reply.draft);
          if (activeChannelId === reply.channelId && activeThreadRootId === reply.rootId) {
            threadBody.readOnly = true;
            syncThreadComposer();
          }
        }
      }

      function reconcileUncertainDeliveries(channelId: string, _messages: readonly ChatMessage[]): void {
        for (const [requestId, pending] of uncertainMessages) {
          if (pending.channelId !== channelId) continue;
          if (!retriedUncertainRequests.has(requestId) && activeChannelId === channelId) {
            if (resendCommand(requestId, "send_message", { channel_id: channelId, body: pending.body })) {
              retriedUncertainRequests.add(requestId);
              bodyInput.readOnly = true;
              setConnected(true, "Avklarer sendinga …");
            }
          }
        }
        for (const [requestId, pending] of uncertainThreadReplies) {
          if (pending.channelId !== channelId) continue;
          if (!retriedUncertainRequests.has(requestId) && activeChannelId === channelId) {
            if (resendCommand(requestId, "send_message", { channel_id: channelId, parent_message_id: pending.rootId, body: pending.body })) {
              retriedUncertainRequests.add(requestId);
              if (activeThreadRootId === pending.rootId) { threadBody.readOnly = true; syncThreadComposer(); }
            }
          }
        }
      }

      function finishPendingMessage(requestId: string | undefined, message: ChatMessage): void {
        if (!requestId) return;
        const pending = pendingMessages.get(requestId) ?? uncertainMessages.get(requestId);
        if (!pending) return;
        if (message?.channel_id !== pending.channelId || message?.body !== pending.body) {
          console.warn("Sendekvitteringa samsvarar ikkje med kommandoen", {
            requestId,
            requestedChannelId: pending.channelId,
            acceptedChannelId: message?.channel_id,
            acceptedMessageId: message?.id
          });
          failPendingMessage(requestId, "tenaren svarte med ei eldre meldingskvittering; utkastet er bevart");
          return;
        }
        pendingMessages.delete(requestId); uncertainMessages.delete(requestId); retriedUncertainRequests.delete(requestId);
        navigation.persistChannelDraft(pending.channelId, "");
        pendingMedia = pendingMedia.filter((media) => !pending.mediaIds.includes(media.id));
        if (message?.channel_id === activeChannelId) {
          bodyInput.readOnly = false;
          renderMediaPreviews();
          setUploadStatus("");
          bodyInput.focus();
          syncComposerState();
        }
        setConnected(connectionSupervisor.snapshot().connected, "Tilkopla");
      }

      function pendingMessageToReveal(message: ChatMessage, requestId: string | null = null): PendingMessage | null {
        if (message.sender_id !== currentParticipantId) return null;
        const requested = requestId ? pendingMessages.get(requestId) ?? uncertainMessages.get(requestId) : null;
        if (requested?.channelId === message.channel_id && requested.body === message.body) return requested;
        return [...pendingMessages.values()].find((pending) =>
          pending.channelId === message.channel_id && pending.body === message.body
        ) || null;
      }

      function failPendingMessage(requestId: string | undefined, message: string): void {
        if (!requestId) return;
        const pending = pendingMessages.get(requestId) ?? uncertainMessages.get(requestId);
        if (!pending) return;
        pendingMessages.delete(requestId); uncertainMessages.delete(requestId); retriedUncertainRequests.delete(requestId);
        navigation.persistChannelDraft(pending.channelId, pending.draft);
        if (activeChannelId === pending.channelId) {
          bodyInput.readOnly = false;
          if (bodyInput.value.trim().length === 0) bodyInput.value = pending.draft;
          persistActiveDraft(); syncComposerState();
          setConnected(connectionSupervisor.snapshot().connected, `Meldinga vart ikkje sendt: ${message}`);
          bodyInput.focus();
        }
      }

      function finishPendingThreadReply(requestId: string | undefined, message: ChatMessage): void {
        if (!requestId) return;
        const pending = pendingThreadReplies.get(requestId) ?? uncertainThreadReplies.get(requestId);
        if (!pending) return;
        pendingThreadReplies.delete(requestId); uncertainThreadReplies.delete(requestId); retriedUncertainRequests.delete(requestId);
        const state = threadComposerState(pending.rootId);
        if (message?.parent_message_id !== pending.rootId || message?.channel_id !== pending.channelId || message?.body !== pending.body) {
          if (activeThreadRootId === pending.rootId && threadBody.value.trim().length === 0) {
            threadBody.value = pending.draft;
          }
          if (state) state.draft = pending.draft;
          if (activeThreadRootId === pending.rootId) threadBody.readOnly = false;
          persistThreadDraft(pending.rootId, pending.channelId);
          setConnected(connectionSupervisor.snapshot().connected, "Tråden fekk ei ugyldig sendekvittering; svaret er bevart");
          syncThreadComposer();
          return;
        }
        navigation.persistThreadDraft(pending.channelId, pending.rootId, "");
        if (state) state.media = state.media.filter((media) => !pending.mediaIds.includes(media.id));
        if (state) state.draft = "";
        clearThreadDraft(pending.rootId, pending.channelId);
        if (activeThreadRootId === pending.rootId) { threadBody.readOnly = false; renderThreadMediaPreviews(); }
        return;
      }

      function failPendingThreadReply(requestId: string | undefined, message: string): boolean {
        if (!requestId) return false;
        const pending = pendingThreadReplies.get(requestId) ?? uncertainThreadReplies.get(requestId);
        if (!pending) return false;
        pendingThreadReplies.delete(requestId); uncertainThreadReplies.delete(requestId); retriedUncertainRequests.delete(requestId);
        const state = threadComposerState(pending.rootId);
        if (activeChannelId === pending.channelId && activeThreadRootId === pending.rootId && threadBody.value.trim().length === 0) {
          threadBody.value = pending.draft;
        }
        if (state) state.draft = pending.draft;
        navigation.persistThreadDraft(pending.channelId, pending.rootId, pending.draft);
        if (activeChannelId === pending.channelId && activeThreadRootId === pending.rootId) {
          threadBody.readOnly = false;
          setConnected(connectionSupervisor.snapshot().connected, `Trådsvaret vart ikkje sendt: ${message}`);
          syncThreadComposer();
        }
        return true;
      }

      function setConnected(connected: boolean, status: string): void {
        applicationStore.updateConnection({ connected, status });
        setConnectionStatus(status);
        const writableChannel = connected
          && activeChannelId !== null
          && connectionSupervisor.snapshot().subscribedChannelId === activeChannelId;
        bodyInput.disabled = !writableChannel;
        sendButton.disabled = !writableChannel || pendingMessages.size > 0;
        attachMediaButton.disabled = !writableChannel || pendingMessages.size > 0;
        messageEmojiPicker.setAttribute("aria-disabled", String(!writableChannel || pendingMessages.size > 0));
        syncThreadComposer();
        circleButtons.forEach((button) => { button.disabled = !connected; });
        exportButton.disabled = !connected;
        processButtons.forEach((button) => { button.disabled = !connected; });
        updateOnboardingButtons();
      }

      function setConnectionStatus(status: string): void {
        applicationStore.updateConnection({ status });
        const connection = applicationStore.snapshot.connection;
        connectionStatusText.textContent = connection.status;
        const routine = connection.status === "Tilkopla";
        const reconnecting = /^(Fornyar økta|Gjenopprettar samtalen|Koplar til)/.test(connection.status);
        statusEl.dataset.routine = String(routine);
        connectionStatusDot.dataset.routine = String(routine);
        connectionStatusDot.dataset.reconnecting = String(reconnecting);
        connectionStatusToggle.setAttribute("aria-label", `Sambandsstatus: ${connection.status}`);
        connectionStatusToggle.title = connection.status;
      }

      function updateOnboardingButtons() {
        const connected = connectionSupervisor.snapshot().connected;
        createCircleButton.disabled = !connected || circleName.value.trim().length < 2;
        createCircleInvitationButton.disabled = !connected || !circleSelect.value;
        acceptInvitationButton.disabled = !connected || !invitationValueToToken(invitationToken.value);
        const selectedCircle = knownCircles.get(circleSelect.value);
        deleteCircleButton.hidden = selectedCircle?.role !== "owner";
        deleteCircleButton.disabled = !connected || selectedCircle?.role !== "owner";
        const memberChannel = knownChannels.find((channel) => channel.id === channelDetailsDialog.dataset.channelId);
        const canManageMember = connected && channelMember.value && memberChannel && ["owner", "moderator"].includes(memberChannel.role);
        addChannelMember.disabled = !canManageMember;
        inviteChannelMember.disabled = !canManageMember || !memberChannel.circle_id;
      }

      async function setHeartFeature(enabled: boolean): Promise<void> {
        if (!circleSelect.value) {
          pushSystem("Vel ein vennekrets før event-planlegging blir slått på.");
          return;
        }
        try {
          await processesApi.setHeartFeature(circleSelect.value, enabled);
          pushSystem(enabled ? "Event-planlegging er slått på for kretsen." : "Event-planlegging er slått av.");
        } catch (error) {
          pushSystem(`Kunne ikkje endre event-planlegging: ${error instanceof Error ? error.message : "ukjend feil"}`);
        }
      }

      async function startEventPlanning() {
        if (!activeChannelId || !circleSelect.value) {
          pushSystem("Vel ein kretskanal før du startar planlegging.");
          return;
        }
        try {
          processId.value = await processesApi.startEventPlanning({ channelId: activeChannelId, requestId: crypto.randomUUID(), title: processTitle.value.trim() || "Event-planlegging" });
          await refreshProcess();
        } catch (error) {
          pushSystem(`Kunne ikkje starte planlegging: ${error instanceof Error ? error.message : "ukjend feil"}`);
        }
      }

      async function refreshProcess() {
        const id = processId.value.trim();
        if (!id) return;
        try {
          renderProcess(await processesApi.get(id));
        } catch (error) {
          pushSystem(`Kunne ikkje hente prosess: ${error instanceof Error ? error.message : "ukjend feil"}`);
        }
      }

      async function inspectProcess() {
        const id = processId.value.trim();
        if (!id) return;
        try {
          await processesApi.inspect(id, crypto.randomUUID());
          pushSystem("Heart-status er lagd i den varige køen. Oppdater status om litt.");
        } catch (error) {
          pushSystem(`Kunne ikkje hente Heart-status: ${error instanceof Error ? error.message : "ukjend feil"}`);
        }
      }

      async function answerProcess(answer: "yes" | "no"): Promise<void> {
        const id = processId.value.trim();
        if (!id) return;
        try {
          await processesApi.answer(id, crypto.randomUUID(), answer);
          pushSystem(`Svaret «${answer}» er lagd i den varige køen.`);
        } catch (error) {
          pushSystem(`Kunne ikkje svare på prosessen: ${error instanceof Error ? error.message : "ukjend feil"}`);
        }
      }

      function renderProcess(view: ProcessView): void {
        processView.replaceChildren();
        processView.hidden = false;
        const heading = document.createElement("strong");
        heading.textContent = `${view.process.definitionName}: ${view.process.status}`;
        processView.append(heading);
        for (const event of view.events) {
          const article = document.createElement("article");
          article.className = "process-event";
          const meta = document.createElement("span");
          meta.className = "meta";
          meta.textContent = `${event.eventType} · ${event.actorId}`;
          const payload = document.createElement("pre");
          payload.textContent = JSON.stringify(event.payload, null, 2);
          article.append(meta, payload);
          processView.append(article);
        }
      }

      function setRenderMode(mode: "view" | "raw"): void {
        renderMode = mode;
        const showsSource = mode === "raw";
        viewModeToggle.setAttribute("aria-checked", String(showsSource));
        const label = showsSource ? "Vis normalvising" : "Vis kjeldekode";
        viewModeToggle.setAttribute("aria-label", label);
        viewModeToggle.title = label;
        renderTimeline();
      }

      function renderKnownUsers() {
        const selectedUserId = directUser.value;
        directUser.replaceChildren(new Option("Vel brukar", ""));
        knownUsers.filter((user) => user.id !== currentParticipantId).forEach((user) => {
          const handle = mentionHandle(user);
          const status = [user.status_emoji, user.status_text].filter(Boolean).join(" ");
          const label = `${user.display_name} (@${handle})${status ? ` · ${status}` : ""}`;
          directUser.add(new Option(label, user.id));
        });
        if ([...directUser.options].some((option) => option.value === selectedUserId)) {
          directUser.value = selectedUserId;
        }
        refreshChannelMemberOptions(channelDetailsDialog.dataset.channelId);
        const own = knownUsers.find((user) => user.id === currentParticipantId);
        if (own) {
          if (!statusDraft.dirty) {
            statusDraft.emoji = own.status_emoji || "";
            statusDraft.text = own.status_text || "";
            statusEmoji.value = statusDraft.emoji;
            statusText.value = statusDraft.text;
          }
          document.querySelectorAll<HTMLElement>("#status-emoji-options [data-emoji]").forEach((button) => {
            button.setAttribute("aria-pressed", String(button.dataset.emoji === statusEmoji.value));
          });
          const statusLabel = own.status_text || own.status_emoji
            ? `${own.status_emoji || ""} ${own.status_text || ""}`.trim()
            : "Set status";
          currentStatusIcon.textContent = own.status_emoji || "🙂";
          currentStatusLabel.textContent = statusLabel;
          currentStatus.setAttribute("aria-label", statusLabel);
          currentStatus.title = statusLabel;
          currentStatus.dataset.tooltip = statusLabel;
        }
        openDirect.disabled = !directUser.value;
        if (directMessageDialog.open) {
          directMessageStatus.textContent = directUser.options.length > 1
            ? ""
            : "Ingen andre brukarar er registrerte enno.";
        }
      }

      function openDirectMessageDialog() {
        directUser.value = "";
        openDirect.disabled = true;
        directMessageStatus.textContent = "Hentar fersk personliste …";
        directMessageDialog.showModal();
        directUser.focus();
        if (!sendCommand("list_users")) {
          directMessageStatus.textContent = "Sprøyt er ikkje tilkopla. Vent litt og prøv igjen.";
        }
      }

      function activeProfile(userId: string | null | undefined): UserProfile | undefined {
        return knownUsers.find((user) => user.id === userId);
      }

      function directChannelLabel(channel: Channel | null | undefined): string {
        return activeProfile(channel?.direct_user_id)?.display_name || channel?.name || "Direktesamtale";
      }

      function profileStatus(profile: UserProfile | null | undefined): Readonly<{ symbol: string; text: string; label: string }> | null {
        if (!profile || (!profile.status_emoji && !profile.status_text)) return null;
        return {
          symbol: profile.status_emoji || "●",
          text: profile.status_text || "",
          label: [profile.status_emoji, profile.status_text].filter(Boolean).join(" ")
        };
      }

      function appendProfileStatus(target: HTMLElement, userId: string | null | undefined): void {
        const status = profileStatus(activeProfile(userId));
        if (!status) return;
        const indicator = document.createElement("span");
        indicator.className = "profile-status";
        indicator.textContent = status.symbol;
        indicator.title = status.label;
        indicator.setAttribute("aria-label", `Status: ${status.label}`);
        target.append(indicator);
      }

      function refreshVisibleProfileStatuses(userId: string | null = null): void {
        document.querySelectorAll<HTMLElement>("[data-profile-user-id]").forEach((target) => {
          if (userId && target.dataset.profileUserId !== userId) return;
          target.querySelector(".profile-status")?.remove();
          appendProfileStatus(target, target.dataset.profileUserId);
        });
      }

      function renderConversationIdentity() {
        const channel = knownChannels.find((item) => item.id === activeChannelId);
        conversationCircle.hidden = true;
        conversationCircle.textContent = "";
        conversationContext.hidden = true;
        conversationContext.replaceChildren();
        conversationPeerStatus.hidden = true;
        conversationPeerStatus.replaceChildren();
        channelPeopleButton.disabled = !channel;
        const channelUsers = channel ? knownChannelUsers.get(channel.id) : null;
        channelPeopleButton.textContent = channelUsers ? `👥 ${channelUsers.length}` : "👥";
        channelPeopleButton.setAttribute("aria-label", channelUsers
          ? `Vis dei ${channelUsers.length} menneska i kanalen`
          : "Vis menneska i kanalen");
        if (channel?.description) {
          renderMarkdown(channel.description, conversationContext);
          conversationContext.hidden = false;
        }
        if (!channel) return;
        conversationTitle.textContent = channel.name;
        conversationCircle.textContent = channel.circle_id
          ? (knownCircles.get(channel.circle_id)?.name || "Vennekrets")
          : (channel.direct_user_id ? "Direktemelding" : "Felles");
        conversationCircle.hidden = false;
        const peer = channel.direct_user_id ? activeProfile(channel.direct_user_id) : null;
        const status = profileStatus(peer);
        if (!peer || !status) return;
        conversationPeerStatus.hidden = false;
        const symbol = document.createElement("span");
        symbol.textContent = status.symbol;
        symbol.title = status.label;
        symbol.setAttribute("aria-label", `Status: ${status.label}`);
        conversationPeerStatus.append(symbol);
        if (status.text) conversationPeerStatus.append(document.createTextNode(` ${status.text}`));
      }

      function rerenderOpenChannelMembers(): void {
        const channelId = channelDetailsDialog.dataset.channelId;
        if (channelDetailsDialog.open && channelId) renderChannelMembers(channelId);
      }

      function failPendingPeopleDirectRequest(requestId: string, message: string): void {
        const userId = pendingPeopleDirectRequests.get(requestId);
        if (!userId) return;
        pendingPeopleDirectRequests.delete(requestId);
        peopleDirectStatuses.set(userId, message);
        rerenderOpenChannelMembers();
      }

      function failPendingPeopleDirectRequests(message: string): void {
        for (const requestId of [...pendingPeopleDirectRequests.keys()]) {
          failPendingPeopleDirectRequest(requestId, message);
        }
      }

      function openDirectFromChannelMember(profile: UserProfile): void {
        if ([...pendingPeopleDirectRequests.values()].some((userId) => userId === profile.id)) return;
        const requestId = sendCommand("open_direct_channel", { user_id: profile.id });
        if (!requestId) {
          peopleDirectStatuses.set(profile.id, "Ikkje tilkopla enno. Prøv igjen.");
          rerenderOpenChannelMembers();
          return;
        }
        pendingPeopleDirectRequests.set(requestId, profile.id);
        peopleDirectStatuses.set(profile.id, `Opnar samtale med ${profile.display_name} …`);
        rerenderOpenChannelMembers();
      }

      function renderChannelMembers(channelId: string): void {
        const users = knownChannelUsers.get(channelId) || [];
        const query = channelMemberSearch.value
          .normalize("NFKD")
          .replace(/[\u0300-\u036f]/g, "")
          .toLocaleLowerCase("nb-NO")
          .trim();
        // The signed-in person is implicit in a channel's member list.  The
        // browser is primarily a way to reach the other people here.
        const otherUsers = users.filter((profile) => profile.id !== currentParticipantId);
        const visibleUsers = query
          ? otherUsers.filter((profile) => profile.display_name
            .normalize("NFKD")
            .replace(/[\u0300-\u036f]/g, "")
            .toLocaleLowerCase("nb-NO")
            .includes(query))
          : otherUsers;
        if (channelId === activeChannelId) {
          channelPeopleButton.textContent = `👥 ${users.length}`;
          channelPeopleButton.setAttribute("aria-label", `Vis dei ${users.length} menneska i kanalen`);
        }
        channelMemberSearch.disabled = false;
        channelMemberCount.textContent = query
          ? `Viser ${visibleUsers.length} av ${otherUsers.length}`
          : `${otherUsers.length} andre menneske`;
        channelMemberList.replaceChildren();
        if (otherUsers.length === 0) {
          const empty = document.createElement("li");
          empty.textContent = "Ingen andre menneske i kanalen enno.";
          channelMemberList.append(empty);
          return;
        }
        if (visibleUsers.length === 0) {
          const empty = document.createElement("li");
          empty.textContent = "Ingen namn passar søket.";
          channelMemberList.append(empty);
          return;
        }
        visibleUsers.forEach((profile) => {
          const item = document.createElement("li");
          item.dataset.profileUserId = profile.id;
          const name = document.createElement("span");
          name.className = "channel-member-name";
          name.textContent = profile.display_name;
          item.append(name);
          appendProfileStatus(item, profile.id);
          const action = document.createElement("button");
          action.type = "button";
          action.className = "channel-member-direct";
          action.textContent = "💬";
          action.setAttribute("aria-label", `Start direktesamtale med ${profile.display_name}`);
          action.title = `Start direktesamtale med ${profile.display_name}`;
          action.disabled = [...pendingPeopleDirectRequests.values()].includes(profile.id);
          action.addEventListener("click", () => openDirectFromChannelMember(profile));
          item.append(action);
          const status = peopleDirectStatuses.get(profile.id);
          if (status) {
            item.classList.add("has-direct-status");
            const statusElement = document.createElement("span");
            statusElement.className = "channel-member-direct-status";
            statusElement.textContent = status;
            statusElement.setAttribute("role", "status");
            item.append(statusElement);
          }
          channelMemberList.append(item);
        });
        refreshChannelMemberOptions(channelId);
      }

      function refreshChannelMemberOptions(channelId: string | null | undefined): void {
        if (!channelId) return;
        const channel = knownChannels.find((item) => item.id === channelId);
        const memberIds = new Set((knownChannelUsers.get(channelId) || []).map((user) => user.id));
        const eligibleUsers = channel?.circle_id ? (knownCircleUsers.get(channel.circle_id) || []) : knownUsers;
        channelMember.replaceChildren(new Option("Vel brukar", ""));
        eligibleUsers
          .filter((user) => user.id !== currentParticipantId && !memberIds.has(user.id))
          .forEach((user) => {
            const status = [user.status_emoji, user.status_text].filter(Boolean).join(" ");
            channelMember.add(new Option(`${user.display_name}${status ? ` · ${status}` : ""}`, user.id));
          });
        updateOnboardingButtons();
      }

      function showChannelMemberLoadError(channelId: string, message: string): void {
        channelMemberSearch.disabled = false;
        channelMemberCount.textContent = "";
        channelMemberList.replaceChildren();
        const item = document.createElement("li");
        const text = document.createElement("span");
        text.textContent = message;
        const retry = document.createElement("button");
        retry.type = "button";
        retry.textContent = "Prøv igjen";
        retry.addEventListener("click", () => requestChannelMembers(channelId));
        item.append(text, retry);
        channelMemberList.append(item);
      }

      function requestChannelMembers(channelId: string): void {
        channelMemberSearch.disabled = true;
        channelMemberCount.textContent = "";
        channelMemberList.replaceChildren(Object.assign(document.createElement("li"), { textContent: "Lastar …" }));
        if (!sendCommand("list_channel_users", { channel_id: channelId })) {
          showChannelMemberLoadError(channelId, "Ikkje tilkopla enno. Vent litt og prøv igjen.");
        }
      }

      function openChannelDetails(editDescription = false) {
        const channel = knownChannels.find((item) => item.id === activeChannelId);
        if (!channel) return;
        channelDetailsDialog.dataset.channelId = channel.id;
        channelMemberSearch.value = "";
        channelMemberAdd.hidden = !["owner", "moderator"].includes(channel.role);
        channelMemberStatus.textContent = "";
        refreshChannelMemberOptions(channel.id);
        if (channel.circle_id) sendCommand("list_circle_users", { circle_id: channel.circle_id });
        channelDescriptionForm.hidden = channel.role !== "owner";
        channelDescriptionInput.value = channel.description || "";
        channelDescriptionStatus.textContent = "";
        channelDetailsDialog.showModal();
        requestChannelMembers(channel.id);
        if (editDescription && channel.role === "owner") channelDescriptionInput.focus();
      }

      function renderServerEvent(event: WireEvent) {
        if (event.protocol !== "sproyt.chat.v1") {
          pushSystem("Serveren svarte med ein ukjend protokoll.");
          return;
        }
        if (event.type === "pong") return;
        const requestedCommand = event.request_id ? pendingCommands.get(event.request_id) : undefined;
        const pendingInvitation = event.request_id ? pendingInvitationResponses.get(event.request_id) : undefined;
        const inspectedInvitationToken = event.request_id ? pendingInvitationInspections.get(event.request_id) : undefined;
        const invitationRecipient = event.request_id ? pendingChannelInvitationRecipients.get(event.request_id) : undefined;
        const directInvitationMessage = event.request_id ? pendingDirectInvitationMessages.get(event.request_id) : undefined;
        const directPersonUserId = event.request_id ? pendingPeopleDirectRequests.get(event.request_id) : undefined;
        if (event.request_id) pendingCommands.delete(event.request_id);
        if (event.request_id) pendingInvitationResponses.delete(event.request_id);
        if (event.request_id) pendingInvitationInspections.delete(event.request_id);
        if (event.request_id) pendingChannelInvitationRecipients.delete(event.request_id);
        if (event.request_id) pendingDirectInvitationMessages.delete(event.request_id);
        if (event.request_id) pendingPeopleDirectRequests.delete(event.request_id);

        if (event.type === "hello") {
          currentParticipantId = event.payload.participant_id;
          const ordinal = event.payload.signup_ordinal;
          signupBadge.hidden = ordinal === null;
          signupBadge.textContent = ordinal === null ? "" : `✨ Sprøyt #${ordinal}`;
          signupBadge.setAttribute("aria-label", ordinal === null ? "" : `Du var nummer ${ordinal} på Sprøyt`);
          signupBadge.title = ordinal === null ? "" : `Du var nummer ${ordinal} på Sprøyt`;
          return;
        }

        if (event.type === "users_listed") {
          knownUsers = event.payload.users;
          renderKnownUsers();
          if (knownChannels.length > 0) renderChannels();
          renderConversationIdentity();
          refreshVisibleProfileStatuses();
          updateMentionSuggestions();
          return;
        }

        if (event.type === "circle_users_listed") {
          knownCircleUsers.set(event.payload.circle_id, event.payload.users);
          const memberChannel = knownChannels.find((channel) => channel.id === channelDetailsDialog.dataset.channelId);
          if (channelDetailsDialog.open && memberChannel?.circle_id === event.payload.circle_id) {
            refreshChannelMemberOptions(memberChannel.id);
          }
          updateMentionSuggestions();
          return;
        }

        if (event.type === "channel_users_listed") {
          knownChannelUsers.set(event.payload.channel_id, event.payload.users);
          if (channelDetailsDialog.open && channelDetailsDialog.dataset.channelId === event.payload.channel_id) {
            renderChannelMembers(event.payload.channel_id);
          }
          return;
        }

        if (event.type === "channel_description_updated") {
          const channel = knownChannels.find((item) => item.id === event.payload.channel_id);
          if (channel) channel.description = event.payload.description;
          channelDescriptionStatus.textContent = "Omtalen er lagra.";
          renderConversationIdentity();
          return;
        }

        if (event.type === "status_updated") {
          knownUsers = [event.payload.profile, ...knownUsers.filter((user) => user.id !== event.payload.profile.id)];
          for (const [circleId, users] of knownCircleUsers) {
            if (users.some((user) => user.id === event.payload.profile.id)) {
              knownCircleUsers.set(circleId, [event.payload.profile, ...users.filter((user) => user.id !== event.payload.profile.id)]);
            }
          }
          for (const [channelId, users] of knownChannelUsers) {
            if (users.some((user) => user.id === event.payload.profile.id)) {
              knownChannelUsers.set(channelId, [event.payload.profile, ...users.filter((user) => user.id !== event.payload.profile.id)]);
            }
          }
          if (event.payload.profile.id === currentParticipantId) statusDraft.dirty = false;
          renderKnownUsers();
          renderConversationIdentity();
          refreshVisibleProfileStatuses(event.payload.profile.id);
          if (event.payload.profile.id === currentParticipantId) {
            const statusEditor = document.querySelector("#status-editor");
            if (statusEditor instanceof HTMLDetailsElement) statusEditor.open = false;
          }
          return;
        }

        if (event.type === "mentions_listed") {
          knownMentions = event.payload.mentions;
          renderPrimaryNavigation();
          renderMentionInbox();
          return;
        }

        if (event.type === "mention_read") {
          const mention = knownMentions.find((item) => item.message.id === event.payload.message_id);
          if (mention) mention.read = true;
          renderPrimaryNavigation();
          renderMentionInbox();
          return;
        }

        if (event.type === "tasks_listed") {
          knownTasks = event.payload.tasks;
          renderPrimaryNavigation();
          renderTaskInbox();
          return;
        }

        if (event.type === "task_created") {
          knownTasks = [event.payload.task, ...knownTasks.filter((task) => task.id !== event.payload.task.id)];
          showInbox("tasks");
          return;
        }

        if (event.type === "task_updated") {
          knownTasks = knownTasks.map((task) => task.id === event.payload.task.id ? event.payload.task : task);
          renderPrimaryNavigation();
          renderTaskInbox();
          return;
        }

        if (event.type === "circles_listed") {
          if (event.request_id !== latestCircleListRequestId) return;
          latestCircleListRequestId = null;
          knownCircles.clear();
          circleSelect.replaceChildren(new Option("Ingen", ""));
          event.payload.circles.forEach(([circle, role]) => {
            knownCircles.set(circle.id, { ...circle, role });
            circleSelect.add(new Option(`${circle.name} (${role})`, circle.id));
          });
          const restoredCircle = restoreActiveCircle();
          if (restoredCircle) sendCommand("list_joinable_channels", { circle_id: restoredCircle });
          updateOnboardingButtons();
          renderChannels();
          return;
        }
        if (event.type === "circle_created") {
          knownCircles.set(event.payload.circle.id, { ...event.payload.circle, role: "owner" });
          circleSelect.add(new Option(`${event.payload.circle.name} (owner)`, event.payload.circle.id));
          circleSelect.value = event.payload.circle.id;
          setActiveCircle(event.payload.circle.id);
          pushSystem(`Vennekretsen ${event.payload.circle.name} er oppretta.`);
          onboardingNotice.textContent = `${event.payload.circle.name} er klar. No kan du invitere vener.`;
          circleName.value = "";
          updateOnboardingButtons();
          sendCommand("create_channel", {
            slug: scopedCircleChannelSlug(event.payload.circle.id, "prat"), name: "Prat", kind: "private", circle_id: event.payload.circle.id
          });
          return;
        }
        if (event.type === "circle_deleted") {
          const deletedCircleId = event.payload.circle_id;
          forgetCircleChannel(deletedCircleId);
          const activeChannel = knownChannels.find((channel) => channel.id === activeChannelId);
          if (activeChannel?.circle_id === deletedCircleId) {
            navigation.clearActiveChannel(activeChannelId);
            syncRenderedNavigation();
            connectionSupervisor.clearSubscribedChannel();
          }
          clearActiveCircle(deletedCircleId);
          sendCommand("list_my_circles");
          sendCommand("list_my_channels");
          pushSystem("Vennekretsen og den tilhøyrande historikken er sletta.");
          return;
        }
        if (event.type === "circle_left") {
          const departedCircleId = event.payload.circle_id;
          forgetCircleChannel(departedCircleId);
          if (circleChannelDialog.open) circleChannelDialog.close();
          knownCircles.delete(departedCircleId);
          knownChannels = knownChannels.filter((channel) => channel.circle_id !== departedCircleId);
          clearActiveCircle(departedCircleId);
          if (activeChannelId && !knownChannels.some((channel) => channel.id === activeChannelId)) {
            navigation.clearActiveChannel(activeChannelId);
            syncRenderedNavigation();
            connectionSupervisor.clearSubscribedChannel();
          }
          onboardingNotice.textContent = "Du har forlate vennekretsen.";
          sendCommand("list_my_circles");
          sendCommand("list_my_channels");
          return;
        }
        if (event.type === "circle_invitation_created") {
          invitationToken.value = `${window.location.origin}/?invite=${encodeURIComponent(event.payload.invitation.token)}`;
          copyInvitation.hidden = false;
          onboardingNotice.textContent = "Invitasjonslenkja er klar. Kopier og send henne til venen din.";
          updateOnboardingButtons();
          return;
        }
        if (event.type === "invitation_created") {
          if (invitationRecipient) {
            const directRequestId = sendCommand("open_direct_channel", { user_id: invitationRecipient });
            if (directRequestId) {
              pendingDirectInvitationMessages.set(directRequestId, `[[invite:${event.payload.invitation.token}]]`);
              channelMemberStatus.textContent = "Opnar direktemeldinga …";
            }
            return;
          }
          invitationToken.value = `[[invite:${event.payload.invitation.token}]]`;
          copyInvitation.hidden = false;
          onboardingNotice.textContent = "Invitasjonsmeldinga er klar. Kopier henne inn i ein samtale.";
          updateOnboardingButtons();
          return;
        }
        if (event.type === "invitation_inspected" || event.type === "invitation_declined") {
          invitationInspectionCache.set(event.payload.token, { status: "resolved", invitation: { ...event.payload.invitation, response: event.payload.invitation.response ?? undefined } });
          updateInvitationCards(event.payload.token, event.payload.invitation);
          return;
        }
        if (event.type === "invitation_accepted") {
          markInvitationAccepted(event.payload.token);
          onboardingNotice.textContent = "Du er med i vennekretsen. Samtalane blir lasta inn no.";
          invitationToken.value = "";
          copyInvitation.hidden = true;
          sendCommand("list_my_circles");
          sendCommand("list_my_channels");
          pendingInvitationChannel = event.payload.invitation.channel.id;
          return;
        }
        if (event.type === "circle_invitation_accepted") {
          onboardingNotice.textContent = "Du er med i vennekretsen. Samtalane blir lasta inn no.";
          invitationToken.value = "";
          copyInvitation.hidden = true;
          const cleanUrl = new URL(window.location.href);
          cleanUrl.searchParams.delete("invite");
          window.history.replaceState({}, "", cleanUrl);
          sendCommand("list_my_circles");
          sendCommand("list_my_channels");
          return;
        }

        if (event.type === "channels_listed") {
          if (event.request_id !== latestChannelListRequestId) return;
          latestChannelListRequestId = null;
          knownChannels = event.payload.channels;
          renderChannels();
          renderConversationIdentity();
          updateAgentAccessControls();
          const requested = knownChannels.find((channel) => channel.slug === requestedChannelSlug);
          const current = knownChannels.find((channel) => channel.id === activeChannelId);
          const restored = knownChannels.find((channel) => channel.id === restoredChannelId);
          // Reconnects (including silent OIDC refresh) must keep the active
          // conversation. The requested slug is only a startup fallback.
          const invited = knownChannels.find((channel) => channel.id === pendingInvitationChannel);
          const next = invited || current || restored || requested || knownChannels[0];
          if (invited) pendingInvitationChannel = null;
          if (next && next.id !== activeChannelId) selectChannel(next);
          return;
        }

        if (event.type === "channel_created") {
          const channel = channelFromBase(event.payload.channel, "owner");
          knownChannels.push(channel);
          renderChannels();
          selectChannel(channel);
          managedChannelName.value = "";
          if (circleChannelDialog.open) circleChannelDialog.close();
          onboardingNotice.textContent = `Kanalen ${event.payload.channel.name} er klar.`;
          updateOnboardingButtons();
          if (circleSelect.value) sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
          return;
        }

        if (event.type === "joinable_channels_listed") {
          const channels = event.payload.channels.map((item) => channelFromBase(item.channel, "member", item.description));
          if (managedCircleId && channels.every((channel) => channel.circle_id === managedCircleId)) {
            renderManagedJoinableChannels(channels);
          }
          updateOnboardingButtons();
          return;
        }

        if (event.type === "membership_joined") {
          pendingInvitationChannel = event.payload.membership.channel_id;
          if (circleChannelDialog.open) circleChannelDialog.close();
          sendCommand("list_my_channels");
          if (circleSelect.value) sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
          return;
        }

        if (event.type === "membership_left") {
          if (event.payload.channel_id === activeChannelId) {
            navigation.clearActiveChannel(activeChannelId);
            syncRenderedNavigation();
            connectionSupervisor.clearSubscribedChannel();
          }
          onboardingNotice.textContent = "Du har forlate kanalen. Du kan bli med igjen dersom han er open.";
          sendCommand("list_my_channels");
          if (circleSelect.value) sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
          return;
        }

        if (event.type === "channel_member_added") {
          channelMemberStatus.textContent = "Brukaren er lagd til i kanalen.";
          channelMember.value = "";
          if (channelDetailsDialog.open && channelDetailsDialog.dataset.channelId === event.payload.membership.channel_id) {
            sendCommand("list_channel_users", { channel_id: event.payload.membership.channel_id });
          }
          updateOnboardingButtons();
          return;
        }

        if (event.type === "direct_channel_opened") {
          let channel = knownChannels.find((item) => item.id === event.payload.channel.id);
          if (!channel) {
            channel = channelFromBase(event.payload.channel, "member");
            knownChannels.push(channel);
          }
          renderChannels();
          if (directPersonUserId) {
            peopleDirectStatuses.delete(directPersonUserId);
            channelDetailsDialog.close();
          }
          directMessageStatus.textContent = "";
          directMessageDialog.close();
          selectChannel(channel);
          if (directInvitationMessage) {
            sendCommand("send_message", { channel_id: channel.id, body: directInvitationMessage });
            channelDetailsDialog.close();
          }
          sendCommand("list_my_channels");
          return;
        }

        if (event.type === "subscription_started") {
          if (event.payload.channel_id !== activeChannelId) {
            sendCommand("unsubscribe_channel", { channel_id: event.payload.channel_id });
            return;
          }
          connectionSupervisor.setSubscribedChannel(event.payload.channel_id);
          setConnectionStatus("Tilkopla");
          renderConversationIdentity();
          event.payload.history.forEach(appendTimelineMessage);
          reconcileUncertainDeliveries(event.payload.channel_id, event.payload.history);
          sendCommand("list_thread_summaries", { channel_id: event.payload.channel_id });
          historyHasMore = event.payload.history.length === historyPageSize;
          historyLoading = false;
          acknowledgeLatest(event.payload.channel_id, event.payload.history);
          bodyInput.disabled = false;
          sendButton.disabled = false;
          attachMediaButton.disabled = false;
          messageEmojiPicker.setAttribute("aria-disabled", "false");
          syncComposerState();
          renderChannels();
          const scrollOffset = reconnectScrollOffset;
          reconnectScrollOffset = null;
          renderTimeline({ forceBottom: scrollOffset === null || scrollOffset < 80 });
          if (pendingThreadToOpen) {
            const rootMessageId = pendingThreadToOpen;
            pendingThreadToOpen = null;
            window.setTimeout(() => openThread(rootMessageId), 0);
          }
          if (scrollOffset !== null && scrollOffset >= 80) restoreConversationScrollOffset(scrollOffset);
          updateAgentAccessControls();
          return;
        }

        if (event.type === "subscription_ended") {
          if (event.payload.channel_id === connectionSupervisor.snapshot().subscribedChannelId) {
            connectionSupervisor.clearSubscribedChannel(event.payload.channel_id);
            setConnected(connectionSupervisor.snapshot().connected, "Koplar til samtalen …");
          }
          return;
        }

        if (event.type === "channel_reactions_listed") {
          if (event.payload.channel_id === activeChannelId) {
            replaceChannelReactions(event.payload.reactions);
            renderTimeline({ preserveScroll: true });
          }
          return;
        }

        if (event.type === "message_reaction_changed") {
          if (event.payload.change.channel_id === activeChannelId) {
            applyReactionChange(event.payload.change);
            if (!patchMessageReactions(event.payload.change.message_id)) {
              renderTimeline({ preserveScroll: true });
            }
          }
          return;
        }

        if (event.type === "thread_summaries_listed") {
          if (event.payload.channel_id !== activeChannelId) return;
          threadSummaries.clear();
          for (const summary of event.payload.summaries) threadSummaries.set(summary.root_message_id, summary);
          renderTimeline({ preserveScroll: true });
          return;
        }

        if (event.type === "thread_loaded") {
          const root = event.payload.messages.find((message) => message.id === event.payload.root_message_id);
          const replies = event.payload.messages.filter((message) => message.parent_message_id === event.payload.root_message_id);
          if (root) threadRoots.set(event.payload.root_message_id, root);
          threadReplies.set(event.payload.root_message_id, replies);
          const threadChannelId = root?.channel_id ?? replies[0]?.channel_id;
          if (threadChannelId) reconcileUncertainDeliveries(threadChannelId, event.payload.messages);
          if (activeThreadRootId === event.payload.root_message_id) {
            renderThread();
            const latest = replies.at(-1)?.sequence;
            if (latest !== undefined) sendCommand("mark_thread_read", { root_message_id: event.payload.root_message_id, sequence: latest });
          }
          return;
        }

        if (event.type === "thread_read_updated") {
          threadSummaries.set(event.payload.summary.root_message_id, event.payload.summary);
          renderTimeline({ preserveScroll: true });
          return;
        }

        if (event.type === "chat") {
          const chatEvent = event.payload.event;
          if (chatEvent.type === "message_accepted") {
            updateLatestSequence(chatEvent.message.channel_id, chatEvent.message.sequence);
            if (chatEvent.message.channel_id === activeChannelId) {
              const revealOwnMessage = pendingMessageToReveal(chatEvent.message);
              appendTimelineMessage(chatEvent.message);
              acknowledgeLatest(chatEvent.message.channel_id, [chatEvent.message]);
              renderTimeline({ revealMessageId: revealOwnMessage ? chatEvent.message.id : null });
            } else {
              renderChannels();
            }
          } else if (chatEvent.type === "message_edited") {
            if (chatEvent.message.channel_id === activeChannelId) {
              replaceTimelineMessage(chatEvent.message);
              renderTimeline({ preserveScroll: true });
            }
          } else if (chatEvent.type === "message_deleted") {
            messageReactions.delete(chatEvent.message.id);
            if (chatEvent.message.channel_id === activeChannelId) {
              replaceTimelineMessage(chatEvent.message);
              renderTimeline({ preserveScroll: true });
            }
          } else if (chatEvent.type === "message_reaction_changed") {
            if (chatEvent.change.channel_id === activeChannelId) {
              applyReactionChange(chatEvent.change);
              if (!patchMessageReactions(chatEvent.change.message_id)) {
                renderTimeline({ preserveScroll: true });
              }
            }
          }
          return;
        }

        if (event.type === "message_accepted") {
          updateLatestSequence(event.payload.message.channel_id, event.payload.message.sequence);
          if (event.payload.message.channel_id === activeChannelId) {
            const revealOwnMessage = pendingMessageToReveal(event.payload.message, event.request_id);
            appendTimelineMessage(event.payload.message);
            acknowledgeLatest(event.payload.message.channel_id, [event.payload.message]);
              renderTimeline({ revealMessageId: revealOwnMessage ? event.payload.message.id : null });
          } else {
            renderChannels();
          }
          finishPendingMessage(event.request_id, event.payload.message);
          finishPendingThreadReply(event.request_id, event.payload.message);
          return;
        }

        if (event.type === "message_edited") {
          if (event.payload.message.channel_id === activeChannelId) {
            replaceTimelineMessage(event.payload.message);
            renderTimeline({ preserveScroll: true });
          }
          return;
        }

        if (event.type === "message_deleted") {
          messageReactions.delete(event.payload.message.id);
          if (event.payload.message.channel_id === activeChannelId) {
            replaceTimelineMessage(event.payload.message);
            renderTimeline({ preserveScroll: true });
          }
          return;
        }

        if (event.type === "lagged") {
          pushSystem(`Klienten låg etter og hoppa over ${event.payload.skipped} event; lastar inn att.`);
          catchUpTargets.set(event.payload.channel_id, event.payload.latest_known_sequence);
          sendCommand("load_recent_messages", {
            channel_id: event.payload.channel_id,
            after: event.payload.last_seen_sequence,
            limit: 200
          });
          return;
        }

        if (event.type === "messages_loaded") {
          const olderHistory = historyRequestIds.delete(event.request_id);
          if (olderHistory) {
            historyLoading = false;
            if (event.payload.channel_id !== activeChannelId) return;
            historyHasMore = event.payload.messages.length === historyPageSize;
            prependTimelineMessages(event.payload.messages);
            renderTimeline({ preserveScroll: true });
            return;
          }
          event.payload.messages.forEach(appendTimelineMessage);
          reconcileUncertainDeliveries(event.payload.channel_id, event.payload.messages);
          acknowledgeLatest(event.payload.channel_id, event.payload.messages);
          renderTimeline();
          const target = catchUpTargets.get(event.payload.channel_id);
          const last = event.payload.messages.at(-1);
          if (target !== undefined && last && last.sequence < target) {
            sendCommand("load_recent_messages", {
              channel_id: event.payload.channel_id,
              after: last.sequence,
              limit: 200
            });
          } else if (target !== undefined) {
            catchUpTargets.delete(event.payload.channel_id);
          }
          return;
        }

        if (event.type === "read_marker_updated") {
          const channel = knownChannels.find((item) => item.id === event.payload.membership.channel_id);
          if (channel) channel.last_read_sequence = event.payload.membership.last_read_sequence;
          renderChannels();
          return;
        }

        if (event.type === "error") {
          const failedHistory = historyRequestIds.delete(event.request_id);
          if (failedHistory) {
            historyLoading = false;
            historyHasMore = false;
            console.error("Kunne ikkje laste eldre meldingar", {
              requestId: event.request_id,
              command: requestedCommand,
              code: event.payload.code,
              message: event.payload.message,
              channelId: activeChannelId
            });
            setConnectionStatus("Kunne ikkje laste eldre meldingar. Nyare meldingar er framleis tilgjengelege.");
            return;
          }
          if (requestedCommand === "send_message") {
            const message = event.payload.message || event.payload.code || "ukjend feil";
            if (!failPendingThreadReply(event.request_id, message)) {
              failPendingMessage(event.request_id, message);
            }
            pushSystem(event.payload.message || event.payload.code);
            return;
          }
          if (requestedCommand === "accept_invitation") {
            const message = event.payload.code === "not_found"
              ? "Invitasjonen finst ikkje eller er ikkje gyldig lenger. Be venen din lage ei ny lenkje."
              : event.payload.code === "permission_denied"
                ? "Du må først vere medlem i vennekretsen før du kan bli med i denne kanalen."
                : "Du kunne ikkje bli med med denne invitasjonen. Kontroller lenkja eller be om ei ny.";
            onboardingNotice.textContent = message;
            if (pendingInvitation) showInvitationError(pendingInvitation.token, message);
            return;
          }
          if (requestedCommand === "decline_invitation") {
            const message = "Invitasjonen kunne ikkje avvisast. Prøv igjen.";
            if (pendingInvitation) showInvitationError(pendingInvitation.token, message);
            return;
          }
          if (requestedCommand === "inspect_invitation") {
            const message = event.payload.code === "not_found"
              ? "Invitasjonen finst ikkje eller er ikkje gyldig lenger."
              : "Invitasjonen kunne ikkje hentast no.";
            if (inspectedInvitationToken) {
              invitationInspectionCache.set(inspectedInvitationToken, {
                status: event.payload.code === "not_found" ? "missing" : "failed",
                message
              });
              showInvitationError(inspectedInvitationToken, message);
            }
            return;
          }
          if (requestedCommand === "create_circle") {
            onboardingNotice.textContent = "Vennekretsen kunne ikkje opprettast. Prøv eit anna namn.";
            return;
          }
          if (requestedCommand === "create_channel") {
            onboardingNotice.textContent = "Kanalen kunne ikkje opprettast. Prøv eit anna namn eller prøv igjen.";
            if (circleChannelDialog.open) circleMembershipNotice.textContent = "Kanalen kunne ikkje opprettast. Prøv eit anna namn.";
            updateOnboardingButtons();
            return;
          }
          if (requestedCommand === "update_channel_description") {
            channelDescriptionStatus.textContent = event.payload.code === "permission_denied"
              ? "Berre eigaren kan endre kanalomtalen."
              : "Kanalomtalen kunne ikkje lagrast. Prøv igjen.";
            return;
          }
          if (requestedCommand === "list_channel_users") {
            const channelId = channelDetailsDialog.dataset.channelId;
            if (channelId) showChannelMemberLoadError(channelId, "Medlemslista kunne ikkje lastast.");
            return;
          }
          if (requestedCommand === "add_channel_member") {
            channelMemberStatus.textContent = event.payload.code === "permission_denied"
              ? "Brukaren må vere medlem av kretsen, og du må ha tilgang til å leggje til medlemmer."
              : "Brukaren kunne ikkje leggjast til. Prøv igjen.";
            return;
          }
          if (requestedCommand === "create_invitation" && invitationRecipient) {
            channelMemberStatus.textContent = "Invitasjonen kunne ikkje lagast. Prøv igjen.";
            return;
          }
          if (requestedCommand === "open_direct_channel") {
            if (directInvitationMessage) {
              channelMemberStatus.textContent = "Direktemeldinga kunne ikkje opnast. Prøv igjen.";
            } else if (directPersonUserId) {
              peopleDirectStatuses.set(directPersonUserId, event.payload.code === "not_found"
                ? "Brukaren finst ikkje lenger. Oppdater lista og prøv igjen."
                : event.payload.code === "conflict"
                  ? "Samtalen kunne ikkje opnast. Prøv igjen."
                  : "Kunne ikkje opne samtalen. Prøv igjen.");
              rerenderOpenChannelMembers();
            } else {
              directMessageStatus.textContent = event.payload.code === "not_found"
                ? "Brukaren finst ikkje lenger. Lukk dialogen og prøv på nytt."
                : event.payload.code === "conflict"
                  ? "Du kan ikkje starte ei direktesamtale med deg sjølv."
                  : "Samtalen kunne ikkje opnast. Prøv igjen.";
              openDirect.disabled = !directUser.value;
            }
            return;
          }
          if (requestedCommand === "leave_circle") {
            circleMembershipNotice.textContent = "Vennekretsen kunne ikkje forlatast. Eigaren må slette kretsen i administrasjon.";
            return;
          }
          if (requestedCommand === "leave_channel") {
            onboardingNotice.textContent = event.payload.code === "permission_denied"
              ? "Standardkanalen Prat kan ikkje forlatast."
              : "Kanalen kunne ikkje forlatast. Prøv igjen.";
            return;
          }
          console.error("Sprøyt-kommando feila", {
            requestId: event.request_id,
            command: requestedCommand || "ukjend",
            code: event.payload.code,
            message: event.payload.message,
            channelId: activeChannelId
          });
          const passiveCommands = new Set([
            "list_channel_reactions", "list_thread_summaries", "mark_read",
            "list_users", "list_my_channels", "list_my_circles", "list_mentions", "list_tasks"
          ]);
          if (passiveCommands.has(requestedCommand ?? "")) {
            setConnectionStatus(`Kunne ikkje oppdatere samtalen (${requestedCommand || "ukjend"}).`);
            return;
          }
          pushSystem(`${requestedCommand || "Kommando"}: ${event.payload.message || event.payload.code}`);
        }
      }

      function renderChannels() {
        renderPrimaryNavigation();
        renderBottomNavigation();
      }

      function renderBottomNavigation() {
        bottomChannelList.replaceChildren();
        bottomCircleContent.replaceChildren();
        const activeCircle = activeCircleId ? knownCircles.get(activeCircleId) : undefined;
        const activeChannel = knownChannels.find((channel) => channel.id === activeChannelId);
        const sharedChannels = knownChannels.filter((channel) => !channel.circle_id && !channel.direct_user_id);
        const directChannels = knownChannels.filter((channel) => channel.direct_user_id);
        const showingShared = !activeCircle && activeRootScope === "shared";
        const showingDirect = !activeCircle && activeRootScope === "direct";
        const activeChannelInScope = activeChannel && (activeCircle
          ? activeChannel.circle_id === activeCircleId
          : (showingDirect ? Boolean(activeChannel.direct_user_id) : (!activeChannel.circle_id && !activeChannel.direct_user_id)));
        const channelLabel = activeChannelInScope
          ? (activeChannel.direct_user_id ? directChannelLabel(activeChannel) : `# ${activeChannel.name}`)
          : (showingDirect ? "Direktesamtalar" : "# Kanal");
        const circleLabel = activeCircle?.name || (showingDirect ? "Direkte" : "Felles");
        const bottomChannelLabel = bottomChannelToggle.querySelector(".bottom-navigation-label");
        if (!(bottomChannelLabel instanceof HTMLElement)) throw new Error("Manglar kanalmerke i botnnavigasjonen");
        bottomChannelLabel.textContent = channelLabel;
        bottomChannelToggle.setAttribute("aria-label", `Vel kanal. Aktiv kanal: ${activeChannelInScope ? (activeChannel.direct_user_id ? directChannelLabel(activeChannel) : activeChannel.name) : "ingen"}`);
        const bottomCircleLabel = bottomCircleToggle.querySelector(".bottom-navigation-label");
        if (!(bottomCircleLabel instanceof HTMLElement)) throw new Error("Manglar områdemerke i botnnavigasjonen");
        bottomCircleLabel.textContent = `◎ ${circleLabel}`;
        bottomCircleToggle.setAttribute("aria-label", `Vel område. Aktivt område: ${circleLabel}`);

        const sharedUnreadCount = sharedChannels.reduce(
          (total, channel) => total + Math.max(0, channel.latest_sequence - channel.last_read_sequence),
          0
        );
        const directUnreadCount = directChannels.reduce(
          (total, channel) => total + Math.max(0, channel.latest_sequence - channel.last_read_sequence),
          0
        );
        updateCircleToolButtons(sharedUnreadCount, directUnreadCount);
        if (knownCircles.size === 0) {
          const empty = document.createElement("p");
          empty.className = "status";
          empty.textContent = "Ingen vennekretsar enno";
          bottomCircleContent.append(empty);
        } else {
          for (const [circleId, circle] of knownCircles) {
            const channels = knownChannels.filter((channel) => channel.circle_id === circleId);
            const unreadCount = channels.reduce(
              (total, channel) => total + Math.max(0, channel.latest_sequence - channel.last_read_sequence),
              0
            );
            const button = document.createElement("button");
            button.type = "button";
            button.textContent = circle.name;
            button.setAttribute("aria-current", circleId === activeCircleId ? "page" : "false");
            if (unreadCount > 0) {
              button.classList.add("has-unread");
              const unread = document.createElement("span");
              unread.className = "unread";
              unread.textContent = approximateUnreadCount(unreadCount);
              unread.setAttribute("aria-label", `${unreadCount} uleste meldingar i ${circle.name}`);
              button.append(unread);
            }
            button.addEventListener("click", () => {
              setActiveCircle(circleId);
              circleSelect.value = circleId;
              sendCommand("list_joinable_channels", { circle_id: circleId });
              closeBottomNavigation(bottomCirclePanel, bottomCircleToggle);
              const preferredChannel = preferredCircleChannel(circleId, channels);
              if (preferredChannel) selectChannel(preferredChannel);
              else renderChannels();
            });
            bottomCircleContent.append(button);
          }
        }

        const scopedChannels = activeCircleId
          ? knownChannels.filter((channel) => channel.circle_id === activeCircleId)
          : (showingDirect ? directChannels : sharedChannels);
        const emptyText = showingDirect
          ? "Ingen direktesamtalar enno."
          : (activeCircleId ? "Ingen kanalar i den valde kretsen." : "Ingen felleskanalar enno.");
        appendBottomChannelButtons(scopedChannels, bottomChannelList, emptyText);
        const selectedCircleId = activeCircleId;
        if (selectedCircleId) {
          const discover = document.createElement("button");
          discover.type = "button";
          discover.className = "channel-group-action";
          discover.textContent = "+ Finn fleire kanalar";
          discover.addEventListener("click", () => {
            closeBottomNavigation(bottomChannelPanel, bottomChannelToggle);
            openChannelManagement(selectedCircleId);
          });
          bottomChannelList.append(discover);
          if (activeChannel?.circle_id === activeCircleId && activeChannel.name.trim().toLocaleLowerCase() !== "prat") {
            const leave = document.createElement("button");
            leave.type = "button";
            leave.className = "channel-group-action danger-button";
            leave.textContent = `Forlat # ${activeChannel.name}`;
            leave.addEventListener("click", () => {
              closeBottomNavigation(bottomChannelPanel, bottomChannelToggle);
              if (!window.confirm(`Vil du forlate kanalen ${activeChannel.name}? Du kan bli med igjen seinare dersom kanalen er open.`)) return;
              sendCommand("leave_channel", { channel_id: activeChannel.id });
            });
            bottomChannelList.append(leave);
          }
        } else if (showingDirect) {
          const startDirect = document.createElement("button");
          startDirect.type = "button";
          startDirect.className = "channel-group-action";
          startDirect.textContent = "+ Ny samtale …";
          startDirect.addEventListener("click", () => {
            closeBottomNavigation(bottomChannelPanel, bottomChannelToggle);
            openDirectMessageDialog();
          });
          bottomChannelList.append(startDirect);
        }
      }

      function activateRootScope(scope: "shared" | "direct"): void {
        const channels = knownChannels.filter((channel) => scope === "direct"
          ? Boolean(channel.direct_user_id)
          : (!channel.circle_id && !channel.direct_user_id));
        navigation.activateRootScope(scope);
        syncRenderedNavigation();
        circleSelect.value = "";
        closeBottomNavigation(bottomCirclePanel, bottomCircleToggle);
        const current = channels.find((channel) => channel.id === activeChannelId);
        if (current) renderChannels();
        else if (channels[0]) selectChannel(channels[0]);
        else renderChannels();
      }

      function updateCircleToolButtons(sharedUnreadCount: number, directUnreadCount: number): void {
        circleToolDirect.setAttribute("aria-pressed", String(!activeCircleId && activeRootScope === "direct"));
        circleToolShared.setAttribute("aria-pressed", String(!activeCircleId && activeRootScope === "shared"));
        circleToolDirect.classList.toggle("has-unread", directUnreadCount > 0);
        circleToolDirect.setAttribute("aria-label", directUnreadCount > 0
          ? `Direkte samtalar, ${directUnreadCount} uleste meldingar`
          : "Direkte samtalar");
        circleToolShared.classList.toggle("has-unread", sharedUnreadCount > 0);
        circleToolShared.setAttribute("aria-label", sharedUnreadCount > 0
          ? `Felles, ${sharedUnreadCount} uleste meldingar`
          : "Felles");
      }

      function appendBottomChannelButtons(channels: Channel[], target: HTMLElement, emptyText = "", panel: HTMLDetailsElement = bottomChannelPanel, toggle: HTMLElement = bottomChannelToggle): void {
        if (channels.length === 0 && emptyText) {
          const empty = document.createElement("p");
          empty.className = "status";
          empty.textContent = emptyText;
          target.append(empty);
          return;
        }
        for (const channel of channels) {
          const unreadCount = Math.max(0, channel.latest_sequence - channel.last_read_sequence);
          const button = document.createElement("button");
          button.type = "button";
          button.textContent = channel.direct_user_id ? directChannelLabel(channel) : `# ${channel.name}`;
          button.setAttribute("aria-current", channel.id === activeChannelId ? "page" : "false");
          if (unreadCount > 0) {
            button.classList.add("has-unread");
            const unread = document.createElement("span");
            unread.className = "unread";
            unread.textContent = approximateUnreadCount(unreadCount);
            unread.setAttribute("aria-label", `${unreadCount} uleste meldingar`);
            button.append(unread);
          }
          button.addEventListener("click", () => {
            closeBottomNavigation(panel, toggle);
            selectChannel(channel);
          });
          target.append(button);
        }
      }

      function closeBottomNavigation(panel: HTMLDetailsElement, toggle: HTMLElement): void {
        panel.open = false;
        toggle.focus();
      }

      function openChannelManagement(circleId: string): void {
        const circle = knownCircles.get(circleId);
        if (!circle) return;
        managedCircleId = circleId;
        circleSelect.value = circleId;
        setActiveCircle(circleId);
        updateOnboardingButtons();
        circleChannelTitle.textContent = `Kanalar i ${circle.name}`;
        circleJoinableList.innerHTML = '<p class="status">Lastar tilgjengelege kanalar …</p>';
        const owner = circle.role === "owner";
        leaveCircleButton.hidden = owner;
        circleMembershipNotice.textContent = owner
          ? "Eigaren kan ikkje forlate kretsen. Kretsen kan slettast frå administrasjon."
          : "Du mistar tilgang til kanalane i kretsen, men meldingane dine blir ståande.";
        if (!circleChannelDialog.open) circleChannelDialog.showModal();
        sendCommand("list_joinable_channels", { circle_id: circleId });
        window.setTimeout(() => circleChannelClose.focus(), 0);
      }

      function renderManagedJoinableChannels(channels: Channel[]): void {
        circleJoinableList.replaceChildren();
        if (channels.length === 0) {
          const empty = document.createElement("p");
          empty.className = "status";
          empty.textContent = "Ingen fleire opne kanalar akkurat no.";
          circleJoinableList.append(empty);
          return;
        }
        for (const channel of channels) {
          const card = document.createElement("article");
          card.className = "joinable-channel-card";
          const join = document.createElement("button");
          join.type = "button";
          join.textContent = "Bli med";
          join.setAttribute("aria-label", `Bli med i ${channel.name}`);
          const name = document.createElement("strong");
          name.textContent = `# ${channel.name}`;
          const description = document.createElement("div");
          description.className = "joinable-channel-description";
          if (channel.description) renderMarkdown(channel.description, description);
          else description.textContent = "Ingen kanalomtale enno.";
          join.addEventListener("click", () => sendCommand("join_channel", {
            channel: { type: "id", value: channel.id }
          }));
          card.append(name, description, join);
          circleJoinableList.append(card);
        }
      }

      function updateNavigationCount(id: string, count: number, label: string): void {
        const badge = requireElement(`#${id}`, HTMLElement);
        const button = badge.closest("button");
        if (!(button instanceof HTMLButtonElement)) return;
        badge.hidden = count === 0;
        badge.textContent = count === 0 ? "" : approximateUnreadCount(count);
        badge.setAttribute("aria-label", `${count} ${label}`);
        const navigationLabel = button.dataset.navigationLabel || button.textContent || "Navigasjon";
        const buttonLabel = count === 0 ? navigationLabel : `${navigationLabel}: ${count} ${label}`;
        button.setAttribute("aria-label", buttonLabel);
        button.title = buttonLabel;
        button.dataset.tooltip = buttonLabel;
        button.classList.toggle("has-unread", count > 0);
      }

      function renderPrimaryNavigation() {
        const unreadCount = knownChannels.reduce(
          (total, channel) => total + Math.max(0, channel.latest_sequence - channel.last_read_sequence),
          0
        );
        updateNavigationCount("unread-count", unreadCount, "uleste meldingar");
        updateNavigationCount("mention-count", knownMentions.filter((mention) => !mention.read).length, "uleste omtalar");
        updateNavigationCount("task-count", knownTasks.filter((task) => task.status !== "done").length, "opne oppgåver");
        const inboxKinds: Array<"unread" | "mentions" | "tasks"> = ["unread", "mentions", "tasks"];
        for (const kind of inboxKinds) {
          const navigationButton = document.querySelector(`#show-${kind}`);
          if (navigationButton instanceof HTMLButtonElement) navigationButton.setAttribute("aria-current", activeInboxKind === kind ? "page" : "false");
        }
      }

      function approximateUnreadCount(count: number): string {
        if (count < 25) return String(count);
        if (count < 50) return "25+";
        if (count < 100) return "50+";
        return "100+";
      }

      function showInbox(kind: "unread" | "mentions" | "tasks"): void {
        const subscribedChannelId = connectionSupervisor.takeSubscribedChannel();
        if (subscribedChannelId) {
          sendCommand("unsubscribe_channel", {
            channel_id: subscribedChannelId
          });
        }
        navigation.deactivateChannel();
        syncRenderedNavigation();
        activeInboxKind = kind;
        setMobileNavigationOpen(false);
        reconnectScrollOffset = null;
        timeline.length = 0;
        threadReplies.clear();
        threadRoots.clear();
        threadSummaries.clear();
        if (threadPanel.open) threadPanel.close();
        messageReactions.clear();
        seenMessageIds.clear();
        historyRequestIds.clear();
        historyHasMore = false;
        historyLoading = false;
        bodyInput.disabled = true;
        sendButton.disabled = true;
        attachMediaButton.disabled = true;
        messageEmojiPicker.setAttribute("aria-disabled", "true");
        syncComposerState();
        messagesEl.replaceChildren();
        renderConversationIdentity();
        conversationCircle.textContent = "For deg";
        conversationCircle.hidden = false;
        if (kind === "unread") {
          conversationTitle.textContent = "Uleste meldingar";
          const unread = knownChannels
            .filter((channel) => channel.latest_sequence > channel.last_read_sequence)
            .sort((left, right) => (right.latest_sequence - right.last_read_sequence) - (left.latest_sequence - left.last_read_sequence));
          if (unread.length === 0) {
            messagesEl.innerHTML = '<div class="empty-state"><h2>Alt er lese</h2><p>Du har ingen uleste meldingar akkurat no.</p></div>';
          } else {
            const inbox = document.createElement("section");
            inbox.className = "unread-inbox";
            const summary = document.createElement("p");
            summary.className = "unread-inbox-summary";
            const total = unread.reduce((count, channel) => count + channel.latest_sequence - channel.last_read_sequence, 0);
            summary.textContent = `${total} uleste meldingar i ${unread.length} ${unread.length === 1 ? "samtale" : "samtalar"}`;
            inbox.append(summary);
            for (const channel of unread) {
              const button = document.createElement("button");
              button.type = "button";
              button.className = "unread-card";
              const identity = document.createElement("span");
              const name = document.createElement("strong");
              name.textContent = channel.direct_user_id ? directChannelLabel(channel) : `# ${channel.name}`;
              const context = document.createElement("small");
              context.textContent = channel.circle_id
                ? (knownCircles.get(channel.circle_id)?.name || "Vennekrets")
                : (channel.direct_user_id ? "Direktemelding" : "Felles");
              const count = document.createElement("span");
              count.className = "unread";
              const unreadCount = channel.latest_sequence - channel.last_read_sequence;
              count.textContent = approximateUnreadCount(unreadCount);
              count.setAttribute("aria-label", `${unreadCount} uleste meldingar`);
              identity.append(name, context);
              button.append(identity, count);
              button.addEventListener("click", () => selectChannel(channel));
              inbox.append(button);
            }
            messagesEl.append(inbox);
          }
        } else if (kind === "mentions") {
          conversationTitle.textContent = "Omtalar";
          messagesEl.innerHTML = '<div class="empty-state"><h2>Lastar omtalar …</h2></div>';
          sendCommand("list_mentions");
        } else {
          conversationTitle.textContent = "Oppgåver";
          messagesEl.innerHTML = '<div class="empty-state"><h2>Lastar oppgåver …</h2></div>';
          sendCommand("list_tasks");
        }
        renderChannels();
      }

      function renderMentionInbox() {
        if (conversationTitle.textContent !== "Omtalar") return;
        messagesEl.replaceChildren();
        if (knownMentions.length === 0) {
          messagesEl.innerHTML = '<div class="empty-state"><h2>Ingen omtalar</h2><p>Når nokon skriv @namnet-ditt, kjem meldinga hit.</p></div>';
          return;
        }
        for (const mention of knownMentions) {
          const card = document.createElement("article");
          card.className = "message";
          const heading = document.createElement("strong");
          heading.textContent = `${mention.message.sender_display_name} i ${mention.channel_name}`;
          const body = document.createElement("p");
          body.textContent = mention.message.body;
          const actions = document.createElement("div");
          actions.className = "onboarding-actions";
          const open = document.createElement("button");
          open.type = "button";
          open.textContent = "Opne samtalen";
          open.addEventListener("click", () => {
            const channel = knownChannels.find((item) => item.id === mention.message.channel_id);
            if (channel) {
              pendingThreadToOpen = mention.message.parent_message_id || null;
              selectChannel(channel);
            }
          });
          actions.append(open);
          if (!mention.read) {
            const read = document.createElement("button");
            read.type = "button";
            read.textContent = "Marker lesen";
            read.addEventListener("click", () => sendCommand("mark_mention_read", { message_id: mention.message.id }));
            actions.append(read);
          }
          const task = document.createElement("button");
          task.type = "button";
          task.textContent = "Lag oppgåve";
          task.addEventListener("click", () => createTaskFromMention(mention, card));
          actions.append(task);
          card.append(heading, body, actions);
          if (!mention.read) card.dataset.unread = "true";
          messagesEl.append(card);
        }
      }

      function createTaskFromMention(mention: Mention, card: HTMLElement): void {
        if (card.querySelector(".task-editor")) return;
        const editor = document.createElement("form");
        editor.className = "task-editor";
        const title = document.createElement("input");
        title.required = true;
        title.maxLength = 240;
        title.setAttribute("aria-label", "Oppgåvetittel");
        title.value = mention.message.body.replace(/@\S+/g, "").trim();
        const process = document.createElement("input");
        process.setAttribute("aria-label", "Heart-prosess-ID");
        process.placeholder = "Heart-prosess-ID (valfritt)";
        const save = document.createElement("button");
        save.type = "submit";
        save.textContent = "Lagre oppgåve";
        editor.append(title, process, save);
        editor.addEventListener("submit", (event) => {
          event.preventDefault();
          if (!title.value.trim()) return;
          if (!currentParticipantId) return;
          sendCommand("create_task", {
            source_message_id: mention.message.id,
            assignee_id: currentParticipantId,
            title: title.value.trim(),
            process_link_id: process.value.trim() || null
          });
        });
        card.append(editor);
        title.focus();
      }

      function renderTaskInbox() {
        if (conversationTitle.textContent !== "Oppgåver") return;
        messagesEl.replaceChildren();
        if (knownTasks.length === 0) {
          messagesEl.innerHTML = '<div class="empty-state"><h2>Ingen oppgåver</h2><p>Du kan gjere ei @omtale om til ei oppgåve.</p></div>';
          return;
        }
        for (const task of knownTasks) {
          const card = document.createElement("article");
          card.className = "message";
          const heading = document.createElement("strong");
          heading.textContent = task.title;
          const details = document.createElement("p");
          details.textContent = `${task.channel_name}${task.process_link_id ? ` · Heart ${task.process_link_id}` : ""}`;
          const toggle = document.createElement("button");
          toggle.type = "button";
          toggle.textContent = task.status === "done" ? "Opne igjen" : "Ferdig";
          toggle.addEventListener("click", () => sendCommand("set_task_done", {
            task_id: task.id, done: task.status !== "done"
          }));
          card.append(heading, details, toggle);
          if (task.status === "done") card.dataset.done = "true";
          messagesEl.append(card);
        }
      }

      function selectChannel(channel: Channel): void {
        if (!channel) return;
        if (channel.id === activeChannelId && channel.id === connectionSupervisor.snapshot().subscribedChannelId) return;
        persistActiveDraft();
        setMobileNavigationOpen(false);
        activeInboxKind = null;
        const previousChannelId = connectionSupervisor.takeSubscribedChannel();
        if (previousChannelId) sendCommand("unsubscribe_channel", { channel_id: previousChannelId });
        timeline.length = 0;
        threadReplies.clear();
        threadRoots.clear();
        threadSummaries.clear();
        if (threadPanel.open) threadPanel.close();
        seenMessageIds.clear();
        historyRequestIds.clear();
        historyHasMore = false;
        historyLoading = false;
        messagesEl.replaceChildren();
        navigation.setActiveChannel(channel);
        syncRenderedNavigation();
        restoreActiveDraft();
        if (channel.circle_id) {
          circleSelect.value = channel.circle_id;
        } else {
          circleSelect.value = "";
        }
        reconnectScrollOffset = null;
        closeMentionSuggestions();
        if (channel.circle_id) sendCommand("list_circle_users", { circle_id: channel.circle_id });
        sendCommand("list_channel_reactions", { channel_id: channel.id });
        renderMediaPreviews();
        requestedChannelSlug = channel.slug;
        renderConversationIdentity();
        renderChannels();
        updateAgentAccessControls();
        bodyInput.disabled = true;
        sendButton.disabled = true;
        attachMediaButton.disabled = true;
        messageEmojiPicker.setAttribute("aria-disabled", "true");
        setConnectionStatus("Koplar til samtalen …");
        if (!sendCommand("subscribe_channel", { channel_id: channel.id })) {
          setConnectionStatus("Vent på samband – trykk på samtalen for å prøve igjen");
        }
      }

      function updateAgentAccessControls() {
        const channel = knownChannels.find((item) => item.id === activeChannelId);
        const canDelegate = channel?.role === "owner" || channel?.role === "moderator";
        createAgentAccessButton.disabled = !canDelegate || temporaryAgentId !== null;
        if (!activeChannelId && temporaryAgentId === null) {
          agentAccessNotice.textContent = "Vel ei samtale for å lage tilgang.";
        } else if (!canDelegate && temporaryAgentId === null) {
          agentAccessNotice.textContent = "Berre eigarar og moderatorar kan gi agenttilgang til denne samtalen.";
        } else if (temporaryAgentId === null) {
          agentAccessNotice.textContent = "Klar til å lage kortliva agenttilgang for denne samtalen.";
        }
      }

      async function createTemporaryAgentAccess() {
        if (!activeChannelId || temporaryAgentId !== null) return;
        createAgentAccessButton.disabled = true;
        agentAccessNotice.textContent = "Lagar kortliva agenttilgang …";
        const expiresAt = new Date(Date.now() + 30 * 60_000).toISOString();
        let created: CreatedAgent | null = null;
        try {
          created = await agentsApi.create({ displayName: "Kortliva MCP-agent", provider: "sproyt-owner-ui", serviceIdentity: crypto.randomUUID(), purpose: `Kortliva MCP-tilgang til kanal ${activeChannelId}`, rateLimitPerMinute: 30, expiresAt });
          for (const scope of ["read_history", "send_messages"] as const) {
            await agentsApi.grant(created.agentId, activeChannelId, scope, expiresAt);
          }
          temporaryAgentId = created.agentId;
          agentCredential.value = created.credential;
          agentCredential.hidden = false;
          copyAgentCredentialButton.hidden = false;
          revokeAgentAccessButton.hidden = false;
          agentAccessNotice.textContent = `Tilgangen ${created.agentId} er klar i 30 minutt. Kopier credentialen no, og trekk han tilbake når testen er ferdig.`;
        } catch (error) {
          if (created) {
            await agentsApi.revoke(created.agentId).catch(() => {});
          }
          agentAccessNotice.textContent = `Kunne ikkje lage agenttilgang: ${error instanceof Error ? error.message : "ukjend feil"}`;
          updateAgentAccessControls();
        }
      }

      async function revokeTemporaryAgentAccess() {
        if (!temporaryAgentId) return;
        revokeAgentAccessButton.disabled = true;
        try {
          await agentsApi.revoke(temporaryAgentId);
          temporaryAgentId = null;
          agentCredential.value = "";
          agentCredential.hidden = true;
          copyAgentCredentialButton.hidden = true;
          revokeAgentAccessButton.hidden = true;
          revokeAgentAccessButton.disabled = false;
          updateAgentAccessControls();
          agentAccessNotice.textContent = "Agenttilgangen er trekt tilbake.";
        } catch (error) {
          revokeAgentAccessButton.disabled = false;
          agentAccessNotice.textContent = `Kunne ikkje trekkje tilbake agenttilgangen: ${error instanceof Error ? error.message : "ukjend feil"}`;
        }
      }

      function updateLatestSequence(channelId: string, sequence: number): void {
        const channel = knownChannels.find((item) => item.id === channelId);
        if (channel) channel.latest_sequence = Math.max(channel.latest_sequence || 0, sequence);
      }

      function acknowledgeLatest(channelId: string, messages: ChatMessage[]): void {
        if (channelId !== activeChannelId || messages.length === 0 || document.visibilityState === "hidden") return;
        const latestMessage = messages.at(-1);
        if (!latestMessage) return;
        const sequence = latestMessage.sequence;
        updateLatestSequence(channelId, sequence);
        sendCommand("mark_read", { channel_id: channelId, sequence });
      }

      document.addEventListener("visibilitychange", () => {
        if (document.visibilityState !== "visible") { hiddenSince = Date.now(); return; }
        resumeAfterBackground(false);
        hiddenSince = null;
        sendCommand("list_my_channels");
        if (!activeChannelId) return;
        const visibleMessages = timeline
          .filter((item): item is Readonly<{ type: "message"; message: ChatMessage }> => item.type === "message")
          .filter((item) => item.message.channel_id === activeChannelId)
          .map((item) => item.message);
        acknowledgeLatest(activeChannelId, visibleMessages);
      });

      function pushSystem(text: string): void {
        timeline.push({ type: "system", text });
        renderTimeline();
      }

      function loadOlderHistory() {
        if (!activeChannelId || !historyHasMore || historyLoading || connectionSupervisor.snapshot().subscribedChannelId !== activeChannelId) return;
        const oldestItem = timeline.find((item): item is Readonly<{ type: "message"; message: ChatMessage }> => item.type === "message");
        const oldest = oldestItem?.message;
        if (!oldest) return;
        historyLoading = true;
        const requestId = sendCommand("load_recent_messages", {
          channel_id: activeChannelId,
          before: oldest.sequence,
          limit: historyPageSize
        });
        if (requestId) historyRequestIds.add(requestId);
        else historyLoading = false;
      }

      function renderTimeline({ preserveScroll = false, forceBottom = false, revealMessageId = null }: Readonly<{ preserveScroll?: boolean; forceBottom?: boolean; revealMessageId?: string | null }> = {}): void {
        const previousHeight = messagesEl.scrollHeight;
        const previousTop = messagesEl.scrollTop;
        const wasNearBottom = previousHeight - previousTop - messagesEl.clientHeight < 80;
        const interaction = captureMessageInteraction(messagesEl);
        messagesEl.replaceChildren();
        for (const item of timeline) {
          if (item.type === "message") {
            appendMessage(item.message);
          } else {
            appendSystem(item.text);
          }
        }
        renderMermaidDiagrams();
        restoreMessageInteraction(messagesEl, interaction);
        if (preserveScroll) {
          messagesEl.scrollTop = messagesEl.scrollHeight - previousHeight + previousTop;
        } else if (revealMessageId) {
          revealTimelineMessage(revealMessageId);
        } else if (forceBottom) {
          settleConversationAtBottom();
        } else if (wasNearBottom) {
          messagesEl.scrollTop = messagesEl.scrollHeight;
        }
      }

      function revealTimelineMessage(messageId: string): void {
        const reveal = () => {
          const card = [...messagesEl.querySelectorAll("[data-message-id]")]
            .find((candidate): candidate is HTMLElement => candidate instanceof HTMLElement && candidate.dataset.messageId === messageId);
          if (!card) return;
          const cardRect = card.getBoundingClientRect();
          const viewportRect = messagesEl.getBoundingClientRect();
          const delta = cardRect.bottom - viewportRect.bottom + 12;
          if (delta > 0) messagesEl.scrollTop += delta;
        };
        reveal();
        requestAnimationFrame(() => {
          reveal();
          requestAnimationFrame(reveal);
        });
        window.setTimeout(reveal, 150);
        messagesEl.querySelectorAll("img").forEach((image) => {
          if (!image.complete) image.addEventListener("load", reveal, { once: true });
        });
        messagesEl.querySelectorAll("video").forEach((video) => {
          if (video.readyState < 1) video.addEventListener("loadedmetadata", reveal, { once: true });
        });
      }

      function captureMessageInteraction(container: HTMLElement): MessageInteraction | null {
        const picker = container.querySelector(".reaction-picker[open]");
        if (!(picker instanceof HTMLDetailsElement)) return null;
        const card = picker.closest("[data-message-id]");
        const messageId = card instanceof HTMLElement ? card.dataset.messageId : undefined;
        if (!messageId) return null;
        const input = picker.querySelector("input");
        return {
          messageId,
          customReaction: input?.value || "",
          focusCustomReaction: document.activeElement === input,
          focusReactionSummary: document.activeElement === picker.querySelector("summary")
        };
      }

      function restoreMessageInteraction(container: HTMLElement, interaction: MessageInteraction | null): void {
        if (!interaction) return;
        const card = [...container.querySelectorAll("[data-message-id]")]
          .find((candidate): candidate is HTMLElement => candidate instanceof HTMLElement && candidate.dataset.messageId === interaction.messageId);
        const picker = card?.querySelector(".reaction-picker");
        if (!card || !(picker instanceof HTMLDetailsElement)) return;
        card.classList.add("reaction-picker-requested");
        picker.open = true;
        const input = picker.querySelector("input");
        if (input) input.value = interaction.customReaction;
        if (input && interaction.focusCustomReaction) input.focus({ preventScroll: true });
        else if (interaction.focusReactionSummary) picker.querySelector("summary")?.focus({ preventScroll: true });
      }

      function settleConversationAtBottom() {
        const scroll = () => { messagesEl.scrollTop = messagesEl.scrollHeight; };
        scroll();
        requestAnimationFrame(() => {
          scroll();
          requestAnimationFrame(scroll);
        });
        window.setTimeout(scroll, 150);
        messagesEl.querySelectorAll("img").forEach((image) => {
          if (!image.complete) image.addEventListener("load", scroll, { once: true });
        });
        messagesEl.querySelectorAll("video").forEach((video) => {
          if (video.readyState < 1) video.addEventListener("loadedmetadata", scroll, { once: true });
        });
      }

      function restoreConversationScrollOffset(offset: number): void {
        const restore = () => {
          messagesEl.scrollTop = Math.max(0, messagesEl.scrollHeight - messagesEl.clientHeight - offset);
        };
        restore();
        requestAnimationFrame(restore);
        messagesEl.querySelectorAll("img").forEach((image) => {
          if (!image.complete) image.addEventListener("load", restore, { once: true });
        });
        messagesEl.querySelectorAll("video").forEach((video) => {
          if (video.readyState < 1) video.addEventListener("loadedmetadata", restore, { once: true });
        });
      }

      function renderMessage(message: ChatMessage): void {
        appendTimelineMessage(message);
        renderTimeline();
      }

      function appendTimelineMessage(message: ChatMessage): void {
        if (seenMessageIds.has(message.id)) return;
        seenMessageIds.add(message.id);
        if (message.parent_message_id) {
          const replies = threadReplies.get(message.parent_message_id) || [];
          replies.push(message);
          replies.sort((left, right) => left.sequence - right.sequence);
          threadReplies.set(message.parent_message_id, replies);
          const previous = threadSummaries.get(message.parent_message_id);
          threadSummaries.set(message.parent_message_id, {
            root_message_id: message.parent_message_id,
            reply_count: (previous?.reply_count || 0) + 1,
            unread_count: activeThreadRootId === message.parent_message_id || message.sender_id === currentParticipantId
              ? (previous?.unread_count || 0)
              : (previous?.unread_count || 0) + 1,
            latest_sequence: message.sequence
          });
          if (activeThreadRootId === message.parent_message_id) {
            renderThread({ revealOwn: message.sender_id === currentParticipantId });
            sendCommand("mark_thread_read", { root_message_id: message.parent_message_id, sequence: message.sequence });
          }
          return;
        }
        timeline.push({ type: "message", message });
      }

      function replaceTimelineMessage(message: ChatMessage): void {
        if (message.parent_message_id) {
          const replies = threadReplies.get(message.parent_message_id) || [];
          const index = replies.findIndex((candidate) => candidate.id === message.id);
          if (index >= 0) replies[index] = message;
          else replies.push(message);
          replies.sort((left, right) => left.sequence - right.sequence);
          threadReplies.set(message.parent_message_id, replies);
          if (activeThreadRootId === message.parent_message_id) renderThread({ revealOwn: message.sender_id === currentParticipantId });
          return;
        }
        if (threadRoots.has(message.id)) {
          threadRoots.set(message.id, message);
          if (activeThreadRootId === message.id) renderThread();
        }
        const item = timeline.find((candidate): candidate is Readonly<{ type: "message"; message: ChatMessage }> => candidate.type === "message" && candidate.message.id === message.id);
        if (item) {
          const index = timeline.indexOf(item);
          if (index >= 0) timeline[index] = { type: "message", message };
        }
        else if (!threadRoots.has(message.id)) appendTimelineMessage(message);
      }

      function prependTimelineMessages(messages: ChatMessage[]): void {
        const older: TimelineItem[] = [];
        for (const message of messages) {
          if (seenMessageIds.has(message.id)) continue;
          seenMessageIds.add(message.id);
          if (message.parent_message_id) {
            const replies = threadReplies.get(message.parent_message_id) || [];
            replies.push(message);
            replies.sort((left, right) => left.sequence - right.sequence);
            threadReplies.set(message.parent_message_id, replies);
            continue;
          }
          older.push({ type: "message", message });
        }
        timeline.unshift(...older);
      }

      function openThread(messageId: string): void {
        persistThreadDraft();
        const wasKnown = threadComposerStates.has(messageId);
        activeThreadRootId = messageId;
        const state = threadComposerState(messageId);
        if (!wasKnown && state && activeChannelId) state.draft = restoreThreadDraft(messageId, activeChannelId);
        threadBody.value = state?.draft || "";
        threadUploadStatus.textContent = state?.status || "";
        threadUploadStatus.dataset.kind = state?.statusKind || "progress";
        threadBody.readOnly = false;
        if (!threadPanel.open) threadPanel.showModal();
        threadMessages.innerHTML = '<div class="empty-state"><p>Lastar tråden …</p></div>';
        renderThreadMediaPreviews();
        syncThreadComposer();
        sendCommand("load_thread", { root_message_id: messageId });
      }

      function renderThread({ revealOwn = false } = {}) {
        if (!activeThreadRootId) return;
        const previousHeight = threadMessages.scrollHeight;
        const previousTop = threadMessages.scrollTop;
        const distanceFromBottom = previousHeight - previousTop - threadMessages.clientHeight;
        const wasNearBottom = distanceFromBottom <= 80;
        const rootItem = timeline.find((item): item is Readonly<{ type: "message"; message: ChatMessage }> => item.type === "message" && item.message.id === activeThreadRootId);
        const root = rootItem?.message
          || threadRoots.get(activeThreadRootId);
        if (!root) return;
        threadForm.hidden = Boolean(root.deleted_at);
        threadMessages.replaceChildren();
        const rootContainer = document.createElement("div");
        rootContainer.className = "thread-root";
        appendMessage(root, rootContainer, false);
        threadMessages.append(rootContainer);
        for (const reply of threadReplies.get(activeThreadRootId) || []) {
          appendMessage(reply, threadMessages, false);
        }
        if (revealOwn || wasNearBottom) settleThreadAtBottom();
        else threadMessages.scrollTop = threadMessages.scrollHeight - previousHeight + previousTop;
      }

      function settleThreadAtBottom() {
        const scroll = () => { threadMessages.scrollTop = threadMessages.scrollHeight; };
        scroll();
        requestAnimationFrame(() => {
          scroll();
          requestAnimationFrame(scroll);
        });
        window.setTimeout(scroll, 150);
        threadMessages.querySelectorAll("img").forEach((image) => {
          if (!image.complete) image.addEventListener("load", scroll, { once: true });
        });
        threadMessages.querySelectorAll("video").forEach((video) => {
          if (video.readyState < 1) video.addEventListener("loadedmetadata", scroll, { once: true });
        });
      }

      type ReactionSummary = { count: number; reactedByMe: boolean; userIds: string[] };
      type ServerReaction = Readonly<{ message_id: string; emoji: string; count: number; reacted_by_me: boolean; user_ids?: string[] }>;
      type ReactionChange = Readonly<{ message_id: string; emoji: string; user_id: string; channel_id: string; count: number; added: boolean }>;
      function replaceChannelReactions(reactions: ReadonlyArray<ServerReaction>): void {
        messageReactions.clear();
        for (const reaction of reactions) {
          if (!messageReactions.has(reaction.message_id)) messageReactions.set(reaction.message_id, new Map());
          const reactionMap = messageReactions.get(reaction.message_id);
          if (!reactionMap) continue;
          reactionMap.set(reaction.emoji, {
            count: reaction.count,
            reactedByMe: reaction.reacted_by_me,
            userIds: reaction.user_ids || []
          });
        }
      }

      function applyReactionChange(change: ReactionChange): void {
        if (!messageReactions.has(change.message_id)) messageReactions.set(change.message_id, new Map());
        const reactions = messageReactions.get(change.message_id);
        if (!reactions) return;
        const current = reactions.get(change.emoji) || { count: 0, reactedByMe: false, userIds: [] };
        current.count = change.count;
        current.userIds = current.userIds.filter((userId) => userId !== change.user_id);
        if (change.added) current.userIds.push(change.user_id);
        if (change.user_id === currentParticipantId) current.reactedByMe = change.added;
        if (current.count === 0) reactions.delete(change.emoji);
        else reactions.set(change.emoji, current);
        if (reactions.size === 0) messageReactions.delete(change.message_id);
      }

      function reactionButton(messageId: string, emoji: string, reaction: ReactionSummary): HTMLButtonElement {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "reaction-badge";
        button.setAttribute("aria-pressed", String(reaction.reactedByMe));
        button.setAttribute("aria-label", `${emoji}: ${reaction.count} reaksjonar`);
        button.textContent = `${emoji} ${reaction.count}`;
        button.addEventListener("click", () => sendCommand("toggle_message_reaction", {
          message_id: messageId, emoji
        }));
        return button;
      }

      function renderMessageReactions(message: ChatMessage, onPickerToggle: (open: boolean) => void): HTMLElement {
        const bar = document.createElement("div");
        bar.className = "message-reactions";
        const reactions = messageReactions.get(message.id) || new Map();
        const displayedEmojis = [...reactions.keys()].sort((left, right) => {
          const leftIndex = reactionEmojis.indexOf(left);
          const rightIndex = reactionEmojis.indexOf(right);
          return (leftIndex < 0 ? 999 : leftIndex) - (rightIndex < 0 ? 999 : rightIndex) || left.localeCompare(right);
        });
        for (const emoji of displayedEmojis) {
          const reaction = reactions.get(emoji);
          if (reaction?.count > 0) bar.append(reactionButton(message.id, emoji, reaction));
        }
        const picker = document.createElement("details");
        picker.className = "reaction-picker";
        picker.addEventListener("toggle", () => onPickerToggle?.(picker.open));
        const summary = document.createElement("summary");
        summary.setAttribute("aria-label", "Legg til reaksjon");
        summary.textContent = "😊 +";
        const choices = document.createElement("div");
        for (const emoji of reactionEmojis) {
          const button = document.createElement("button");
          button.type = "button";
          button.textContent = emoji;
          button.setAttribute("aria-label", `Reager med ${emoji}`);
          button.addEventListener("click", () => {
            sendCommand("toggle_message_reaction", { message_id: message.id, emoji });
            picker.open = false;
          });
          choices.append(button);
        }
        const custom = document.createElement("div");
        custom.className = "reaction-custom";
        const customInput = document.createElement("input");
        customInput.type = "search";
        customInput.maxLength = 32;
        customInput.setAttribute("list", "reaction-emoji-catalog");
        customInput.setAttribute("aria-label", "Søk eller lim inn Unicode-emoji");
        customInput.placeholder = "Søk eller lim inn emoji";
        const customButton = document.createElement("button");
        customButton.type = "button";
        customButton.textContent = "Bruk";
        const submitCustomReaction = () => {
          const emoji = customInput.value.trim();
          if (!emoji) return;
          sendCommand("toggle_message_reaction", { message_id: message.id, emoji });
          picker.open = false;
        };
        customButton.addEventListener("click", submitCustomReaction);
        customInput.addEventListener("keydown", (event) => {
          if (event.key === "Enter") { event.preventDefault(); submitCustomReaction(); }
        });
        custom.append(customInput, customButton);
        choices.append(custom);
        picker.append(summary, choices);
        bar.append(picker);
        return bar;
      }

      function messageHasReactions(messageId: string): boolean {
        return [...(messageReactions.get(messageId)?.values() || [])]
          .some((reaction) => reaction.count > 0);
      }

      function syncMessageReactionDetails(menu: HTMLElement, messageId: string): void {
        const details = menu.querySelector(".message-reaction-details");
        if (!(details instanceof HTMLElement)) return;
        const list = details.querySelector("ul");
        if (!(list instanceof HTMLUListElement)) return;
        list.replaceChildren();
        const reactions = messageReactions.get(messageId) || new Map();
        for (const [emoji, reaction] of reactions) {
          if (reaction.count === 0) continue;
          const names = reaction.userIds.map((userId: string) => userId === currentParticipantId
            ? "Du"
            : (activeProfile(userId)?.display_name || "Ein ven"));
          const item = document.createElement("li");
          item.textContent = `${emoji} ${names.join(", ")}`;
          list.append(item);
        }
        details.hidden = list.childElementCount === 0;
      }

      function placeMessageMenu(card: HTMLElement, footer: HTMLElement, menu: HTMLElement, thread: boolean, messageId: string): void {
        syncMessageReactionDetails(menu, messageId);
        if (messageHasReactions(messageId) || thread) {
          menu.classList.add("footer-menu");
          footer.insertBefore(menu, null);
          return;
        }
        menu.classList.remove("footer-menu");
        card.querySelector(".meta")?.append(menu);
      }

      function patchMessageReactions(messageId: string): boolean {
        const timelineItem = timeline.find((item): item is Readonly<{ type: "message"; message: ChatMessage }> => item.type === "message" && item.message.id === messageId);
        const message = timelineItem?.message
          || threadRoots.get(messageId)
          || [...threadReplies.values()].flat().find((candidate) => candidate.id === messageId);
        if (!message) return false;
        let patched = false;
        for (const container of [messagesEl, threadMessages]) {
          const card = [...container.querySelectorAll("[data-message-id]")]
            .find((candidate): candidate is HTMLElement => candidate instanceof HTMLElement && candidate.dataset.messageId === messageId);
          const reactions = card?.querySelector(".message-reactions");
          if (!card || !(reactions instanceof HTMLElement)) continue;
          const interaction = captureMessageInteraction(container);
          const nextReactions = renderMessageReactions(message, (open) => {
            card.classList.toggle("reaction-picker-requested", open);
          });
          const thread = reactions.querySelector(".thread-link");
          const menu = card.querySelector(".message-menu");
          if (thread instanceof HTMLElement) nextReactions.append(thread);
          if (menu instanceof HTMLElement) placeMessageMenu(card, nextReactions, menu, thread instanceof HTMLElement, messageId);
          reactions.replaceWith(nextReactions);
          restoreMessageInteraction(container, interaction);
          patched = true;
        }
        return patched;
      }

      function appendMessage(message: ChatMessage, target: HTMLElement = messagesEl, includeThread: boolean = true): void {
        const wrapper = document.createElement("article");
        wrapper.className = "message";
        wrapper.dataset.messageId = message.id;

        const meta = document.createElement("div");
        meta.className = "meta";
        const metaText = document.createElement("span");
        metaText.className = "message-meta-text";
        const sender = message.sender_id === currentParticipantId
          ? "Du"
          : (message.sender_display_name || "Ein ven");
        const senderLabel = document.createElement("span");
        senderLabel.textContent = sender;
        senderLabel.dataset.profileUserId = message.sender_id;
        appendProfileStatus(senderLabel, message.sender_id);
        metaText.append(senderLabel);
        const sentAt = new Date(message.sent_at);
        if (!Number.isNaN(sentAt.valueOf())) {
          const timestamp = document.createElement("time");
          timestamp.dateTime = sentAt.toISOString();
          timestamp.title = sentAt.toLocaleString([], { dateStyle: "full", timeStyle: "short" });
          timestamp.textContent = ` · ${formatMessageTimestamp(sentAt)}`;
          metaText.append(timestamp);
        }
        if (message.deleted_at) {
          const deleted = document.createElement("small");
          deleted.textContent = " · sletta";
          deleted.title = new Date(message.deleted_at).toLocaleString();
          metaText.append(deleted);
        } else if (message.edited_at) {
          const edited = document.createElement("small");
          edited.textContent = " · redigert";
          edited.title = new Date(message.edited_at).toLocaleString();
          metaText.append(edited);
        }
        meta.append(metaText);

        const body = document.createElement("div");
        if (message.deleted_at) {
          body.className = "rendered message-tombstone";
          body.textContent = "Meldinga er sletta.";
        } else if (renderMode === "raw") {
          const pre = document.createElement("pre");
          pre.className = "raw-body";
          pre.textContent = message.body;
          body.append(pre);
        } else {
          body.className = "rendered";
          renderMessageBody(message.body, body);
        }

        wrapper.append(meta, body);
        let footer = null;
        if (!message.deleted_at) {
          footer = renderMessageReactions(message, (open) => {
            wrapper.classList.toggle("reaction-picker-requested", open);
          });
          wrapper.append(footer);
        }
        const summary = threadSummaries.get(message.id);
        const replyCount = summary?.reply_count || 0;
        let thread = null;
        if (includeThread && !message.parent_message_id && replyCount > 0) {
          if (!footer) {
            footer = document.createElement("div");
            footer.className = "message-reactions";
            wrapper.append(footer);
          }
          thread = document.createElement("button");
          thread.type = "button";
          thread.className = "thread-link";
          thread.textContent = replyCount === 0 ? "🧵" : `🧵 ${replyCount}`;
          const unreadCount = summary?.unread_count ?? 0;
          if (unreadCount > 0) thread.textContent += ` · ${unreadCount}`;
          thread.title = replyCount === 0 ? "Start ein tråd" : `${replyCount} svar${unreadCount > 0 ? `, ${unreadCount} uleste` : ""}`;
          thread.setAttribute("aria-label", replyCount === 0 ? "Start ein tråd" : `Opne tråd med ${replyCount} svar`);
          thread.addEventListener("click", () => openThread(message.id));
          footer.append(thread);
        }
        if (!message.deleted_at) {
          const menu = document.createElement("details");
          menu.className = "message-menu";
          const menuSummary = document.createElement("summary");
          menuSummary.textContent = "…";
          menuSummary.setAttribute("aria-label", "Fleire handlingar for meldinga");
          const menuItems = document.createElement("div");
          const reactionDetails = document.createElement("section");
          reactionDetails.className = "message-reaction-details";
          reactionDetails.hidden = true;
          const reactionHeading = document.createElement("strong");
          reactionHeading.textContent = "Reaksjonar";
          const reactionNames = document.createElement("ul");
          reactionDetails.append(reactionHeading, reactionNames);
          menuItems.append(reactionDetails);
          const addReaction = document.createElement("button");
          addReaction.type = "button";
          addReaction.textContent = "Legg til reaksjon";
          addReaction.addEventListener("click", () => {
            menu.open = false;
            const picker = wrapper.querySelector(".reaction-picker");
            if (!(picker instanceof HTMLDetailsElement)) return;
            wrapper.classList.add("reaction-picker-requested");
            picker.open = true;
            picker.querySelector("summary")?.focus({ preventScroll: true });
          });
          menuItems.append(addReaction);
          if (includeThread && !message.parent_message_id) {
            const openThreadButton = document.createElement("button");
            openThreadButton.type = "button";
            openThreadButton.textContent = replyCount === 0 ? "Start tråd" : "Opne tråd";
            openThreadButton.addEventListener("click", () => {
              menu.open = false;
              openThread(message.id);
            });
            menuItems.append(openThreadButton);
          }
          if (message.sender_id === currentParticipantId) {
            const edit = document.createElement("button");
            edit.type = "button";
            edit.textContent = "Rediger";
            edit.addEventListener("click", () => {
              menu.open = false;
            const editor = document.createElement("form");
            editor.className = "message-editor";
            const input = document.createElement("textarea");
            const mediaTokenPattern = /\[\[media:[0-9a-f-]{36}\|[^|\]]+\|[^\]]*\]\]/gi;
            const mediaTokens = message.body.match(mediaTokenPattern) || [];
            input.value = message.body.replace(mediaTokenPattern, "").trim();
            input.setAttribute("aria-label", "Rediger melding");
            const controls = document.createElement("div");
            const cancel = document.createElement("button");
            cancel.type = "button";
            cancel.textContent = "Avbryt";
            const save = document.createElement("button");
            save.type = "submit";
            save.textContent = "Lagre";
            controls.append(cancel, save);
            editor.append(input, controls);
            body.hidden = true;
            menu.hidden = true;
            wrapper.insertBefore(editor, wrapper.querySelector(".message-reactions"));
            cancel.addEventListener("click", () => { editor.remove(); body.hidden = false; menu.hidden = false; });
            editor.addEventListener("submit", (event) => {
              event.preventDefault();
              const value = input.value.trim();
              const updatedBody = [value, ...mediaTokens].filter(Boolean).join("\n");
              if (!updatedBody || updatedBody === message.body) { cancel.click(); return; }
              sendCommand("edit_message", { message_id: message.id, body: updatedBody });
              cancel.click();
            });
            input.focus();
          });
            const remove = document.createElement("button");
            remove.type = "button";
            remove.textContent = "Slett";
            remove.addEventListener("click", () => {
              if (window.confirm("Vil du slette meldinga? Ho blir ståande som ei sletta melding i samtalen.")) {
                sendCommand("delete_message", { message_id: message.id });
              }
            });
            menuItems.append(edit, remove);
          }
          menu.append(menuSummary, menuItems);
          if (footer) placeMessageMenu(wrapper, footer, menu, thread !== null, message.id);
        }
        target.append(wrapper);
      }

      function formatMessageTimestamp(sentAt: Date, now: Date = new Date()): string {
        const sameDay = sentAt.getFullYear() === now.getFullYear()
          && sentAt.getMonth() === now.getMonth()
          && sentAt.getDate() === now.getDate();
        if (sameDay) return sentAt.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
        const options: Intl.DateTimeFormatOptions = { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" };
        if (sentAt.getFullYear() !== now.getFullYear()) options.year = "numeric";
        return sentAt.toLocaleString([], options);
      }

      function renderSystem(text: string): void {
        pushSystem(text);
      }

      function appendSystem(text: string): void {
        const line = document.createElement("div");
        line.className = "system";
        line.textContent = text;
        messagesEl.append(line);
      }

      sessionController.start().catch(() => sessionController.schedule(30));
      connectionSupervisor.start();
      const invitationFromUrl = new URL(window.location.href).searchParams.get("invite");
      if (invitationFromUrl) {
        invitationToken.value = window.location.href;
        onboardingNotice.textContent = "Du er invitert til ein vennekrets. Trykk «Bli med» for å godta.";
        updateOnboardingButtons();
      }

      let pendingInvitationChannel: string | null = null;

      function renderMessageBody(source: string, target: HTMLElement): void {
        const token = /\[\[media:([0-9a-f-]{36})\|([^|\]]+)\|([^\]]*)\]\]/gi;
        const attachments: Array<{ id: string; contentType: string; name: string }> = [];
        const invitations: string[] = [];
        const withoutInvitations = source.replace(/\[\[invite:([A-Za-z0-9_-]{32,128})\]\]/g, (_match: string, invitationToken: string) => {
          invitations.push(invitationToken);
          return "";
        });
        const text = withoutInvitations.replace(token, (_match: string, id: string, contentType: string, encodedName: string) => {
          let name = "media";
          try { name = decodeURIComponent(encodedName || "media"); } catch (_) {}
          attachments.push({ id, contentType, name });
          return "";
        }).trim();
        if (text) renderMarkdown(text, target);
        invitations.forEach((invitationToken) => renderInvitationCard(invitationToken, target));
        attachments.forEach((media) => {
          const figure = document.createElement("figure");
          figure.className = "message-media";
          const element = media.contentType.startsWith("video/") ? document.createElement("video") : document.createElement("img");
          const participant = new URL(window.location.href).searchParams.get("participant");
          const authQuery = participant ? `?participant=${encodeURIComponent(participant)}` : "";
          const originalUrl = `/api/v1/media/${media.id}${authQuery}`;
          element.src = media.contentType.startsWith("image/")
            ? `/api/v1/media/${media.id}/preview${authQuery}`
            : originalUrl;
          if (element instanceof HTMLVideoElement) { element.controls = true; element.preload = "metadata"; }
          else {
            element.alt = media.name;
            element.loading = "lazy";
            element.tabIndex = 0;
            element.setAttribute("role", "button");
            element.setAttribute("aria-label", `Vis ${media.name} i full storleik`);
            const open = () => openMediaLightbox(originalUrl, media.name);
            element.addEventListener("click", open);
            element.addEventListener("keydown", (event) => {
              if (event.key === "Enter" || event.key === " ") { event.preventDefault(); open(); }
            });
          }
          const caption = document.createElement("figcaption");
          caption.textContent = media.name;
          figure.append(element, caption);
          target.append(figure);
        });
      }

      function renderInvitationCard(token: string, target: HTMLElement): void {
        const card = document.createElement("section");
        card.className = "invitation-card";
        card.dataset.invitationToken = token;
        card.innerHTML = "<p>Lastar invitasjonen …</p>";
        target.append(card);
        requestInvitationInspection(token);
      }

      function requestInvitationInspection(token: string, force: boolean = false): void {
        const cached = invitationInspectionCache.get(token);
        if (cached?.status === "pending") {
          return;
        }
        if (cached?.status === "missing" || cached?.status === "failed") {
          if (cached.message) showInvitationError(token, cached.message);
          return;
        }
        if (!force && cached?.status === "resolved") {
          updateInvitationCards(token, cached.invitation);
          return;
        }
        const requestId = sendCommand("inspect_invitation", { token });
        if (!requestId) {
          showInvitationError(token, "Invitasjonen kan ikkje hentast medan sambandet er borte.");
          return;
        }
        invitationInspectionCache.set(token, { status: "pending" });
        pendingInvitationInspections.set(requestId, token);
      }

      function refreshVisibleInvitationCards() {
        if (document.visibilityState === "hidden") return;
        const tokens = new Set(
          [...document.querySelectorAll(".invitation-card")]
            .filter((card): card is HTMLElement => card instanceof HTMLElement)
            .map((card) => card.dataset.invitationToken)
            .filter((token): token is string => typeof token === "string")
            .slice(0, 20)
        );
        tokens.forEach((token) => requestInvitationInspection(token, true));
      }

      function updateInvitationCards(token: string, invitation: Invitation): void {
        document.querySelectorAll(".invitation-card").forEach((card) => {
          if (!(card instanceof HTMLElement)) return;
          if (card.dataset.invitationToken !== token) return;
          const accepted = invitation.response === "accepted";
          const declined = invitation.response === "declined";
          const authoredByMe = invitation.invited_by === currentParticipantId;
          const targetName = invitation.channel_name ? `kanalen ${invitation.channel_name}` : `vennekretsen ${invitation.circle_name}`;
          card.classList.toggle("declined", declined);
          card.removeAttribute("aria-busy");
          card.replaceChildren();
          const heading = document.createElement("h4");
          heading.textContent = `Invitasjon til ${targetName}`;
          const detail = document.createElement("p");
          if (authoredByMe) {
            const responses = [];
            if (invitation.accepted_count > 0) responses.push(`${invitation.accepted_count} har godteke`);
            if (invitation.declined_count > 0) responses.push(`${invitation.declined_count} har avvist`);
            detail.textContent = responses.length > 0
              ? `Du sende invitasjonen. ${responses.join(", ")}.`
              : "Du sende invitasjonen. Ventar på svar.";
          } else {
            detail.textContent = accepted ? "Du har godteke invitasjonen." : `${invitation.invited_by_name} har invitert deg.`;
          }
          card.append(heading, detail);
          if (!accepted && !authoredByMe) {
            const actions = document.createElement("div"); actions.className = "invitation-actions";
            const accept = document.createElement("button"); accept.type = "button"; accept.textContent = declined ? "Godta likevel" : "Godta";
            accept.addEventListener("click", () => respondToInvitation(token, "accept_invitation", "Godtek invitasjonen …"));
            const decline = document.createElement("button"); decline.type = "button"; decline.textContent = "Avvis"; decline.disabled = declined;
            decline.addEventListener("click", () => respondToInvitation(token, "decline_invitation", "Avviser invitasjonen …"));
            actions.append(accept, decline); card.append(actions);
          }
        });
      }

      function respondToInvitation(token: string, command: "accept_invitation" | "decline_invitation", pendingText: string): void {
        const requestId = sendCommand(command, { token });
        if (!requestId) {
          showInvitationError(token, "Vent til sambandet er tilbake, og prøv igjen.");
          return;
        }
        pendingInvitationResponses.set(requestId, { token, command });
        document.querySelectorAll(".invitation-card").forEach((card) => {
          if (!(card instanceof HTMLElement)) return;
          if (card.dataset.invitationToken !== token) return;
          card.setAttribute("aria-busy", "true");
          const detail = card.querySelector("p");
          if (detail) detail.textContent = pendingText;
          card.querySelectorAll(".invitation-actions button").forEach((button) => { if (button instanceof HTMLButtonElement) button.disabled = true; });
        });
      }

      function showInvitationError(token: string, message: string): void {
        document.querySelectorAll(".invitation-card").forEach((card) => {
          if (!(card instanceof HTMLElement)) return;
          if (card.dataset.invitationToken !== token) return;
          card.removeAttribute("aria-busy");
          const detail = card.querySelector("p");
          if (detail) detail.textContent = message;
          detail?.setAttribute("role", "alert");
          card.querySelectorAll(".invitation-actions button").forEach((button) => { if (button instanceof HTMLButtonElement) button.disabled = false; });
        });
      }

      function markInvitationAccepted(token: string): void {
        document.querySelectorAll(".invitation-card").forEach((card) => {
          if (!(card instanceof HTMLElement)) return;
          if (card.dataset.invitationToken !== token) return;
          card.removeAttribute("aria-busy");
          card.classList.remove("declined");
          const detail = card.querySelector("p");
          if (detail) detail.textContent = "Du har godteke invitasjonen.";
          card.querySelector(".invitation-actions")?.remove();
        });
      }

      function openMediaLightbox(url: string, name: string): void {
        mediaLightboxImage.src = url;
        mediaLightboxImage.alt = name;
        mediaLightboxCaption.textContent = name;
        mediaLightbox.showModal();
      }

      function renderMarkdown(source: string, target: HTMLElement): void {
        const lines = source.replace(/\r\n/g, "\n").split("\n");
        let paragraph: string[] = [];
        let list: { kind: "ol" | "ul"; element: HTMLOListElement | HTMLUListElement } | null = null;
        let inFence = false;
        let fenceLanguage = "";
        let fenceLines: string[] = [];

        const flushParagraph = () => {
          if (paragraph.length === 0) {
            return;
          }
          const p = document.createElement("p");
          appendInline(p, paragraph.join(" "));
          target.append(p);
          paragraph = [];
        };

        const flushList = () => {
          if (!list) {
            return;
          }
          target.append(list.element);
          list = null;
        };

        const flushFence = () => {
          const code = fenceLines.join("\n");
          if (fenceLanguage.toLowerCase() === "mermaid") {
            const shell = document.createElement("div");
            shell.className = "mermaid-shell";
            const diagram = document.createElement("div");
            diagram.className = "mermaid";
            diagram.textContent = code;
            shell.append(diagram);
            target.append(shell);
          } else {
            const pre = document.createElement("pre");
            const codeEl = document.createElement("code");
            if (fenceLanguage) {
              codeEl.dataset.language = fenceLanguage;
            }
            codeEl.textContent = code;
            pre.append(codeEl);
            target.append(pre);
          }
          inFence = false;
          fenceLanguage = "";
          fenceLines = [];
        };

        for (const line of lines) {
          const fence = line.match(/^```([A-Za-z0-9_-]+)?\s*$/);
          if (fence) {
            if (inFence) {
              flushFence();
            } else {
              flushParagraph();
              flushList();
              inFence = true;
              fenceLanguage = fence[1] || "";
              fenceLines = [];
            }
            continue;
          }

          if (inFence) {
            fenceLines.push(line);
            continue;
          }

          if (/^\s*$/.test(line)) {
            flushParagraph();
            flushList();
            continue;
          }

          const heading = line.match(/^(#{1,3})\s+(.+)$/);
          if (heading) {
            flushParagraph();
            flushList();
            const marker = heading[1];
            const content = heading[2];
            if (!marker || !content) continue;
            const level = String(marker.length);
            const h = document.createElement(`h${level}`);
            appendInline(h, content);
            target.append(h);
            continue;
          }

          const quote = line.match(/^>\s?(.+)$/);
          if (quote) {
            flushParagraph();
            flushList();
            const blockquote = document.createElement("blockquote");
            const content = quote[1];
            if (content) appendInline(blockquote, content);
            target.append(blockquote);
            continue;
          }

          const unordered = line.match(/^\s*[-*]\s+(.+)$/);
          const ordered = line.match(/^\s*\d+\.\s+(.+)$/);
          if (unordered || ordered) {
            const match = unordered ?? ordered;
            if (!match) continue;
            flushParagraph();
            const kind = ordered ? "ol" : "ul";
            let listElement: HTMLOListElement | HTMLUListElement;
            const currentList = list;
            if (!currentList || currentList.kind !== kind) {
              flushList();
              const nextList: { kind: "ol" | "ul"; element: HTMLOListElement | HTMLUListElement } = { kind, element: document.createElement(kind) };
              list = nextList;
              listElement = nextList.element;
            } else {
              listElement = currentList.element;
            }
            const li = document.createElement("li");
            const content = match[1];
            if (content) appendInline(li, content);
            listElement.append(li);
            continue;
          }

          flushList();
          paragraph.push(line.trim());
        }

        if (inFence) {
          flushFence();
        }
        flushParagraph();
        flushList();
      }

      function appendInline(parent: HTMLElement, text: string): void {
        const parts = text.split(/(`[^`]+`)/g);
        for (const part of parts) {
          if (part.startsWith("`") && part.endsWith("`") && part.length > 1) {
            const code = document.createElement("code");
            code.textContent = part.slice(1, -1);
            parent.append(code);
          } else if (part.length > 0) {
            appendLinkedText(parent, part);
          }
        }
      }

      function appendLinkedText(parent: HTMLElement, text: string): void {
        const urlPattern = /https?:\/\/[^\s<>]+/gi;
        let offset = 0;
        for (const match of text.matchAll(urlPattern)) {
          const start = match.index ?? 0;
          if (start > offset) parent.append(document.createTextNode(text.slice(offset, start)));
          let href = match[0];
          let suffix = "";
          while (/[),.!?:;\]}]$/.test(href)) {
            suffix = href.slice(-1) + suffix;
            href = href.slice(0, -1);
          }
          const link = document.createElement("a");
          link.href = href;
          link.target = "_blank";
          link.rel = "noopener noreferrer";
          link.referrerPolicy = "no-referrer";
          link.title = href;
          link.textContent = readableLinkLabel(href);
          parent.append(link);
          if (suffix) parent.append(document.createTextNode(suffix));
          offset = start + match[0].length;
        }
        if (offset < text.length) parent.append(document.createTextNode(text.slice(offset)));
      }

      function readableLinkLabel(href: string): string {
        try {
          const url = new URL(href);
          const host = url.hostname.replace(/^www\./i, "");
          const path = url.pathname === "/" ? "" : url.pathname.replace(/\/$/, "");
          const label = `${host}${path}`;
          return label.length > 72 ? `${label.slice(0, 69)}…` : label;
        } catch (_) {
          return href.length > 72 ? `${href.slice(0, 69)}…` : href;
        }
      }

      async function renderMermaidDiagrams() {
        if (renderMode !== "view") {
          return;
        }
        const diagrams = [...messagesEl.querySelectorAll(".mermaid")].filter((diagram): diagram is HTMLElement => diagram instanceof HTMLElement);
        if (diagrams.length === 0) return;
        if (mermaidPromise === null) {
          const mermaidUrl = new URL("https://cdn.jsdelivr.net/npm/mermaid@11.16.0/dist/mermaid.esm.min.mjs");
          mermaidPromise = import(mermaidUrl.href).then((module: unknown) => {
            if (!isRecord(module)) throw new Error("Mermaid-modulen manglar standardeksport");
            const api = module.default;
            if (!isMermaidApi(api)) throw new Error("Mermaid-modulen har ugyldig API");
            api.initialize({
              startOnLoad: false,
              securityLevel: "strict",
              theme: window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "default"
            });
            return api;
          });
        }
        let mermaid;
        try {
          mermaid = await mermaidPromise;
        } catch (_) {
          diagrams.forEach((diagram) => { diagram.textContent = "Diagrammet kunne ikkje lastast."; });
          return;
        }
        for (const diagram of diagrams) {
          if (diagram.dataset.rendered) {
            continue;
          }
          diagram.dataset.rendered = "true";
          try {
            await mermaid.run({ nodes: [diagram] });
          } catch (error) {
            diagram.textContent = `Mermaid-feil: ${errorMessage(error)}`;
          }
        }
      }
    
