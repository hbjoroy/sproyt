import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useState, type PointerEvent } from "react";
import type { ConversationSnapshot } from "../../application/conversation-snapshot";
import type { ApplicationRuntime } from "../../application/runtime";
import { installSproytStyles, type SproytThemeMode } from "../design-system";
import { createConversationViewProps } from "./host-adapter";
import type { ComposerTarget } from "./host-adapter";
import { mountConversationView } from "./mount";
import { createPreviewReactionPicker, PreviewReactionActions } from "./preview-reactions";
import type { PreviewReactionHost } from "./preview-reactions";
import { PreviewComposer, type PreviewComposerHost } from "./preview-composer";
import { PreviewMediaContent } from "./preview-media";
export type { PreviewComposerState } from "./preview-composer";
import { PreviewManagement } from "./preview-management";
import { InvitationCard } from "./invitation-card";
import { invitationTokensFromMessage, type InvitationCards } from "../../application/invitation-cards";
import type { ChatMessage } from "../../types";
import type { PreviewSettingsHost } from "./preview-settings";
import { PreviewCommunity, type CommunityHost } from "./preview-community";
import type { AdvancedHost } from "../../application/advanced-host";
import { PreviewGrafana } from "./preview-advanced";
import { createTimelineScrollController } from "./timeline-scroll";
import { PreviewImageGeneration } from "./preview-imagegen";
import type { ImageGenerationOwner } from "../../imagegen";
import { PreviewInboxes, type PreviewInboxHost, type PreviewInboxState } from "./preview-inboxes";
import { HeaderActions } from "./header-actions";
import { MarkdownContent, markdownTextFromMessage } from "./markdown-content";

interface DevelopmentPreviewHost extends PreviewReactionHost, PreviewComposerHost, PreviewInboxHost {
  readonly imageGeneration: ImageGenerationOwner;
  readonly legacyContainer: HTMLElement;
  readonly runtime: ApplicationRuntime;
  readonly snapshot: () => ConversationSnapshot;
  readonly theme: () => SproytThemeMode;
  readonly cycleTheme: () => void;
  readonly renderMode: () => "view" | "raw";
  readonly setRenderMode: (mode: "view" | "raw") => void;
  readonly settings: PreviewSettingsHost;
  readonly community: CommunityHost;
  readonly advanced: AdvancedHost;
  readonly inboxState: () => PreviewInboxState;
  readonly select: (channelId: string) => void;
  readonly search: (query: string) => void;
  readonly openThread: (rootId: string) => void;
  readonly closeThread: () => void;
  readonly loadOlder: () => void;
  readonly isOwnMessage: (message: ChatMessage) => boolean;
  readonly takeScrollIntent: () => Readonly<{
    channelRevealMessageId: string | null;
    threadRevealMessageId: string | null;
  }>;
  readonly messageStatus: (message: ChatMessage) => string | undefined;
  readonly invitations: InvitationCards;
  readonly threadLoad: () => { readonly loading: boolean; readonly error?: string };
  readonly canEditMessage: (message: ChatMessage) => boolean;
  readonly editMessage: (messageId: string, body: string) => void;
  readonly deleteMessage: (messageId: string) => void;
  readonly returnToComposer: (target?: ComposerTarget) => void;
  readonly managementCapabilities: () => { agent: boolean; heart: boolean };
  readonly setChannelNotifications: (channelId: string, enabled: boolean) => void;
  /** Persists both host-owned composer scopes before starting login. */
  readonly reauthenticateNow: () => void;
}

function ChannelNotificationControl({ channelId, channelName, enabled, pending, error, onChange }: {
  readonly channelId: string; readonly channelName: string; readonly enabled: boolean; readonly pending: boolean;
  readonly error?: string; readonly onChange: (channelId: string, enabled: boolean) => void;
}) {
  const label = `${enabled ? "Slå av" : "Slå på"} varsel for ${channelName}`;
  return <>
    <Button className="sp-channel-notification-toggle" variant="quiet" busy={pending}
      aria-label={label} title={label} aria-pressed={enabled}
      onClick={() => onChange(channelId, !enabled)}>
      <BellIcon muted={!enabled} />
    </Button>
    {error && <Status tone="error"><span>{error}</span> <Button onClick={() => onChange(channelId, !enabled)}>Prøv igjen</Button></Status>}
  </>;
}

function BellIcon({ muted = false }: { readonly muted?: boolean }) {
  return <svg className="sp-bell-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
    <path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9M10 21h4" />
    {muted && <path d="M4 4l16 16" />}
  </svg>;
}

function ChannelMembersIcon() {
  return <svg className="sp-channel-members-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
    <circle cx="9" cy="8" r="3" /><circle cx="17" cy="10" r="2.25" />
    <path d="M3 20c0-3.3 2.7-6 6-6s6 2.7 6 6M14 20c0-2.5 1.6-4.6 4-5.4" />
  </svg>;
}

function ChannelActions({ snapshot, host }: { readonly snapshot: ConversationSnapshot; readonly host: DevelopmentPreviewHost }) {
  const [open, setOpen] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [confirmLeave, setConfirmLeave] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [leaveError, setLeaveError] = useState("");
  const channel = snapshot.activeChannel;
  const conversation = channel ? snapshot.groups.flatMap(group => group.conversations).find(item => item.id === channel.id) : undefined;
  if (!channel || channel.is_direct) return null;
  const notifications = conversation?.notifications;
  const openDetails = () => { setOpen(false); requestAnimationFrame(() => setDetailsOpen(true)); };
  return <>
    <Button className="sp-context-menu-trigger sp-message-symbol" variant="quiet"
      aria-label={`Medlemmer i ${channel.name}`} title={`Medlemmer i ${channel.name}`} onClick={openDetails}><ChannelMembersIcon /></Button>
    <Button className="sp-context-menu-trigger sp-message-symbol" variant="quiet"
      aria-label="Kanalval" title="Kanalval" onClick={() => setOpen(true)}><span aria-hidden="true">⋯</span></Button>
    <Dialog open={open} title={`Kanalval: ${channel.name}`} closeLabel="Lukk kanalvala"
      onClose={() => { setOpen(false); setConfirmLeave(false); setLeaveError(""); }}>
      <div className="sp-channel-menu-dialog">
        <Button variant="quiet" onClick={openDetails}><ChannelMembersIcon /><span>Medlemmer og kanalomtale</span></Button>
        {notifications && <Button variant="quiet" disabled={notifications.pending} aria-pressed={notifications.enabled}
          onClick={() => host.setChannelNotifications(channel.id, !notifications.enabled)}>
          <BellIcon muted={!notifications.enabled} /><span>Varsel {notifications.enabled ? "på" : "av"}</span>
        </Button>}
        {notifications?.error && <Status tone="error">{notifications.error}</Status>}
        {!confirmLeave ? <Button variant="danger" onClick={() => setConfirmLeave(true)}>Forlat kanalen</Button>
          : <div className="sp-leave-confirm" role="group" aria-label="Stadfest at du vil forlate kanalen">
            <p>Vil du forlate {channel.name}? Meldingane dine blir ståande.</p>
            <div className="sp-row"><Button onClick={() => setConfirmLeave(false)}>Avbryt</Button>
              <Button variant="danger" busy={leaving} onClick={() => {
                if (leaving) return;
                setLeaving(true); setLeaveError("");
                void host.community.leaveChannel(channel.id).then(() => setOpen(false)).catch(error => {
                  setLeaveError(error instanceof Error ? error.message : "Kunne ikkje forlate kanalen.");
                }).finally(() => setLeaving(false));
              }}>Ja, forlat kanalen</Button></div>
          </div>}
        {leaveError && <Status tone="error">{leaveError}</Status>}
      </div>
    </Dialog>
    {detailsOpen && <PreviewCommunity destination={{ kind: "channel", channelId: channel.id }} snapshot={snapshot} host={host.community}
      onNavigate={() => {}} onClose={() => setDetailsOpen(false)} />}
  </>;
}

function PreviewMessageContent({ host, message }: { readonly host: DevelopmentPreviewHost; readonly message: ChatMessage }) {
  return <div className="sp-message-content">
    <MarkdownContent source={markdownTextFromMessage(message.body)} />
    {invitationTokensFromMessage(message.body).map((token, index) => <InvitationCard key={`${token}-${index}`} token={token} host={host.invitations} />)}
  </div>;
}

function PreviewMessageMutations({ host, message }: { readonly host: DevelopmentPreviewHost; readonly message: ChatMessage }) {
  const [editing, setEditing] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const mediaTokenPattern = /\[\[media:[0-9a-f-]{36}\|[^|\]]+\|[^\]]*\]\]/giu;
  const mediaTokens = message.body.match(mediaTokenPattern) ?? [];
  const [body, setBody] = useState("");
  const keepPointerTarget = (event: PointerEvent<HTMLButtonElement>) => {
    // The empty composer collapses on blur. Keep the action under pointerup
    // until click opens the dialog, as the reaction actions already do.
    if (event.pointerType === "mouse" && event.button === 0) event.preventDefault();
  };
  if (!host.canEditMessage(message)) return null;
  const save = () => {
    const updated = [body.trim(), ...mediaTokens].filter(Boolean).join("\n");
    if (updated && updated !== message.body) host.editMessage(message.id, updated);
    setEditing(false);
  };
  return <>
    <Button onPointerDown={keepPointerTarget} onClick={() => { setBody(message.body.replace(mediaTokenPattern, "").trim()); setEditing(true); }}>Rediger</Button>
    <Button variant="danger" onPointerDown={keepPointerTarget} onClick={() => setDeleting(true)}>Slett</Button>
    {editing && <Dialog open title="Rediger melding" closeLabel="Avbryt" onClose={() => setEditing(false)}>
      <form onSubmit={event => { event.preventDefault(); save(); }}>
        <label htmlFor={`edit-${message.id}`}>Melding</label>
        <textarea id={`edit-${message.id}`} value={body} autoFocus rows={5}
          onChange={event => setBody(event.currentTarget.value)} />
        {mediaTokens.length > 0 && <Status>Vedlegga blir verande på meldinga.</Status>}
        <Button type="submit" disabled={!body.trim() && mediaTokens.length === 0}>Lagre</Button>
      </form>
    </Dialog>}
    {deleting && <Dialog open title="Slett melding" closeLabel="Avbryt" onClose={() => setDeleting(false)}>
      <p>Meldinga blir ståande som sletta i samtalen.</p>
      <Button variant="danger" onClick={() => { host.deleteMessage(message.id); setDeleting(false); }}>Slett melding</Button>
    </Dialog>}
  </>;
}


/** Local preview. Domain state, transport and commands retain their
 * existing owner; incomplete mutation/media flows stay in the full interface. */
export function mountDevelopmentPreview(host: DevelopmentPreviewHost) {
  installSproytStyles();
  const container = document.createElement("section");
  container.id = "sproyt-react-preview";
  container.setAttribute("aria-label", "Sprøyt");
  Object.assign(container.style, { position: "fixed", top: "var(--app-offset-top, 0px)", right: "0", bottom: "auto", left: "0",
    zIndex: "1000", height: "var(--app-height, 100dvh)",
    paddingTop: "env(safe-area-inset-top)", paddingRight: "env(safe-area-inset-right)",
    paddingBottom: "env(safe-area-inset-bottom)", paddingLeft: "env(safe-area-inset-left)" });
  document.body.append(container);
  const previouslyInert = host.legacyContainer.inert;
  const previouslyHidden = host.legacyContainer.hidden;
  const previousAriaHidden = host.legacyContainer.getAttribute("aria-hidden");
  host.legacyContainer.inert = true;
  host.legacyContainer.hidden = true;
  host.legacyContainer.setAttribute("aria-hidden", "true");
  let view: "list" | "detail" = "detail";
  let disposed = false;
  let unsubscribe = () => {};
  let mounted: ReturnType<typeof mountConversationView> | undefined;
  let previousSnapshot: ConversationSnapshot | null = null;
  const reactionPicker = createPreviewReactionPicker(host);
  const channelScroll = createTimelineScrollController({ onNearStart: host.loadOlder });
  const threadScroll = createTimelineScrollController();
  const close = (target?: ComposerTarget) => {
    if (disposed) return;
    disposed = true;
    reactionPicker.close();
    channelScroll.dispose();
    threadScroll.dispose();
    unsubscribe();
    mounted?.unmount();
    container.remove();
    host.legacyContainer.inert = previouslyInert;
    host.legacyContainer.hidden = previouslyHidden;
    if (previousAriaHidden === null) host.legacyContainer.removeAttribute("aria-hidden");
    else host.legacyContainer.setAttribute("aria-hidden", previousAriaHidden);
    const url = new URL(window.location.href);
    url.searchParams.delete("ui");
    window.history.replaceState(window.history.state, "", url);
    host.returnToComposer(target);
  };
  const explicitPreview = ["localhost", "127.0.0.1", "[::1]"].includes(window.location.hostname)
    && new URLSearchParams(window.location.search).get("ui") === "react";
  const fullInterface = () => explicitPreview ? <Button onClick={() => close()}>Til fullt grensesnitt</Button> : null;
  const props = () => {
    const snapshot = host.snapshot();
    const scrollIntent = host.takeScrollIntent();
    const channelMessageIds = snapshot.timeline.messages.map(message => message.id);
    const previousChannelIds = previousSnapshot?.selection.channelId === snapshot.selection.channelId
      ? new Set(previousSnapshot.timeline.messages.map(message => message.id)) : null;
    const revealChannelMessage = scrollIntent.channelRevealMessageId ?? (previousChannelIds
      ? [...snapshot.timeline.messages].reverse().find(message => !previousChannelIds.has(message.id) && host.isOwnMessage(message))?.id ?? null
      : null);
    channelScroll.prepare({
      key: snapshot.selection.channelId ? `channel:${snapshot.selection.channelId}` : null,
      messageIds: channelMessageIds,
      hasOlder: snapshot.timeline.hasOlder,
      revealMessageId: revealChannelMessage
    });
    const threadMessageIds = snapshot.thread
      ? [snapshot.thread.root?.id, ...snapshot.thread.replies.map(message => message.id)].filter((id): id is string => Boolean(id))
      : [];
    const previousThread = previousSnapshot?.thread;
    const previousThreadIds = previousThread && snapshot.thread && previousThread.rootMessageId === snapshot.thread.rootMessageId
      ? new Set([previousThread.root?.id, ...previousThread.replies.map(message => message.id)].filter(Boolean)) : null;
    const revealThreadMessage = scrollIntent.threadRevealMessageId ?? (previousThreadIds && snapshot.thread
      ? [...snapshot.thread.replies].reverse().find(message => !previousThreadIds.has(message.id) && host.isOwnMessage(message))?.id ?? null
      : null);
    threadScroll.prepare({
      key: snapshot.thread ? `thread:${snapshot.thread.rootMessageId}` : null,
      messageIds: threadMessageIds,
      revealMessageId: revealThreadMessage
    });
    previousSnapshot = snapshot;
    return createConversationViewProps(snapshot, {
    runtime: host.runtime, theme: host.theme(), view,
    header: <>{host.runtime.getSnapshot().session.reauthenticationRequired && <Status tone="error">
        Økta må stadfestast før Sprøyt kan halde fram. Utkasta dine blir lagra først. <Button onClick={host.reauthenticateNow}>Logg inn på nytt</Button>
      </Status>}
      <HeaderActions primary={<PreviewInboxes state={host.inboxState()} host={host} />}>
      {explicitPreview && <Status>Førehandsvising for utvikling. Meldingar, vedlegg, trådar og reaksjonar er tilgjengelege her.</Status>}
      <Button onClick={host.cycleTheme}>Byt tema</Button><a href="/auth/logout">Logg ut</a>{fullInterface()}
      <Button onClick={() => host.setRenderMode(host.renderMode() === "raw" ? "view" : "raw")}>{host.renderMode() === "raw" ? "Vis formatert" : "Vis råtekst"}</Button>
      <PreviewManagement snapshot={snapshot} capabilities={host.managementCapabilities()} settings={host.settings} advanced={host.advanced}
        community={{ ...host.community, renderIntegration: channelId => <PreviewGrafana key={channelId} host={host.advanced} channelId={channelId} /> }} /></HeaderActions></>,
    navigationActions: null,
    contextActions: <ChannelActions snapshot={snapshot} host={host} />,
    renderConversationAction: conversation => conversation.notifications ? <ChannelNotificationControl
      channelId={conversation.id} channelName={conversation.channel.name}
      enabled={conversation.notifications.enabled} pending={conversation.notifications.pending}
      error={conversation.notifications.error} onChange={host.setChannelNotifications} /> : null,
    onBack() { view = "list"; update(); },
    onQueryChange(query) { host.search(query); update(); },
    onSelect(channelId) { reactionPicker.close(); host.select(channelId); view = "detail"; update(); },
    onCloseThread() {
      const rootId = snapshot.thread?.rootMessageId;
      reactionPicker.close();
      host.closeThread();
      update();
      requestAnimationFrame(() => {
        if (!disposed && rootId) container.querySelector<HTMLElement>(`.sp-channel-pane [data-thread-trigger="${CSS.escape(rootId)}"]`)?.focus({ preventScroll: true });
      });
    },
    threadTimeline: {
      ...host.threadLoad(),
      viewportRef: threadScroll.viewportRef,
      onScroll: threadScroll.onScroll,
      onRetry() { if (snapshot.thread) host.openThread(snapshot.thread.rootMessageId); update(); }
    },
    timeline: {
      onLoadOlder: host.loadOlder,
      viewportRef: channelScroll.viewportRef,
      onScroll: channelScroll.onScroll
    },
    message: {
      formatTime: sentAt => new Date(sentAt).toLocaleTimeString(["nn-NO", "nb-NO"], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" }),
      formatAuthor: message => {
        const profile = host.settings.profileFor?.(message.sender_id);
        return [host.isOwnMessage(message) ? "Du" : message.sender_display_name,
          profile?.status_emoji.trim(), profile?.status_text.trim()].filter(Boolean).join(" · ");
      },
      messageStatus: host.messageStatus,
      onReactionRequest: reactionPicker.open,
      renderActions: (message, context) => {
        if (message.deleted_at) return null;
        const replies = snapshot.threadSummaries.find(summary => summary.root_message_id === message.id)?.reply_count ?? 0;
        const threadAction = message.parent_message_id === null && !context?.threadParent ? <Button className="sp-message-symbol" variant="quiet"
          aria-label={replies ? `${replies} svar i tråd` : "Svar i tråd"} title={replies ? `${replies} svar i tråd` : "Svar i tråd"}
          data-thread-trigger={message.id} aria-expanded={snapshot.thread?.rootMessageId === message.id}
          onPointerDown={event => { if (event.pointerType === "mouse" && event.button === 0) event.preventDefault(); }}
          onClick={() => { host.openThread(message.id); update(); }}><span aria-hidden="true">↩</span>{replies > 0 && <span>{replies}</span>}</Button> : null;
        return <PreviewReactionActions message={message} host={host} open={reactionPicker.open}
          primaryAction={threadAction} overflowActions={<PreviewMessageMutations message={message} host={host} />} />;
      },
      // React owns all visible message content, including Markdown, Mermaid,
      // media and invitation cards. The host supplies state and commands only.
      renderContent: message => host.renderMode() === "raw"
        ? <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{message.body}</pre>
        : <><PreviewMessageContent host={host} message={message} /><PreviewMediaContent body={message.body} mediaOnly /></>
    },
    renderComposer: target => <div className="sp-composer-dock" key={`${target.channelId}:${target.parentMessageId ?? ""}`}>
      {Boolean(target.parentMessageId) === Boolean(snapshot.thread) && <PreviewImageGeneration owner={host.imageGeneration} channelId={snapshot.selection.channelId} />}
      <PreviewComposer host={host} target={target} /></div>,
    overlays: null
    });
  };
  function update() {
    if (!disposed) mounted?.update(props());
  }
  try {
    mounted = mountConversationView(container, props());
    unsubscribe = host.runtime.subscribe(update);
  } catch (error) {
    close();
    throw error;
  }
  return { update, unmount: close };
}
