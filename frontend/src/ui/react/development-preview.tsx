import { Button, Dialog, Status, TextField } from "@sproyt/ui/react";
import { createPortal } from "react-dom";
import channelActionsCss from "../channel-actions-popover.css";
import { useEffect, useId, useRef, useState, type PointerEvent } from "react";
import type { ConversationSnapshot } from "../../application/conversation-snapshot";
import type { ApplicationRuntime } from "../../application/runtime";
import { installSproytStyles, type SproytThemeMode } from "../design-system";
import { createConversationViewProps } from "./host-adapter";
import { messagesForTimeline } from "./conversation-view";
import type { ComposerTarget } from "./host-adapter";
import { mountConversationView } from "./mount";
import { createPreviewReactionPicker, PreviewMessageReactions, PreviewReactionActions } from "./preview-reactions";
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
import { GettingStarted } from "./getting-started";
import type { CircleChatAgentApi } from "../../chat-agents";
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
import { ConnectionHealth, type ConnectionHealthState } from "./connection-health";
import { DeviceSetupReminders } from "./device-setup-reminders";
import type { deviceInstall } from "../../device-install";
import type { DevicePushStatus } from "../../device-push";
import type { ThreadPreviewApi } from "../../thread-previews";
import type { AgentPlaceLookupApi } from "../../agent-place-lookup";
import { AgentPlaceDiscovery } from "./agent-place-discovery";
import { AgentPlaceCard } from "./agent-place-card";
import type { AgentGeoapifyRollApi } from "../../agent-geoapify-roll";
import { AgentGeoapifyCard } from "./agent-geoapify-card";

const PLACE_LOOKUP_MARKER = "Opne stadssøket for ferske barforslag.";

interface DevelopmentPreviewHost extends PreviewReactionHost, PreviewComposerHost, PreviewInboxHost {
  readonly retryThreadSummaries: () => void;
  readonly threadPreviews: ThreadPreviewApi;
  readonly agentPlaceLookup: AgentPlaceLookupApi;
  readonly agentGeoapifyRoll: AgentGeoapifyRollApi;
  readonly accountEpoch: () => number;
  readonly deviceInstall: typeof deviceInstall;
  readonly checkDevicePush: (owner: string, admitted: () => boolean) => Promise<DevicePushStatus>;
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
  readonly retryDirectory: () => void;
  readonly retryConnection: () => Promise<void>;
  readonly connectionHealth: () => ConnectionHealthState;
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

function ThreadDirectoryTrigger({ snapshot, onOpen, compact = false }: {
  snapshot: ConversationSnapshot; onOpen: () => void; compact?: boolean;
}) {
  if (!snapshot.activeChannel) return null;
  const unread = snapshot.threadSummaryStatus.ready
    ? snapshot.threadSummaries.reduce((sum, item) => sum + item.unread_count, 0) : 0;
  const label = unread ? `Trådar i samtalen, ${unread} uleste svar` : "Trådar i samtalen";
  return <Button variant="quiet" className={compact ? "sp-mobile-channel-menu-entry" : "sp-context-menu-trigger sp-message-symbol"}
    aria-label={label} title={label} onClick={onOpen}>
    <svg aria-hidden="true" viewBox="0 0 24 24" style={{ width: "1.2rem", height: "1.2rem", fill: "none", stroke: "currentColor", strokeWidth: 1.6, strokeLinecap: "round", strokeLinejoin: "round" }}>
      <path d="M4 4h13a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H9l-4 3v-3a2 2 0 0 1-2-2V6a2 2 0 0 1 1-2Z" />
      <path d="M8 9h7M8 12h5" />
    </svg>{compact && <span>Trådar i samtalen</span>}{unread > 0 && <span aria-hidden="true">{unread > 99 ? "99+" : unread}</span>}
  </Button>;
}

function ThreadIcon() {
  return <svg className="sp-thread-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
    <path d="M4 5.5h16v11H9l-5 3v-14Z" />
    <path d="M8 9h8M8 12.5h5" />
  </svg>;
}

function ProcessPilotMenuEntry({ api, channelId, onOpen }: { api: ProcessPilotApi; channelId: string; onOpen(): void }) {
  const [available, setAvailable] = useState(false);
  const [error, setError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setAvailable(false); setError(false);
    void api.configuration(channelId, controller.signal).then(configuration => {
      if (!controller.signal.aborted) setAvailable(configuration.can_configure || configuration.can_start);
    }).catch(() => { if (!controller.signal.aborted) setError(true); });
    return () => controller.abort();
  }, [api, channelId, attempt]);
  return available ? <Button variant="quiet" onClick={onOpen}>Prosesspilot</Button>
    : error ? <Button variant="quiet" onClick={() => setAttempt(value => value + 1)}>Prøv å hente prosessval igjen</Button> : null;
}

function ChannelActions({ snapshot, host, compact = false, inlineOpen, onInlineClose, onDetails }: { readonly inlineOpen?: boolean; readonly onInlineClose?: () => void; readonly snapshot: ConversationSnapshot; readonly host: DevelopmentPreviewHost; readonly compact?: boolean;
  readonly onDetails: (channelId: string, focusMemberAction: boolean, section?: "about" | "members" | "agents" | "notifications") => void }) {
  const [open, setOpen] = useState(false);
  const [inviteOpen, setInviteOpen] = useState(false);
  const [locationOpen, setLocationOpen] = useState(false);
  const [processOpen, setProcessOpen] = useState(false);
  const [confirmLeave, setConfirmLeave] = useState(false);
  const [leaving, setLeaving] = useState(false);
  const [leaveError, setLeaveError] = useState("");
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const id = useId();
  useEffect(() => {
    if (inlineOpen === false) {
      const more = panel.current?.querySelector("details");
      if (more) more.open = false;
    }
  }, [inlineOpen]);
  const channel = snapshot.activeChannel;
  const owner = host.community.selfId();
  const scopeKey = (current: ConversationSnapshot, account: string | null) => {
    const selected = current.activeChannel;
    return JSON.stringify([account, selected?.id, selected?.role, selected?.kind, selected?.circle_id,
      current.circles.find(circle => circle.id === selected?.circle_id)?.role]);
  };
  const access = useRef({ key: "", epoch: 0, live: true });
  const observe = (key: string) => { if (access.current.key !== key) { access.current.key = key; access.current.epoch++; } };
  observe(scopeKey(snapshot, owner));
  useEffect(() => { access.current.live = true; return () => { access.current.live = false; access.current.epoch++; }; }, []);
  const hideMenu = () => { if (panel.current?.matches(":popover-open")) panel.current.hidePopover(); };
  useEffect(() => {
    hideMenu();
    setInviteOpen(false); setLocationOpen(false); setProcessOpen(false); setConfirmLeave(false); setLeaving(false); setLeaveError("");
  }, [owner, channel?.id, channel?.role]);
  useEffect(() => {
    if (!open) return;
    const dismiss = (event: Event) => {
      if (event.type === "scroll" && panel.current?.contains(event.target as Node)) return;
      hideMenu();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !panel.current?.matches(":popover-open")) return;
      event.preventDefault(); event.stopPropagation();
      panel.current.hidePopover(); trigger.current?.focus({ preventScroll: true });
    };
    document.addEventListener("scroll", dismiss, true);
    document.addEventListener("keydown", escape, true);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("scroll", dismiss, true);
      document.removeEventListener("keydown", escape, true);
      window.removeEventListener("resize", dismiss);
    };
  }, [open]);
  const dismiss = () => { if (inlineOpen !== undefined) onInlineClose?.(); else { hideMenu(); trigger.current?.focus({ preventScroll: true }); } };
  const toggle = () => {
    const button = trigger.current, menu = panel.current;
    if (!button || !menu) return;
    button.focus({ preventScroll: true });
    if (menu.matches(":popover-open")) { menu.hidePopover(); return; }
    const anchor = button.getBoundingClientRect();
    const width = Math.min(300, window.innerWidth - 16);
    menu.style.width = `${width}px`;
    menu.style.left = `${Math.max(8, Math.min(anchor.right - width, window.innerWidth - width - 8))}px`;
    menu.showPopover();
    menu.style.top = `${Math.max(8, Math.min(anchor.bottom + 4, window.innerHeight - menu.getBoundingClientRect().height - 8))}px`;
    menu.querySelector<HTMLButtonElement>("button")?.focus({ preventScroll: true });
  };
  const conversation = channel ? snapshot.groups.flatMap(group => group.conversations).find(item => item.id === channel.id) : undefined;
  if (!channel || channel.is_direct) return null;
  const mayManageAgents = !!owner && !!channel.circle_id && (["owner", "moderator"].includes(channel.role)
    || (channel.kind !== "private" && channel.role === "member" && snapshot.circles.some(circle =>
      circle.id === channel.circle_id && ["owner", "moderator"].includes(circle.role))));
  const openDetails = (section: "about" | "members" | "agents" | "notifications" = "about", focusMemberAction = false) => {
    dismiss(); onDetails(channel.id, focusMemberAction, section);
  };
  // WebKit suppresses native dialogs below hidden ancestors. Keep dialogs in
  // the visible theme root when their compact disclosure closes or changes size.
  const dialogHost = inlineOpen !== undefined ? panel.current?.closest(".sp-theme") : null;
  const dialogs = <>
    {processOpen && <Dialog open title={`Prosesspilot: ${channel.name}`} closeLabel="Lukk prosesspilot" onClose={() => setProcessOpen(false)}>
      <ProcessPilotChannelAction key={`${host.processPilotIdentity()}:${channel.id}`} api={host.processPilot} channelId={channel.id} />
    </Dialog>}
    {confirmLeave && <Dialog open title="Forlat kanalen" closeLabel="Lukk stadfestinga" onClose={() => setConfirmLeave(false)}>
      <p>Vil du forlate {channel.name}? Meldingane dine blir ståande.</p>
      <div className="sp-row"><Button onClick={() => setConfirmLeave(false)}>Avbryt</Button>
        <Button variant="danger" busy={leaving} onClick={() => {
          if (leaving || host.community.selfId() !== owner || host.snapshot().activeChannel?.id !== channel.id) return;
          const ticket = { key: access.current.key, epoch: access.current.epoch };
          const admitted = () => {
            observe(scopeKey(host.snapshot(), host.community.selfId()));
            return access.current.live && access.current.key === ticket.key && access.current.epoch === ticket.epoch;
          };
          if (!admitted()) return;
          setLeaving(true); setLeaveError("");
          void host.community.leaveChannel(channel.id).then(() => { if (admitted()) setConfirmLeave(false); }).catch(error => {
            if (admitted()) setLeaveError(error instanceof Error ? error.message : "Kunne ikkje forlate kanalen.");
          }).finally(() => { if (admitted()) setLeaving(false); });
        }}>Ja, forlat kanalen</Button></div>
      {leaveError && <Status tone="error">{leaveError}</Status>}
    </Dialog>}
    {locationOpen && <AgentLocationSharingDialog api={host.agentLocations} channelId={channel.id} channelName={channel.name}
      onClose={() => setLocationOpen(false)} />}
    {inviteOpen && channel.circle_id && <PreviewCommunity destination={{ kind: "invite", circleId: channel.circle_id }} snapshot={snapshot} host={host.community}
      onNavigate={() => {}} onClose={() => setInviteOpen(false)} />}
  </>;
  return <>
    <style>{channelActionsCss}</style>
    {!compact && <Button className="sp-context-menu-trigger sp-message-symbol" variant="quiet"
      aria-label={`Medlemmer i ${channel.name}`} title={`Medlemmer i ${channel.name}`} onClick={() => openDetails("members")}><ChannelMembersIcon /></Button>
    }{inlineOpen === undefined && <Button ref={trigger} className={compact ? "sp-mobile-channel-menu-entry" : "sp-context-menu-trigger sp-message-symbol"} variant="quiet"
      aria-label="Kanalval" title="Kanalval" aria-expanded={open} aria-controls={id} onClick={toggle}><span aria-hidden="true">⋮</span>{compact && <span>Kanalval</span>}</Button>}
    <div ref={panel} id={id} popover={inlineOpen === undefined ? "auto" : undefined} className={inlineOpen === undefined ? "sp-channel-actions-popover" : "sp-channel-actions-inline"} role="group" aria-label={`Kanalval: ${channel.name}`}
      onToggle={event => {
        const visible = event.newState === "open";
        setOpen(visible);
        if (!visible) { const more = panel.current?.querySelector("details"); if (more) more.open = false; }
      }}>
      <Button variant="quiet" onClick={() => openDetails("about")}><ChannelMembersIcon /><span>Kanaldetaljar</span></Button>
      {mayManageAgents && <Button variant="quiet" onClick={() => openDetails("agents")}>Agentar i kanalen</Button>}
      {["owner", "moderator"].includes(channel.role) && <Button variant="quiet" onClick={() => openDetails("members", true)}>
        <ChannelMembersIcon /><span>Legg til eller inviter kanalmedlem</span></Button>}
      {conversation?.notifications && <Button variant="quiet" onClick={() => openDetails("notifications")}>
        <BellIcon muted={!conversation.notifications.enabled} /><span>Kanalvarsling</span></Button>}
      <details className="sp-channel-actions-more"><summary>Fleire val</summary>
      {snapshot.circles.some(circle => circle.id === channel.circle_id && circle.role === "owner") && <Button variant="quiet"
        onClick={() => { dismiss(); setInviteOpen(true); }}>Inviter til kretsen</Button>}
      {(inlineOpen ?? open) && <ProcessPilotMenuEntry key={`${owner}:${channel.id}:${channel.role}:${snapshot.circles.find(circle => circle.id === channel.circle_id)?.role}`}
        api={host.processPilot} channelId={channel.id} onOpen={() => { dismiss(); setProcessOpen(true); }} />}
      {host.managementCapabilities().agent && ["owner", "moderator"].includes(channel.role) && <Button variant="quiet" onClick={() => {
        dismiss(); host.navigateView({ kind: "management", page: { kind: "advanced", feature: "agent", channelId: channel.id } });
      }}>Mellombels tilgang for ekstern agent</Button>}
      {host.managementCapabilities().heart && <Button variant="quiet" onClick={() => {
        dismiss(); host.navigateView({ kind: "management", page: { kind: "advanced", feature: "heart", channelId: channel.id } });
      }}>Prosessoppsett</Button>}
      <Button variant="quiet" onClick={() => { dismiss(); setLocationOpen(true); }}>Del posisjon med agent</Button>
      <Button variant="danger" onClick={() => { dismiss(); setLeaveError(""); setConfirmLeave(true); }}>Forlat kanalen</Button>
      </details>
    </div>
    {dialogHost ? createPortal(dialogs, dialogHost) : dialogs}
  </>;
}
function PreviewMessageContent({ host, message, place }: { readonly host: DevelopmentPreviewHost; readonly message: ChatMessage;
  readonly place?: { status: "pending" | "yes" | "no" | "error"; googleAvailable: boolean; geoapifyAvailable: boolean; owner: string; epoch: number; channelId: string; admitted: () => boolean; retry: () => void } }) {
  const workTask = workItemTaskId(message.body);
  if (workTask) return <WorkItemTaskMessage key={`${host.processPilotIdentity()}:${message.id}:${workTask}`} api={host.workItems} taskId={workTask} messageId={message.id} />;
  const workStatus = workItemStatusId(message.body);
  if (workStatus) return <WorkItemStatusMessage key={`${host.processPilotIdentity()}:${message.id}:${workStatus}`} api={host.workItems} itemId={workStatus} messageId={message.id} />;
  const taskId = processTaskId(message.body);
  if (taskId) return <ProcessTaskMessage key={`${host.processPilotIdentity()}:${message.id}:${taskId}`} api={host.processPilot} taskId={taskId} messageId={message.id} />;
  return <div className="sp-message-content">
    <MarkdownContent source={markdownTextFromMessage(message.body)} />
    {invitationTokensFromMessage(message.body).map((token, index) => <InvitationCard key={`${token}-${index}`} token={token} host={host.invitations} />)}
    {place?.status === "yes" && place.googleAvailable && <AgentPlaceCard key={`${place.epoch}:${place.owner}:${place.channelId}:${message.id}:${message.body}:${message.edited_at ?? ""}`}
      api={host.agentPlaceLookup} owner={place.owner} channelId={place.channelId} messageId={message.id} admitted={place.admitted} />}
    {place?.status === "yes" && place.geoapifyAvailable && <AgentGeoapifyCard key={`geo:${place.epoch}:${place.owner}:${place.channelId}:${message.id}:${message.body}:${message.edited_at ?? ""}`}
      api={host.agentGeoapifyRoll} owner={place.owner} channelId={place.channelId} messageId={message.id} admitted={place.admitted} />}
    {place?.status === "error" && message.body === PLACE_LOOKUP_MARKER && <Button variant="quiet" onClick={place.retry}>Prøv å hente stadssøket</Button>}
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

function MessageActions({ host, message, threadAction, openThread, threadHasReplies, openReaction }: {
  readonly host: DevelopmentPreviewHost; readonly message: ChatMessage;
  readonly threadAction: React.ReactNode;
  readonly openThread?: () => void;
  readonly threadHasReplies: boolean;
  readonly openReaction: (message: ChatMessage, anchor: HTMLElement) => void;
}) {
  const [apps, setApps] = useState<readonly WorkApplication[]>([]);
  const [linkOpen, setLinkOpen] = useState(false);
  const [linkCopied, setLinkCopied] = useState(false);
  useEffect(() => {
    if (!linkCopied) return;
    const timer = window.setTimeout(() => setLinkCopied(false), 3000);
    return () => window.clearTimeout(timer);
  }, [linkCopied]);
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
  return <div ref={availabilityAnchor} className="sp-message-action-host">
    <PreviewReactionActions message={message} host={host} open={openReaction} primaryAction={threadAction}
      overflowActions={close => <>
        {openThread && <Button variant="quiet" onClick={() => { close(); openThread(); }}>{threadHasReplies ? "Svar i tråd" : "Start tråd"}</Button>}
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
  let renderedOwnerEpoch = 0;
  let renderedEntry = "";
  let memberFocus: { owner: string; channelId: string; historyEntryId: string } | null = null;
  let threadFocus: { owner: string | null; messageId: string; element: HTMLElement } | null = null;
  const reactionPicker = createPreviewReactionPicker(host);
  const placeDiscovery = new AgentPlaceDiscovery(host.agentPlaceLookup, () => update());
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
    placeDiscovery.dispose();
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
      renderedOwnerEpoch++; memberFocus = null;
      renderedOwner = owner; channelScroll.reset(); threadScroll.reset(); resumedThread = false; renderedEntry = "";
      reactionPicker.close();
    }
    const historyState = host.viewHistory.getSnapshot();
    const ownerEpoch = renderedOwnerEpoch;
    if (memberFocus && (memberFocus.owner !== owner || memberFocus.historyEntryId !== historyState.entryId
      || historyState.target?.kind !== "management" || historyState.target.page.kind !== "community"
      || historyState.target.page.destination.kind !== "channel" || historyState.target.page.destination.channelId !== memberFocus.channelId)) memberFocus = null;
    const pageKey = owner && (historyState.target?.kind === "management" || historyState.target?.kind === "inbox")
      ? `${owner}:${historyState.entryId}` : undefined;
    const channelId = snapshot.activeChannel?.id ?? null;
    const accountEpoch = host.accountEpoch();
    const placeAdmitted = () => !disposed && Boolean(owner && channelId) && host.community.selfId() === owner
      && host.accountEpoch() === accountEpoch && host.snapshot().activeChannel?.id === channelId
      && host.snapshot().channels.some(channel => channel.id === channelId)
      && !["management", "inbox", "directory"].includes(host.viewHistory.getSnapshot().target?.kind ?? "");
    const placeCandidates = [...snapshot.timeline.messages, ...(snapshot.thread?.root ? [snapshot.thread.root] : []), ...(snapshot.thread?.replies ?? [])]
      .filter(message => !message.deleted_at && message.body === PLACE_LOOKUP_MARKER).map(message => ({ id: message.id, revision: JSON.stringify([message.body, message.edited_at ?? null]) }));
    channelScroll.setPaused(Boolean(pageKey));
    threadScroll.setPaused(Boolean(pageKey));
    if (!snapshot.activeChannel && !snapshot.channels.length && snapshot.directory.ready && historyState.target?.kind !== "conversation") view = "list";
    if (historyState.entryId !== renderedEntry) {
      renderedEntry = historyState.entryId;
      if (historyState.target?.kind === "directory") view = "list";
      else if (historyState.target?.kind === "conversation") view = "detail";
    }
    placeDiscovery.sync(!pageKey && view === "detail" ? owner : null, !pageKey && view === "detail" ? channelId : null,
      accountEpoch, placeCandidates, placeAdmitted);
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
    const compactContext = snapshot.activeChannel?.is_direct ? snapshot.activeChannel.direct_user_id ? "Direkte" : "Gruppesamtale" : snapshot.activeChannel?.circle_id
      ? snapshot.circles.find(circle => circle.id === snapshot.activeChannel?.circle_id)?.name ?? "Vennekrets"
      : snapshot.activeChannel ? "Felles" : undefined;
    const backToConversations = () => {
      const scope = snapshot.activeChannel?.is_direct ? "direct" : "circles";
      if (historyState.target?.kind !== "directory" || historyState.target.scope !== scope) host.navigateView({ kind: "directory", scope });
      view = "list"; update();
    };
    const openCommunity = () => host.navigateView({ kind: "management", page: { kind: "community", destination: { kind: "circles" } } });
    const newDirect = () => host.navigateView({ kind: "management", page: { kind: "community", destination: { kind: "people" } } });
    const openInvitation = () => host.navigateView({ kind: "management", page: { kind: "community", destination: { kind: "accept-invitation" } } });
    const openThreadDirectory = () => {
      const channelId = snapshot.activeChannel?.id;
      if (!channelId || !owner || disposed || host.community.selfId() !== owner
        || host.viewHistory.getSnapshot().entryId !== historyState.entryId || host.snapshot().activeChannel?.id !== channelId) return;
      host.navigateView({ kind: "management", page: { kind: "community", destination: { kind: "channel-threads", channelId } } });
    };
    const openMessageThread = (messageId: string, element?: HTMLElement) => {
      if (disposed || host.community.selfId() !== owner || host.snapshot().selection.channelId !== snapshot.selection.channelId) return;
      threadFocus = element ? { owner, messageId, element } : null;
      host.openThread(messageId); update();
    };
    const openDetails = (channelId: string, focusMemberAction: boolean, section: "about" | "members" | "agents" | "notifications" = "about") => {
      const admitted = () => !disposed && !!owner && host.community.selfId() === owner && renderedOwnerEpoch === ownerEpoch;
      if (!admitted() || host.viewHistory.getSnapshot().entryId !== historyState.entryId
        || host.snapshot().activeChannel?.id !== channelId) return;
      host.navigateView({ kind: "management", page: { kind: "community", destination: { kind: "channel", channelId, channelSection: section } } });
      const opened = host.viewHistory.getSnapshot();
      if (!admitted() || opened.target?.kind !== "management" || opened.target.page.kind !== "community"
        || opened.target.page.destination.kind !== "channel" || opened.target.page.destination.channelId !== channelId) return;
      memberFocus = focusMemberAction ? { owner: owner!, channelId, historyEntryId: opened.entryId } : null;
      update();
    };
    const channelNotificationsFor = (channelId: string) => {
      const admitted = () => {
        const currentRoute = host.viewHistory.getSnapshot();
        return !disposed && !!owner && host.community.selfId() === owner && renderedOwnerEpoch === ownerEpoch
          && currentRoute.entryId === historyState.entryId && currentRoute.target?.kind === "management"
          && currentRoute.target.page.kind === "community" && currentRoute.target.page.destination.kind === "channel"
          && currentRoute.target.page.destination.channelId === channelId;
      };
      if (!admitted()) return undefined;
      const notifications = snapshot.groups.flatMap(group => group.conversations).find(item => item.id === channelId)?.notifications;
      if (!notifications) return undefined;
      return { enabled: notifications.enabled, busy: notifications.pending, error: notifications.error, toggle: () => {
        if (!admitted()) return;
        const current = host.snapshot().groups.flatMap(group => group.conversations).find(item => item.id === channelId)?.notifications;
        if (!current || current.pending || !admitted()) return;
        host.setChannelNotifications(channelId, !current.enabled);
      } };
    };
    const directMembers = (compact = false) => snapshot.activeChannel?.is_direct ? <Button variant="quiet"
      className={compact ? "sp-mobile-channel-menu-entry" : "sp-context-menu-trigger sp-message-symbol"}
      aria-label={`Medlemmer i ${snapshot.title}`} title="Om samtalen" onClick={() => openDetails(snapshot.activeChannel!.id, false, "members")}>
      <ChannelMembersIcon />{compact && <span>Om samtalen</span>}</Button> : null;
    const showWelcome = !snapshot.activeChannel && !snapshot.channels.length && snapshot.directory.ready && !snapshot.directory.error
      && !new URL(window.location.href).searchParams.has("invite") && historyState.target?.kind !== "conversation";
    return createConversationViewProps(snapshot, {
    runtime: host.runtime, theme: host.theme(), view, pageKey,
    navigationFooter: <ViewHistoryControls inMenu history={host.viewHistory} snapshot={snapshot}
      onCapture={() => host.viewHistory.capture(captureUpdatePosition())} onOpen={host.openRecentView} />,
    onFindChannels: openCommunity, onInvitation: openInvitation,
    onGuide: () => host.navigateView({ kind: "management", page: { kind: "welcome" } }),
    emptyContent: !snapshot.activeChannel ? showWelcome ? <GettingStarted mode="welcome" onFindChannels={openCommunity}
      onNewDirect={newDirect} onInvitation={openInvitation} onProfile={() => host.navigateView({ kind: "management", page: { kind: "profile" } })} />
      : <div className="sp-directory-waiting"><Status tone={snapshot.directory.error ? "error" : "info"}>{snapshot.directory.error || (!snapshot.directory.ready ? "Hentar samtalane dine …" : "Vel ei samtale frå oversikta.")}</Status>
        {!snapshot.directory.ready && <Button variant="quiet" onClick={host.retryDirectory}>Prøv igjen</Button>}
        <Button variant="quiet" onClick={backToConversations}>Vis samtalar</Button></div> : undefined,
    ownerId: owner ?? undefined,
    favorites: host.favorites,
    directoryScope: historyState.target?.kind === "directory" ? historyState.target.scope : undefined,
    onDirectoryScopeChange: scope => host.navigateView({ kind: "directory", scope }),
    recentChannelIds: historyState.recent.flatMap(item => item.target.kind === "conversation" ? [item.target.channelId] : []),
    header: <><AppUpdateNotice update={host.appUpdate} /><ReceivedShares target={host.shares} snapshot={snapshot} />
      {owner && <DeviceSetupReminders key={owner} owner={owner}
        active={!pageKey && !host.appUpdate.getSnapshot().message && !host.navigationNotice()}
        connected={host.connectionHealth().phase === "connected"} install={host.deviceInstall} checkPush={host.checkDevicePush}
        openNotifications={() => {
          if (!disposed && host.community.selfId() === owner && host.viewHistory.getSnapshot().entryId === historyState.entryId)
            host.navigateView({ kind: "management", page: { kind: "notifications" } });
        }} />}
      {host.navigationNotice() && <Status tone="error">{host.navigationNotice()}</Status>}
      {host.hasLegacyDraft() && <Status>Eit eldre lokalt utkast manglar kontomerking. <Button onClick={host.recoverLegacyDraft}>Hent eldre utkast</Button></Status>}
      <HeaderActions navigationKey={historyState.entryId} connection={host.connectionHealth()}
        onConversations={backToConversations}
        onLatest={channelScroll.goToLatest}
        onSettings={() => host.navigateView({ kind: "management", page: { kind: "overview" } })}
        primary={<><ViewHistoryControls history={host.viewHistory} snapshot={snapshot}
        onCapture={() => host.viewHistory.capture(captureUpdatePosition())} onOpen={host.openRecentView} />
        <PreviewInboxes key={owner ?? "signed-out"} state={host.inboxState()} host={host}
          activeKind={historyState.target?.kind === "inbox" ? historyState.target.inbox : null}
          onNavigate={inbox => host.navigateView({ kind: "inbox", inbox })}
          onBack={() => { if (host.viewHistory.getSnapshot().entryId === historyState.entryId && host.community.selfId() === owner) host.backFromManagement(); }} /></>}
        compactActions={(open, close) => <>
          <ThreadDirectoryTrigger compact snapshot={snapshot} onOpen={() => { close(); openThreadDirectory(); }} />
          {snapshot.activeChannel?.is_direct ? directMembers(true) : <ChannelActions compact inlineOpen={open} onInlineClose={close}
            snapshot={snapshot} host={host} onDetails={openDetails} />}
        </>}
        compactConversation={view === "detail" && snapshot.activeChannel && !pageKey ? {
          context: compactContext, title: snapshot.title, onBack: backToConversations,
          unreadThreadReplies: snapshot.threadSummaryStatus.ready ? snapshot.threadSummaries.reduce((sum, item) => sum + item.unread_count, 0) : 0
        } : undefined}>
      {explicitPreview && <Status>Førehandsvising for utvikling. Meldingar, vedlegg, trådar og reaksjonar er tilgjengelege her.</Status>}
      <PreviewManagement channelNotificationsFor={channelNotificationsFor} hideTrigger snapshot={snapshot} onRetryDirectory={host.retryDirectory} capabilities={host.managementCapabilities()} settings={host.settings} advanced={host.advanced} chatAgents={host.chatAgents} appUpdate={host.appUpdate}
        threadPreviews={host.threadPreviews} onRetryThreadSummaries={host.retryThreadSummaries}
        onOpenThread={(channelId, rootId) => {
          if (disposed || !owner || host.community.selfId() !== owner || host.viewHistory.getSnapshot().entryId !== historyState.entryId
            || host.snapshot().activeChannel?.id !== channelId) return;
          host.navigateView({ kind: "conversation", channelId, threadId: rootId });
        }}
        focusMemberAction={!!memberFocus && memberFocus.owner === owner && memberFocus.historyEntryId === historyState.entryId
          && historyState.target?.kind === "management" && historyState.target.page.kind === "community"
          && historyState.target.page.destination.kind === "channel" && historyState.target.page.destination.channelId === memberFocus.channelId}
        appearanceContent={<><p className="sp-help">Desse visingsvala gjeld denne nettlesaren.</p>
          <section className="sp-settings-group"><h2>Tema</h2><p>Gjeldande tema: {host.theme() === "system" ? "Følg systemet" : host.theme() === "dark" ? "Mørkt" : "Lyst"}.</p><Button onClick={host.cycleTheme}>Byt tema</Button></section>
          <section className="sp-settings-group"><h2>Meldingsvising</h2><Button onClick={() => host.setRenderMode(host.renderMode() === "raw" ? "view" : "raw")}>{host.renderMode() === "raw" ? "Vis formatert" : "Vis råtekst"}</Button></section></>}
        helpContent={<><ConnectionHealth key={owner ?? "signed-out"} details state={host.connectionHealth()} onRetry={host.retryConnection}
          onLogin={host.reauthenticateNow} onReload={() => { void host.appUpdate.run(captureUpdatePosition); }} />{explicitPreview && <section><h2>Utviklarførehandsvising</h2>{fullInterface()}</section>}</>}
        captureUpdatePosition={() => {
        if (host.community.selfId() !== owner) throw new Error("Kontoen vart endra. Start oppdateringa på nytt.");
        const positions = captureUpdatePosition(), current = host.snapshot();
        if (current.selection.channelId) host.navigateView({ kind: "conversation", channelId: current.selection.channelId,
          ...(current.thread ? { threadId: current.thread.rootMessageId } : {}) }, "replace");
        return positions;
      }}
        target={host.managementTarget()} onNavigate={(page, mode) => {
          if (host.viewHistory.getSnapshot().entryId !== historyState.entryId || host.community.selfId() !== owner) return;
          if (page) host.navigateView({ kind: "management", page }, mode); else host.backFromManagement();
        }}
        onBack={() => { if (host.viewHistory.getSnapshot().entryId === historyState.entryId && host.community.selfId() === owner) host.backFromManagement(); }}
        onSelectChannel={channelId => { if (host.viewHistory.getSnapshot().entryId === historyState.entryId && host.community.selfId() === owner) host.navigateView({ kind: "conversation", channelId }); }}
        community={{ ...host.community, renderIntegration: channelId => <PreviewGrafana key={channelId} host={host.advanced} channelId={channelId} /> }} /></HeaderActions>
      {!(historyState.target?.kind === "management" && historyState.target.page.kind === "help") && <ConnectionHealth key={`health:${owner ?? "signed-out"}`} state={host.connectionHealth()}
        onRetry={host.retryConnection} onLogin={host.reauthenticateNow} onReload={() => { void host.appUpdate.run(captureUpdatePosition); }} />}</>,
    navigationActions: <NewDirectMessage snapshot={snapshot} host={host.community}
      onOpen={() => host.navigateView({ kind: "management", page: { kind: "community", destination: { kind: "people" } } })}
      onStarted={() => { view = "detail"; update(); }} />,
    renderGroupActions: group => group.id === "scope:direct" ? null : <NavigationScopeActions
      key={`${host.community.selfId() ?? ""}:${group.id}`} group={group} snapshot={snapshot} host={host.community}
      onNavigate={page => host.navigateView({ kind: "management", page })}
      onSelect={channelId => { reactionPicker.close(); host.navigateView({ kind: "conversation", channelId }); view = "detail"; update(); }} />,
    contextActions: <><Button variant="quiet" onClick={channelScroll.goToLatest}>Gå til siste</Button><ThreadDirectoryTrigger snapshot={snapshot} onOpen={openThreadDirectory} />{directMembers()}<ChannelActions snapshot={snapshot} host={host} onDetails={openDetails} /></>,
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
        if (disposed || !rootId || host.community.selfId() !== owner) return;
        const previous = threadFocus?.owner === owner && threadFocus.messageId === rootId ? threadFocus.element : null;
        const target = previous?.isConnected && previous.getClientRects().length ? previous
          : container.querySelector<HTMLElement>(`.sp-channel-pane [data-thread-trigger="${CSS.escape(rootId)}"]`);
        target?.focus({ preventScroll: true });
        threadFocus = null;
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
      renderReactions: message => <PreviewMessageReactions message={message} host={host} />,
      onReactionRequest: reactionPicker.open,
      renderActions: (message, context) => {
        if (message.deleted_at) return !context?.threadParent && snapshot.timeline.retainedDeletedRootIds.includes(message.id)
          ? <Button variant="quiet" data-thread-trigger={message.id} onClick={event => openMessageThread(message.id, event.currentTarget)}>Opne tråd</Button>
          : null;
        const replies = snapshot.threadSummaries.find(summary => summary.root_message_id === message.id)?.reply_count ?? 0;
        const isRoot = message.parent_message_id === null && !context?.threadParent;
        const threadAction = isRoot ? <Button className="sp-message-symbol" variant="quiet"
          aria-label="Svar i tråd" title="Svar i tråd"
          data-thread-trigger={message.id} aria-expanded={snapshot.thread?.rootMessageId === message.id}
          onPointerDown={event => { if (event.pointerType === "mouse" && event.button === 0) event.preventDefault(); }}
          onClick={event => openMessageThread(message.id, event.currentTarget)}><ThreadIcon /></Button> : null;
        return <MessageActions message={message} host={host} openReaction={reactionPicker.open}
          threadAction={threadAction} threadHasReplies={replies > 0} openThread={isRoot ? () => openMessageThread(message.id) : undefined} />;
      },
      renderThreadSummary: message => {
        if (message.parent_message_id !== null) return null;
        const summary = snapshot.threadSummaries.find(item => item.root_message_id === message.id);
        if (!summary?.reply_count) return null;
        // Refreshing after a deletion temporarily invalidates the old counts.
        const ready = snapshot.threadSummaryStatus.ready;
        const countLabel = summary.reply_count === 1 ? "1 svar" : `${summary.reply_count} svar`;
        const unreadLabel = summary.unread_count === 1 ? "1 ulest" : `${summary.unread_count} uleste`;
        const label = ready ? `Opne tråd, ${countLabel}${summary.unread_count ? `, ${unreadLabel}` : ""}` : "Opne tråd";
        return <Button className="sp-thread-summary" variant="quiet" data-thread-trigger={message.deleted_at ? message.id : undefined}
          aria-label={label} aria-expanded={snapshot.thread?.rootMessageId === message.id}
          onPointerDown={event => { if (event.pointerType === "mouse" && event.button === 0) event.preventDefault(); }}
          onClick={event => openMessageThread(message.id, event.currentTarget)}><ThreadIcon /><span>{ready ? countLabel : "Opne tråd"}</span>
          {ready && summary.unread_count > 0 && <span className="sp-thread-unread">{unreadLabel}</span>}</Button>;
      },
      // React owns all visible message content, including Markdown, Mermaid,
      // media and invitation cards. The host supplies state and commands only.
      renderContent: message => host.renderMode() === "raw"
        ? <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{message.body}</pre>
        : <><PreviewMessageContent host={host} message={message} place={owner && channelId && !pageKey && view === "detail"
          ? { status: placeDiscovery.status(message.id) ?? "pending", googleAvailable: placeDiscovery.googleAvailable(message.id), geoapifyAvailable: placeDiscovery.geoapifyAvailable(message.id), owner, epoch: accountEpoch, channelId,
            admitted: placeAdmitted, retry: () => placeDiscovery.retry(message.id) } : undefined} /><PreviewMediaContent body={message.body} mediaOnly /></>
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
