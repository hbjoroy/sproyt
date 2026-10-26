import { Button, Dialog, Status, TextField } from "@sproyt/ui/react";
import { useEffect, useRef, useState, type PointerEvent } from "react";
import type { ConversationSnapshot } from "../../application/conversation-snapshot";
import type { ApplicationRuntime } from "../../application/runtime";
import { installSproytStyles, type SproytThemeMode } from "../design-system";
import { createConversationViewProps } from "./host-adapter";
import { messagesForTimeline } from "./conversation-view";
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
import { MessageProfileStatus, type PreviewSettingsHost } from "./preview-settings";
import { PreviewCommunity, type CommunityHost } from "./preview-community";
import type { AdvancedHost } from "../../application/advanced-host";
import { PreviewGrafana } from "./preview-advanced";
import { createTimelineScrollController } from "./timeline-scroll";
import { PreviewImageGeneration } from "./preview-imagegen";
import type { ImageGenerationOwner } from "../../imagegen";
import { PreviewInboxes, type PreviewInboxHost, type PreviewInboxState } from "./preview-inboxes";
import { HeaderActions } from "./header-actions";
import { MarkdownContent, markdownTextFromMessage } from "./markdown-content";
import { processTaskId, type ProcessPilotApi } from "../../process-pilot";
import { ProcessPilotChannelAction, ProcessTaskMessage } from "./process-pilot";
import { NavigationScopeActions } from "./navigation-scope-actions";
import { NewDirectMessage } from "./new-direct-message";
import type { CircleChatAgentApi } from "../../chat-agents";
import { ChannelChatAgentsDialog } from "./channel-chat-agents";
import type { WorkApplication, WorkItemApi } from "../../work-items";
import { WorkItemRegistration } from "./work-item-registration";
import { WorkItemTaskMessage } from "./work-item-task";
import { WorkItemStatusMessage } from "./work-item-status";
import { workItemStatusId, workItemTaskId } from "../../work-items";
import type { AppUpdate, UpdatePosition } from "../../app-update";
import { AppUpdateNotice } from "./app-update-action";
import { ReceivedShares } from "./share-target";
import { ViewHistory, viewHref, type ViewTarget } from "../../view-history";
import type { ManagementTarget } from "../../application/management-target";
import { ViewHistoryControls } from "./view-history-controls";
import type { ShareTarget } from "../../share-target";
import { AgentLocationSharingDialog } from "./agent-location-sharing";

interface DevelopmentPreviewHost extends PreviewReactionHost, PreviewComposerHost, PreviewInboxHost {
  readonly favorites: import("../../circle-favorites").CircleFavorites;
  readonly viewHistory: ViewHistory;
  readonly managementTarget: () => ManagementTarget | null;
  readonly navigateView: (target: ViewTarget, mode?: "push" | "replace") => void;
  readonly openRecentView: (target: ViewTarget) => void;
  readonly backFromManagement: () => void;
  readonly navigationNotice: () => string;
  readonly hasLegacyDraft: () => boolean;
  readonly recoverLegacyDraft: () => void;
  readonly processPilot: ProcessPilotApi;
  readonly workItems: WorkItemApi;
  readonly processPilotIdentity: () => string;
  readonly imageGeneration: ImageGenerationOwner;
  readonly chatAgents: CircleChatAgentApi;
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
  readonly retryHistory: () => void;
  readonly acknowledgeVisible: (key: string, messageIds: readonly string[]) => void;
  readonly isOwnMessage: (message: ChatMessage) => boolean;
  readonly takeScrollIntent: () => Readonly<{
    channelRevealMessageId: string | null;
    threadRevealMessageId: string | null;
  }>;
  readonly messageStatus: (message: ChatMessage) => string | undefined;
  readonly invitations: InvitationCards;
  readonly threadLoad: () => { readonly loading: boolean; readonly error?: string };
  readonly canDeleteMessage: (message: ChatMessage) => boolean;
  readonly canEditMessage: (message: ChatMessage) => boolean;
  readonly editMessage: (messageId: string, body: string) => void;
  readonly deleteMessage: (messageId: string) => void;
  readonly returnToComposer: (target?: ComposerTarget) => void;
  readonly managementCapabilities: () => { agent: boolean; heart: boolean };
  readonly setChannelNotifications: (channelId: string, enabled: boolean) => void;
  /** Persists both host-owned composer scopes before starting login. */
  readonly reauthenticateNow: () => void;
  readonly appUpdate: AppUpdate;
  readonly shares: ShareTarget;
}

function ChannelNotificationControl({ channelId, channelName, enabled, pending, error, onChange }: {
  readonly channelId: string; readonly channelName: string; readonly enabled: boolean; readonly pending: boolean;
  readonly error?: string; readonly onChange: (channelId: string, enabled: boolean) => void;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);
  useEffect(() => {
    if (pending || !restoreFocus.current) return;
    restoreFocus.current = false;
    // Disabling or moving a keyed DOM button can drop focus to the body.
    // Never take focus back from another control the user moved to meanwhile.
    if (document.activeElement === document.body || document.activeElement === button.current) {
      button.current?.focus({ preventScroll: true });
    }
  }, [pending, enabled]);
  const label = `${enabled ? "Slå av" : "Slå på"} varsel for ${channelName}`;
  return <>
    <Button ref={button} className="sp-channel-notification-toggle" variant="symbol" busy={pending}
      aria-label={label} title={label} aria-pressed={enabled}
      onClick={event => {
        event.currentTarget.focus({ preventScroll: true });
        restoreFocus.current = true;
        onChange(channelId, !enabled);
      }}>
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

function ChannelActions({ snapshot, host, compact = false }: { readonly snapshot: ConversationSnapshot; readonly host: DevelopmentPreviewHost; readonly compact?: boolean }) {
  const [open, setOpen] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [focusMemberAction, setFocusMemberAction] = useState(false);
  const [inviteOpen, setInviteOpen] = useState(false);
  type AgentTicket = { owner: string; channelId: string; channelName: string; privateChannel: boolean;
    key: string; epoch: number; api: CircleChatAgentApi };
  const [agentTicket, setAgentTicket] = useState<AgentTicket | null>(null);
  const [locationOpen, setLocationOpen] = useState(false);
  const [confirmLeave, setConfirmLeave] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [leaveError, setLeaveError] = useState("");
  const channel = snapshot.activeChannel;
  const owner = host.community.selfId();
  const scopeKey = (current: ConversationSnapshot, account: string | null) => {
    const selected = current.activeChannel;
    return JSON.stringify([account, selected?.id, selected?.role, selected?.kind, selected?.circle_id,
      current.circles.find(circle => circle.id === selected?.circle_id)?.role]);
  };
  const agentScope = useRef({ key: "", epoch: 0, live: true });
  const observeScope = (key: string) => {
    if (agentScope.current.key !== key) { agentScope.current.key = key; agentScope.current.epoch++; }
  };
  observeScope(scopeKey(snapshot, owner));
  useEffect(() => {
    agentScope.current.live = true;
    return () => { agentScope.current.live = false; agentScope.current.epoch++; };
  }, []);
  const ticketAdmitted = (ticket: Pick<AgentTicket, "key" | "epoch">) => {
    observeScope(scopeKey(host.snapshot(), host.community.selfId()));
    return agentScope.current.live && agentScope.current.key === ticket.key && agentScope.current.epoch === ticket.epoch;
  };
  const conversation = channel ? snapshot.groups.flatMap(group => group.conversations).find(item => item.id === channel.id) : undefined;
  if (!channel || channel.is_direct) return null;
  const mayManageAgents = !!owner && !!channel.circle_id && (["owner", "moderator"].includes(channel.role)
    || (channel.kind !== "private" && channel.role === "member" && snapshot.circles.some(circle =>
      circle.id === channel.circle_id && ["owner", "moderator"].includes(circle.role))));
  const notifications = conversation?.notifications;
  const openDetails = () => { setFocusMemberAction(false); setOpen(false); requestAnimationFrame(() => setDetailsOpen(true)); };
  return <>
    {!compact && <Button className="sp-context-menu-trigger sp-message-symbol" variant="quiet"
      aria-label={`Medlemmer i ${channel.name}`} title={`Medlemmer i ${channel.name}`} onClick={openDetails}><ChannelMembersIcon /></Button>
    }<Button className={compact ? "sp-mobile-channel-menu-entry" : "sp-context-menu-trigger sp-message-symbol"} variant="quiet"
      aria-label="Kanalval" title="Kanalval" onClick={() => setOpen(true)}><span aria-hidden="true">⋯</span>{compact && <span>Kanalval</span>}</Button>
    <Dialog open={open} title={`Kanalval: ${channel.name}`} closeLabel="Lukk kanalvala"
      onClose={() => { setOpen(false); setConfirmLeave(false); setLeaveError(""); }}>
      <div className="sp-channel-menu-dialog">
        <Button variant="quiet" onClick={openDetails}><ChannelMembersIcon /><span>Medlemmer og kanalomtale</span></Button>
        {mayManageAgents && <Button variant="quiet" onClick={() => {
          if (!owner) return;
          const captured = { key: agentScope.current.key, epoch: agentScope.current.epoch };
          if (!ticketAdmitted(captured)) return;
          const ticket = { ...captured, owner, channelId: channel.id, channelName: channel.name, privateChannel: channel.kind === "private",
            api: host.chatAgents.forOwner(owner, () => ticketAdmitted(captured)) };
          setOpen(false); setAgentTicket(ticket);
        }}>Agentar i kanalen</Button>}
        {["owner", "moderator"].includes(channel.role) && <Button variant="quiet" onClick={() => {
          setFocusMemberAction(true); setOpen(false); requestAnimationFrame(() => setDetailsOpen(true));
        }}><ChannelMembersIcon /><span>Legg til eller inviter kanalmedlem</span></Button>}
        {snapshot.circles.some(circle => circle.id === channel.circle_id && circle.role === "owner") && <Button variant="quiet"
          onClick={() => { setOpen(false); setInviteOpen(true); }}>Inviter til kretsen</Button>}
        {notifications && <Button variant="quiet" disabled={notifications.pending} aria-pressed={notifications.enabled}
          onClick={() => host.setChannelNotifications(channel.id, !notifications.enabled)}>
          <BellIcon muted={!notifications.enabled} /><span>Varsel {notifications.enabled ? "på" : "av"}</span>
        </Button>}
        {notifications?.error && <Status tone="error">{notifications.error}</Status>}
        {open && <ProcessPilotChannelAction key={`${host.processPilotIdentity()}:${channel.id}`} api={host.processPilot} channelId={channel.id} />}
        <Button variant="quiet" onClick={() => { setOpen(false); setLocationOpen(true); }}><span aria-hidden="true">⌖</span><span>Del posisjon med agent</span></Button>
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
    {detailsOpen && <PreviewCommunity destination={{ kind: "channel", channelId: channel.id }} snapshot={snapshot} host={host.community} focusMemberAction={focusMemberAction}
      onNavigate={() => {}} onClose={() => setDetailsOpen(false)} />}
    {agentTicket && mayManageAgents && agentTicket.key === agentScope.current.key && agentTicket.epoch === agentScope.current.epoch
      && <ChannelChatAgentsDialog key={`${agentTicket.owner}:${agentTicket.channelId}:${agentTicket.epoch}`} api={agentTicket.api}
        channelId={agentTicket.channelId} channelName={agentTicket.channelName} privateChannel={agentTicket.privateChannel}
        onClose={() => { if (ticketAdmitted(agentTicket)) setAgentTicket(null); }} />}
    {locationOpen && <AgentLocationSharingDialog api={host.agentLocations} channelId={channel.id} channelName={channel.name}
      onClose={() => setLocationOpen(false)} />}
    {inviteOpen && channel.circle_id && <PreviewCommunity destination={{ kind: "invite", circleId: channel.circle_id }} snapshot={snapshot} host={host.community}
      onNavigate={() => {}} onClose={() => setInviteOpen(false)} />}
  </>;
}

function PreviewMessageContent({ host, message }: { readonly host: DevelopmentPreviewHost; readonly message: ChatMessage }) {
  const workTask = workItemTaskId(message.body);
  if (workTask) return <WorkItemTaskMessage key={`${host.processPilotIdentity()}:${message.id}:${workTask}`} api={host.workItems} taskId={workTask} messageId={message.id} />;
  const workStatus = workItemStatusId(message.body);
  if (workStatus) return <WorkItemStatusMessage key={`${host.processPilotIdentity()}:${message.id}:${workStatus}`} api={host.workItems} itemId={workStatus} messageId={message.id} />;
  const taskId = processTaskId(message.body);
  if (taskId) return <ProcessTaskMessage key={`${host.processPilotIdentity()}:${message.id}:${taskId}`} api={host.processPilot} taskId={taskId} messageId={message.id} />;
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
  if (!host.canEditMessage(message) && !host.canDeleteMessage(message)) return null;
  const save = () => {
    const updated = [body.trim(), ...mediaTokens].filter(Boolean).join("\n");
    if (updated && updated !== message.body) host.editMessage(message.id, updated);
    setEditing(false);
  };
  return <>
    {host.canEditMessage(message) && <Button onPointerDown={keepPointerTarget} onClick={() => { setBody(message.body.replace(mediaTokenPattern, "").trim()); setEditing(true); }}>Rediger</Button>}
    {host.canDeleteMessage(message) && <Button variant="danger" onPointerDown={keepPointerTarget} onClick={() => setDeleting(true)}>Slett</Button>}
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
      <p>Meldinga blir fjerna frå samtalen. Eventuelle svar i tråden blir verande.</p>
      <Button variant="danger" onClick={() => { host.deleteMessage(message.id); setDeleting(false); }}>Slett melding</Button>
    </Dialog>}
  </>;
}

function WorkItemMenuAction({ host, message, hasApplications, hasExistingItems, onOpen }: {
  host: DevelopmentPreviewHost; message: ChatMessage; hasApplications: boolean; hasExistingItems: boolean; onOpen: () => void;
}) {
  const [hasItems, setHasItems] = useState(false);
  const anchor = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (hasApplications) return;
    let active = true;
    let requested = false;
    const dialog = anchor.current?.closest("dialog");
    const load = () => {
      if (!dialog?.open || requested) return;
      requested = true;
      void host.workItems.sourceItems(message.channel_id, message.id).then(items => { if (active) setHasItems(items.length > 0); }).catch(() => { requested = false; });
    };
    const observer = new MutationObserver(load);
    if (dialog) observer.observe(dialog, { attributes: true, attributeFilter: ["open"] });
    load();
    return () => { active = false; observer.disconnect(); };
  }, [host.workItems, message.channel_id, message.id, hasApplications]);
  return <span ref={anchor}>{(hasApplications || hasExistingItems || hasItems) && <Button onClick={onOpen}>Arbeidssaker</Button>}</span>;
}

function MessageActions({ host, message, threadAction, openReaction }: {
  readonly host: DevelopmentPreviewHost; readonly message: ChatMessage;
  readonly threadAction: React.ReactNode;
  readonly openReaction: (message: ChatMessage, anchor: HTMLElement) => void;
}) {
  const [apps, setApps] = useState<readonly WorkApplication[]>([]);
  const [linkOpen, setLinkOpen] = useState(false);
  const [linkCopied, setLinkCopied] = useState(false);
  const messageLink = viewHref({ kind: "conversation", channelId: message.channel_id, messageId: message.id,
    sequence: message.sequence, ...(message.parent_message_id ? { threadId: message.parent_message_id } : {}) }, window.location.href);
  const [issueOpen, setIssueOpen] = useState(false);
  const [hasExistingItems, setHasExistingItems] = useState(false);
  const availabilityAnchor = useRef<HTMLDivElement>(null);
  const hasMutations = host.canEditMessage(message) || host.canDeleteMessage(message);
  const isAgent = host.settings.profileFor?.(message.sender_id)?.kind === "agent";
  const canHaveWorkItems = !isAgent && !workItemTaskId(message.body) && !workItemStatusId(message.body) && !processTaskId(message.body);
  useEffect(() => {
    setApps([]); setHasExistingItems(false);
    if (!canHaveWorkItems) return;
    let active = true;
    let observer: IntersectionObserver | undefined;
    void host.workItems.applications(message.channel_id)
      .catch(() => [])
      .then(value => {
        if (!active) return;
        setApps(value);
        // Disabled processes may still have registered cases. Only check visible
        // read-only messages; writable messages retain the lazy menu lookup.
        if (value.length || hasMutations) return;
        const load = () => {
          observer?.disconnect();
          void host.workItems.sourceItems(message.channel_id, message.id)
            .then(items => { if (active) setHasExistingItems(items.length > 0); })
            .catch(() => {});
        };
        const element = availabilityAnchor.current?.closest("[data-message-id]");
        if (element && typeof IntersectionObserver !== "undefined") {
          observer = new IntersectionObserver(entries => { if (entries.some(entry => entry.isIntersecting)) load(); });
          observer.observe(element);
        } else load();
      });
    return () => { active = false; observer?.disconnect(); };
  }, [host.workItems, canHaveWorkItems, hasMutations, message.channel_id, message.id]);
  const hasWorkItemAction = canHaveWorkItems && (apps.length > 0 || hasExistingItems);
  return <div ref={availabilityAnchor} style={{ display: "contents" }}>
    <PreviewReactionActions message={message} host={host} open={openReaction} primaryAction={threadAction}
      overflowActions={close => <>
        <Button variant="quiet" onClick={() => {
          close();
          void navigator.clipboard?.writeText(messageLink).then(() => setLinkCopied(true), () => setLinkOpen(true));
          if (!navigator.clipboard) setLinkOpen(true);
        }}>Kopier lenkje til meldinga</Button>
        <PreviewMessageMutations message={message} host={host} />
        {canHaveWorkItems
          && <WorkItemMenuAction host={host} message={message} hasApplications={apps.length > 0} hasExistingItems={hasExistingItems} onOpen={() => { close(); setIssueOpen(true); }} />}
      </>} />
    {linkCopied && <span className="sp-copy-link-notice" role="status">Lenkja er kopiert.</span>}
    {linkOpen && <Dialog open title="Lenkje til meldinga" onClose={() => setLinkOpen(false)}>
      <TextField label="Meldingslenkje" value={messageLink} readOnly onFocus={event => event.currentTarget.select()} />
      <p>Merk og kopier lenkja. Ho opnar denne meldinga for dei som har tilgang til samtalen.</p>
    </Dialog>}
    {issueOpen && <WorkItemRegistration api={host.workItems} message={message} open onClose={() => setIssueOpen(false)} />}
  </div>;
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
  let resumedThread = false;
  let renderedOwner = host.community.selfId();
  let renderedEntry = "";
  const reactionPicker = createPreviewReactionPicker(host);
  const restorePosition = (key: string) => host.appUpdate.resumePosition(host.processPilotIdentity(), key);
  const captureReadingPosition = () => host.viewHistory.capture(captureUpdatePosition());
  const channelScroll = createTimelineScrollController({ onNearStart: host.loadOlder, onVisibleMessages: host.acknowledgeVisible, restorePosition, onPositionChanged: captureReadingPosition });
  const threadScroll = createTimelineScrollController({ onVisibleMessages: host.acknowledgeVisible, restorePosition, onPositionChanged: captureReadingPosition });
  const captureUpdatePosition = () => [channelScroll.readingPosition(), threadScroll.readingPosition()].filter((entry): entry is UpdatePosition => entry !== null);
  window.addEventListener("pagehide", captureReadingPosition);
  const close = (target?: ComposerTarget) => {
    if (disposed) return;
    disposed = true;
    window.removeEventListener("pagehide", captureReadingPosition);
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
    const owner = host.community.selfId();
    if (owner !== renderedOwner) {
      renderedOwner = owner; channelScroll.reset(); threadScroll.reset(); resumedThread = false; renderedEntry = "";
      reactionPicker.close();
    }
    const historyState = host.viewHistory.getSnapshot();
    if (historyState.entryId !== renderedEntry) {
      renderedEntry = historyState.entryId;
      if (historyState.target?.kind === "directory") view = "list";
      else if (historyState.target?.kind === "conversation") view = "detail";
    }
    if (!resumedThread && snapshot.selection.channelId && !snapshot.timeline.loading) {
      resumedThread = true;
      const rootId = host.appUpdate.resumeThread(host.processPilotIdentity(), snapshot.selection.channelId);
      if (rootId && historyState.target?.kind === "conversation" && !historyState.target.messageId && !historyState.target.threadId)
        queueMicrotask(() => {
          if (!disposed && host.community.selfId() === owner && host.viewHistory.getSnapshot().entryId === historyState.entryId)
            host.navigateView({ kind: "conversation", channelId: snapshot.selection.channelId!, threadId: rootId }, "replace");
        });
    }
    const scrollIntent = host.takeScrollIntent();
    const channelMessages = messagesForTimeline({ ...snapshot.timeline });
    const channelMessageIds = channelMessages.map(message => message.id);
    channelScroll.prepare({
      restorationId: `${owner}:${historyState.entryId}`,
      restoredPosition: historyState.positions.find(p => p.key === `channel:${snapshot.selection.channelId}`)?.position,
      key: snapshot.selection.channelId ? `channel:${snapshot.selection.channelId}` : null,
      messageIds: channelMessageIds,
      messageSequences: channelMessages.map(message => message.sequence),
      initialReadSequence: snapshot.activeChannel?.last_read_sequence,
      initialUnreadLimit: 20,
      hasOlder: snapshot.timeline.hasOlder,
      loading: snapshot.timeline.loading,
      error: snapshot.timeline.error,
      waitingForLink: snapshot.timeline.waitingForMessageLink,
      revealMessageId: scrollIntent.channelRevealMessageId
    });
    const threadMessageIds = snapshot.thread
      ? [snapshot.thread.root?.id, ...snapshot.thread.replies.filter(message => !message.deleted_at).map(message => message.id)].filter((id): id is string => Boolean(id))
      : [];
    threadScroll.prepare({
      restorationId: `${owner}:${historyState.entryId}`,
      restoredPosition: historyState.positions.find(p => p.key === `thread:${snapshot.thread?.rootMessageId}`)?.position,
      key: snapshot.thread ? `thread:${snapshot.thread.rootMessageId}` : null,
      messageIds: threadMessageIds,
      loading: host.threadLoad().loading,
      revealMessageId: scrollIntent.threadRevealMessageId
    });
    const compactContext = snapshot.activeChannel?.is_direct ? "Direkte" : snapshot.activeChannel?.circle_id
      ? snapshot.circles.find(circle => circle.id === snapshot.activeChannel?.circle_id)?.name ?? "Vennekrets"
      : snapshot.activeChannel ? "Felles" : undefined;
    const backToConversations = () => { host.navigateView({ kind: "directory", scope: snapshot.activeChannel?.is_direct ? "direct" : "circles" }); view = "list"; update(); };
    return createConversationViewProps(snapshot, {
    runtime: host.runtime, theme: host.theme(), view,
    ownerId: owner ?? undefined,
    favorites: host.favorites,
    directoryScope: historyState.target?.kind === "directory" ? historyState.target.scope : undefined,
    onDirectoryScopeChange: scope => host.navigateView({ kind: "directory", scope }),
    recentChannelIds: historyState.recent.flatMap(item => item.target.kind === "conversation" ? [item.target.channelId] : []),
    header: <><AppUpdateNotice update={host.appUpdate} /><ReceivedShares target={host.shares} snapshot={snapshot} />{host.runtime.getSnapshot().session.reauthenticationRequired && <Status tone="error">
        Økta må stadfestast før Sprøyt kan halde fram. Utkasta dine blir lagra først. <Button onClick={host.reauthenticateNow}>Logg inn på nytt</Button>
      </Status>}
      {host.navigationNotice() && <Status tone="error">{host.navigationNotice()}</Status>}
      {host.hasLegacyDraft() && <Status>Eit eldre lokalt utkast manglar kontomerking. <Button onClick={host.recoverLegacyDraft}>Hent eldre utkast</Button></Status>}
      <HeaderActions primary={<><ViewHistoryControls history={host.viewHistory} snapshot={snapshot}
        onCapture={() => host.viewHistory.capture(captureUpdatePosition())} onOpen={host.openRecentView} />
        <PreviewInboxes key={owner ?? "signed-out"} state={host.inboxState()} host={host}
          activeKind={historyState.target?.kind === "inbox" ? historyState.target.inbox : null}
          onNavigate={inbox => host.navigateView({ kind: "inbox", inbox })}
          onBack={() => { if (host.viewHistory.getSnapshot().entryId === historyState.entryId && host.community.selfId() === owner) host.backFromManagement(); }} /></>}
        compactConversation={view === "detail" ? {
          context: compactContext, title: snapshot.title, onBack: backToConversations
        } : undefined}>
      {view === "detail" && <div className="sp-mobile-channel-menu"><Button variant="quiet" onClick={channelScroll.goToLatest}>Gå til siste</Button><ChannelActions compact snapshot={snapshot} host={host} /></div>}
      {explicitPreview && <Status>Førehandsvising for utvikling. Meldingar, vedlegg, trådar og reaksjonar er tilgjengelege her.</Status>}
      <Button onClick={host.cycleTheme}>Byt tema</Button><a href="/auth/logout">Logg ut</a>{fullInterface()}
      <ViewHistoryControls inMenu history={host.viewHistory} snapshot={snapshot}
        onCapture={() => host.viewHistory.capture(captureUpdatePosition())} onOpen={host.openRecentView} />
      <Button onClick={() => host.setRenderMode(host.renderMode() === "raw" ? "view" : "raw")}>{host.renderMode() === "raw" ? "Vis formatert" : "Vis råtekst"}</Button>
      <PreviewManagement snapshot={snapshot} capabilities={host.managementCapabilities()} settings={host.settings} advanced={host.advanced} chatAgents={host.chatAgents} appUpdate={host.appUpdate} captureUpdatePosition={() => {
        if (host.community.selfId() !== owner) throw new Error("Kontoen vart endra. Start oppdateringa på nytt.");
        const positions = captureUpdatePosition(), current = host.snapshot();
        if (current.selection.channelId) host.navigateView({ kind: "conversation", channelId: current.selection.channelId,
          ...(current.thread ? { threadId: current.thread.rootMessageId } : {}) }, "replace");
        return positions;
      }}
        target={host.managementTarget()} onNavigate={page => {
          if (host.viewHistory.getSnapshot().entryId !== historyState.entryId || host.community.selfId() !== owner) return;
          if (page) host.navigateView({ kind: "management", page }); else host.backFromManagement();
        }}
        onBack={() => { if (host.viewHistory.getSnapshot().entryId === historyState.entryId && host.community.selfId() === owner) host.backFromManagement(); }}
        onSelectChannel={channelId => { if (host.viewHistory.getSnapshot().entryId === historyState.entryId && host.community.selfId() === owner) host.navigateView({ kind: "conversation", channelId }); }}
        community={{ ...host.community, renderIntegration: channelId => <PreviewGrafana key={channelId} host={host.advanced} channelId={channelId} /> }} /></HeaderActions></>,
    navigationActions: <NewDirectMessage snapshot={snapshot} host={host.community}
      onOpen={() => host.navigateView({ kind: "management", page: { kind: "community", destination: { kind: "people" } } })}
      onStarted={() => { view = "detail"; update(); }} />,
    renderGroupActions: group => group.id === "scope:direct" ? null : <NavigationScopeActions
      key={`${host.community.selfId() ?? ""}:${group.id}`} group={group} snapshot={snapshot} host={host.community}
      onNavigate={page => host.navigateView({ kind: "management", page })}
      onSelect={channelId => { reactionPicker.close(); host.navigateView({ kind: "conversation", channelId }); view = "detail"; update(); }} />,
    contextActions: <><Button variant="quiet" onClick={channelScroll.goToLatest}>Gå til siste</Button><ChannelActions snapshot={snapshot} host={host} /></>,
    renderConversationAction: conversation => conversation.notifications ? <ChannelNotificationControl
      channelId={conversation.id} channelName={conversation.channel.name}
      enabled={conversation.notifications.enabled} pending={conversation.notifications.pending}
      error={conversation.notifications.error} onChange={host.setChannelNotifications} /> : null,
    onBack: backToConversations,
    onQueryChange(query) { host.search(query); update(); },
    onSelect(channelId) { reactionPicker.close(); host.navigateView({ kind: "conversation", channelId,
      ...(channelId === snapshot.selection.channelId && snapshot.thread ? { threadId: snapshot.thread.rootMessageId } : {}) }); view = "detail"; update(); },
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
      onRetry: host.retryHistory,
      unreadAfterSequence: channelScroll.unreadAfterSequence(),
      viewportRef: channelScroll.viewportRef,
      onScroll: channelScroll.onScroll
    },
    message: {
      formatTime: sentAt => new Date(sentAt).toLocaleTimeString(["nn-NO", "nb-NO"], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" }),
      formatAuthor: message => host.isOwnMessage(message) ? "Du" : message.sender_display_name,
      earlyAdopter: message => Boolean(host.settings.profileFor?.(message.sender_id)?.early_adopter),
      renderAuthorStatus: message => <MessageProfileStatus host={host.settings} userId={message.sender_id} own={host.isOwnMessage(message)} />,
      messageStatus: host.messageStatus,
      onReactionRequest: reactionPicker.open,
      renderActions: (message, context) => {
        if (message.deleted_at) return !context?.threadParent && snapshot.timeline.retainedDeletedRootIds.includes(message.id)
          ? <Button variant="quiet" data-thread-trigger={message.id} onClick={() => { host.openThread(message.id); update(); }}>Opne tråd</Button>
          : null;
        const replies = snapshot.threadSummaries.find(summary => summary.root_message_id === message.id)?.reply_count ?? 0;
        const threadAction = message.parent_message_id === null && !context?.threadParent ? <Button className="sp-message-symbol" variant="quiet"
          aria-label={replies ? `${replies} svar i tråd` : "Svar i tråd"} title={replies ? `${replies} svar i tråd` : "Svar i tråd"}
          data-thread-trigger={message.id} aria-expanded={snapshot.thread?.rootMessageId === message.id}
          onPointerDown={event => { if (event.pointerType === "mouse" && event.button === 0) event.preventDefault(); }}
          onClick={() => { host.openThread(message.id); update(); }}><span aria-hidden="true">↩</span>{replies > 0 && <span>{replies}</span>}</Button> : null;
        return <MessageActions message={message} host={host} openReaction={reactionPicker.open}
          threadAction={threadAction} />;
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
  return { update, capturePositions: captureUpdatePosition, unmount: close };
}
