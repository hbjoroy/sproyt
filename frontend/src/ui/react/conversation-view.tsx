import { useCallback, useEffect, useId, useRef, useSyncExternalStore } from "react";
import type { ReactNode, Ref } from "react";
import { AppShell, Button, ConversationList, Message, Status, TextField, Theme } from "@sproyt/ui/react";
import type { Conversation, ThemeMode } from "@sproyt/ui/react";
import type { ApplicationRuntime } from "../../application/runtime";
import type { ConversationTimelineItem } from "../../application/conversation-snapshot";
import type { ChatMessage } from "../../types";

/** IDs belong to the application. Two circles may have the same display name. */
export interface ConversationGroup {
  readonly id: string;
  readonly name: string;
  readonly conversations: readonly (Omit<Conversation, "group"> & { readonly action?: ReactNode })[];
  readonly actions?: ReactNode;
}

export interface ConversationNavigationProps {
  readonly groups: readonly ConversationGroup[];
  readonly selectedId: string | null;
  readonly query: string;
  readonly onQueryChange: (query: string) => void;
  readonly onSelect: (id: string) => void;
  readonly actions?: ReactNode;
}

/** Filtering, labels, membership and notification policy remain host-owned. */
export function ConversationNavigation(props: ConversationNavigationProps) {
  return <>
    <TextField label="Finn samtale" type="search" value={props.query}
      onChange={event => props.onQueryChange(event.currentTarget.value)} />
    {props.actions}
    {props.groups.map(group => <section key={group.id} data-conversation-group={group.id}>
      {group.actions}
      {group.conversations.some(item => item.action) ? <div className="sp-conversations">
        <h2 className="sp-group-label sp-kicker">{group.name}</h2>
        {group.conversations.map(item => <div className="sp-conversation-row" key={item.id}>
          <button type="button" className="sp-conversation"
            aria-current={item.id === props.selectedId ? "page" : undefined}
            onClick={() => props.onSelect(item.id)}>
            <span className="sp-conversation-name">{item.name}</span>
            {!!item.unread && <span className="sp-badge" aria-label={`${item.unread} uleste`}>{item.unread}</span>}
          </button>
          <div className="sp-conversation-actions">{item.action}</div>
        </div>)}
      </div> : <ConversationList items={group.conversations.map(item => ({ ...item, group: group.name }))}
        selectedId={props.selectedId ?? ""} onSelect={props.onSelect}
        emptyLabel={`Ingen samtalar i ${group.name}.`} />}
    </section>)}
    {props.groups.length === 0 && <Status>Ingen samtalar funne.</Status>}
  </>;
}

export interface TimelineProps {
  readonly channelId: string | null;
  readonly parentMessageId?: string | null;
  readonly messages: readonly ChatMessage[];
  /** Ordered host timeline. Keep system notices at their original position. */
  readonly items?: readonly ConversationTimelineItem[];
  readonly loading?: boolean;
  readonly error?: string;
  readonly onRetry?: () => void;
  readonly hasOlder?: boolean;
  readonly onLoadOlder?: () => void;
  /** Host keeps scroll/read policy, including media resize and older-page anchoring. */
  readonly viewportRef?: Ref<HTMLDivElement>;
  readonly onScroll?: () => void;
  readonly notices?: readonly string[];
  readonly formatTime: (sentAt: string) => string;
  readonly formatAuthor?: (message: ChatMessage) => string;
  /** Marks public first-50 members without exposing their private signup number. */
  readonly earlyAdopter?: (message: ChatMessage) => boolean;
  /** Render safe React-owned message content. */
  readonly renderContent: (message: ChatMessage) => ReactNode;
  /** Permission checks, replies, reaction counts, edit and delete stay with the host. */
  readonly renderActions?: (message: ChatMessage, context?: { readonly threadParent: boolean }) => ReactNode;
  readonly messageStatus?: (message: ChatMessage) => string | undefined;
  readonly onReactionRequest?: (message: ChatMessage, anchor: HTMLElement) => void;
}

export function messagesForTimeline(props: Pick<TimelineProps, "messages" | "channelId" | "parentMessageId">): ChatMessage[] {
  const parentId = props.parentMessageId ?? null;
  return props.messages.filter(message => message.channel_id === props.channelId
    && message.parent_message_id === parentId).sort((a, b) => a.sequence - b.sequence);
}

export function ConversationTimeline(props: TimelineProps) {
  const messages = messagesForTimeline(props);
  const entries: readonly ConversationTimelineItem[] = props.items ?? [
    ...(props.notices ?? []).map(text => ({ type: "system" as const, text })),
    ...messages.map(message => ({ type: "message" as const, message }))
  ];
  const visibleEntries = entries.filter(item => item.type === "system"
    || (item.message.channel_id === props.channelId
      && item.message.parent_message_id === (props.parentMessageId ?? null)));
  return <div className="sp-timeline" ref={props.viewportRef} onScroll={props.onScroll}
    aria-label={props.parentMessageId ? "Svar i tråden" : "Meldingar"} aria-busy={props.loading || undefined}>
    {props.hasOlder && props.onLoadOlder && <Button busy={props.loading} onClick={props.onLoadOlder}>Last eldre meldingar</Button>}
    {props.error && <Status tone="error">{props.error}{props.onRetry && <Button onClick={props.onRetry}>Prøv igjen</Button>}</Status>}
    {props.loading && <Status>Lastar meldingar …</Status>}
    {!props.loading && !props.error && visibleEntries.length === 0 && <Status>Ingen meldingar enno.</Status>}
    {visibleEntries.map((item, index) => item.type === "system"
      ? <Status key={`notice-${index}`}>{item.text}</Status>
      : <ConversationMessage key={item.message.id} message={item.message} {...props}
          dateLabel={index === 0 || (() => {
            const previous = visibleEntries.slice(0, index).reverse().find(entry => entry.type === "message");
            return previous?.type !== "message" || new Date(previous.message.sent_at).toLocaleDateString(["nn-NO", "nb-NO"])
              !== new Date(item.message.sent_at).toLocaleDateString(["nn-NO", "nb-NO"]);
          })() ? new Date(item.message.sent_at).toLocaleDateString(["nn-NO", "nb-NO"], { day: "numeric", month: "long", year: "numeric" }) : undefined} />)}
  </div>;
}

export type MessagePresentation = Pick<TimelineProps, "formatTime" | "formatAuthor" | "earlyAdopter" | "renderContent" | "renderActions" | "messageStatus" | "onReactionRequest">;

export function formatMessageDateTime(sentAt: string): string {
  return new Date(sentAt).toLocaleString(["nn-NO", "nb-NO"], { dateStyle: "full", timeStyle: "medium" });
}

/** The thread parent uses the same safe rendering and permission policy as replies. */
export function ConversationMessage(props: MessagePresentation & { readonly message: ChatMessage; readonly threadParent?: boolean; readonly dateLabel?: string }) {
  const message = props.message;
  const wrapper = useRef<HTMLDivElement>(null);
  const latestMessage = useRef(message);
  latestMessage.current = message;
  const fullTimestamp = formatMessageDateTime(message.sent_at);
  const timestampTooltipId = `message-time-${message.id}`;
  const earlyAdopter = props.earlyAdopter?.(message) ?? false;
  const earlyAdopterTooltipId = `early-adopter-${message.id}`;
  const requestReaction = props.onReactionRequest;
  const deliveryStatus = props.messageStatus?.(message);
  const visibleStatus = deliveryStatus === "Sendt" ? undefined : deliveryStatus?.replace(/^Sendt · /, "");
  const onReactionRequest = useCallback((anchor: HTMLElement) => {
    if (!latestMessage.current.deleted_at) requestReaction?.(latestMessage.current, anchor);
  }, [requestReaction]);
  useEffect(() => {
    const time = wrapper.current?.querySelector("time");
    if (time) {
      time.title = fullTimestamp;
      time.tabIndex = 0;
      time.setAttribute("role", "button");
      time.setAttribute("aria-label", `Sendt ${fullTimestamp}. Trykk for å vise eller skjule tidspunktet.`);
      time.setAttribute("aria-describedby", timestampTooltipId);
      time.setAttribute("aria-expanded", "false");
      // The message surface owns a long-press reaction gesture. Keep a tap on
      // its timestamp from starting that timer; the timestamp has its own action.
      const keepTimestampGesture = (event: Event) => event.stopPropagation();
      let ignoreClickAfterTouch = false;
      let openedFromTouch = false;
      const toggleTimestamp = () => {
        const owner = wrapper.current;
        if (!owner) return;
        const open = owner.dataset.timeOpen !== "true";
        if (open) owner.dataset.timeOpen = "true";
        else {
          delete owner.dataset.timeOpen;
          time.blur();
          openedFromTouch = false;
        }
        time.setAttribute("aria-expanded", String(open));
      };
      const toggleTimestampFromTouch = (event: Event) => {
        event.preventDefault();
        event.stopPropagation();
        ignoreClickAfterTouch = true;
        openedFromTouch = true;
        time.focus({ preventScroll: true });
        toggleTimestamp();
      };
      const toggleTimestampFromClick = () => {
        if (ignoreClickAfterTouch) {
          ignoreClickAfterTouch = false;
          return;
        }
        openedFromTouch = false;
        toggleTimestamp();
      };
      const toggleTimestampFromKeyboard = (event: Event) => {
        if (!(event instanceof KeyboardEvent) || (event.key !== "Enter" && event.key !== " ")) return;
        event.preventDefault();
        openedFromTouch = false;
        toggleTimestamp();
      };
      const closeTimestamp = () => {
        if (wrapper.current) delete wrapper.current.dataset.timeOpen;
        time.setAttribute("aria-expanded", "false");
      };
      const closeTimestampAfterFocus = () => {
        if (!openedFromTouch) closeTimestamp();
      };
      const closeTimestampFromOutside = (event: Event) => {
        if (event.target instanceof Node && time.contains(event.target)) return;
        openedFromTouch = false;
        closeTimestamp();
      };
      time.addEventListener("pointerdown", keepTimestampGesture);
      time.addEventListener("touchend", toggleTimestampFromTouch);
      time.addEventListener("click", toggleTimestampFromClick);
      time.addEventListener("keydown", toggleTimestampFromKeyboard);
      time.addEventListener("blur", closeTimestampAfterFocus);
      document.addEventListener("pointerdown", closeTimestampFromOutside, true);
      return () => {
        time.removeEventListener("pointerdown", keepTimestampGesture);
        time.removeEventListener("touchend", toggleTimestampFromTouch);
        time.removeEventListener("click", toggleTimestampFromClick);
        time.removeEventListener("keydown", toggleTimestampFromKeyboard);
        time.removeEventListener("blur", closeTimestampAfterFocus);
        document.removeEventListener("pointerdown", closeTimestampFromOutside, true);
      };
    }
  }, [fullTimestamp, timestampTooltipId]);
  useEffect(() => {
    const author = wrapper.current?.querySelector(".sp-message-meta strong");
    if (!(author instanceof HTMLElement)) return;
    author.title = earlyAdopter ? "Blant dei første 50 på Sprøyt" : author.textContent ?? "";
    if (!earlyAdopter) return;
    author.tabIndex = 0;
    author.setAttribute("role", "button");
    author.setAttribute("aria-describedby", earlyAdopterTooltipId);
    author.setAttribute("aria-expanded", "false");
    const stopGesture = (event: Event) => event.stopPropagation();
    const toggle = (event: Event) => {
      if (event instanceof KeyboardEvent && event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault(); event.stopPropagation();
      const owner = wrapper.current;
      if (!owner) return;
      const open = owner.dataset.earlyAdopterOpen !== "true";
      if (open) owner.dataset.earlyAdopterOpen = "true";
      else delete owner.dataset.earlyAdopterOpen;
      author.setAttribute("aria-expanded", String(open));
    };
    const close = (event: Event) => {
      if (event.target instanceof Node && author.contains(event.target)) return;
      if (wrapper.current) delete wrapper.current.dataset.earlyAdopterOpen;
      author.setAttribute("aria-expanded", "false");
    };
    author.addEventListener("pointerdown", stopGesture);
    author.addEventListener("click", toggle);
    author.addEventListener("keydown", toggle);
    document.addEventListener("pointerdown", close, true);
    return () => {
      author.removeEventListener("pointerdown", stopGesture);
      author.removeEventListener("click", toggle);
      author.removeEventListener("keydown", toggle);
      document.removeEventListener("pointerdown", close, true);
    };
  }, [earlyAdopter, earlyAdopterTooltipId, message.sender_display_name, props.formatAuthor]);
  const author = props.formatAuthor?.(message) ?? message.sender_display_name;
  return <div ref={wrapper} data-message-id={message.id} data-date-start={props.dateLabel ? "true" : undefined}>
    {props.dateLabel && <div className="sp-date sp-kicker">{props.dateLabel}</div>}
    <Message author={author} dateTime={message.sent_at}
        time={props.formatTime(message.sent_at)}
        status={message.deleted_at ? "Sletta" : visibleStatus ?? (message.edited_at ? "Redigert" : undefined)}
        actions={props.renderActions?.(message, { threadParent: Boolean(props.threadParent) })}
        onReactionRequest={!message.deleted_at && props.onReactionRequest
          ? onReactionRequest : undefined}>
        {message.deleted_at ? <p>Meldinga er sletta.</p> : props.renderContent(message)}
    </Message>
    <span id={timestampTooltipId} className="sp-message-time-tooltip" role="tooltip">{fullTimestamp}</span>
    {earlyAdopter && <span id={earlyAdopterTooltipId} className="sp-early-adopter-tooltip" role="tooltip">Blant dei første 50 på Sprøyt</span>}
  </div>;
}

export interface ConversationViewProps {
  readonly runtime: Pick<ApplicationRuntime, "getSnapshot" | "subscribe">;
  readonly theme: ThemeMode;
  readonly view: "list" | "detail";
  readonly header: ReactNode;
  readonly navigation: ConversationNavigationProps;
  readonly title: string;
  readonly context?: string;
  readonly contextActions?: ReactNode;
  readonly onBack: () => void;
  readonly timeline: TimelineProps;
  /** Host-owned composer adapter preserves drafts and attachment-only sending. */
  readonly composer: ReactNode;
  /** Supply a ThreadPane; the channel stays mounted underneath on narrow views. */
  readonly thread?: ReactNode;
  readonly overlays?: ReactNode;
}

function ConnectionIndicator({ connected, status }: { readonly connected: boolean; readonly status: string }) {
  const reconnecting = !connected && /^(Fornyar økta|Gjenopprettar samtalen|Koplar til)/.test(status);
  const state = connected ? "connected" : reconnecting ? "reconnecting" : "disconnected";
  const symbol = connected ? "●" : reconnecting ? "◐" : "○";
  return <div className="sp-connection-status" data-connected={connected} data-state={state}
    role="status" aria-label={`Sambandsstatus: ${status}`} title={status}>
    <span aria-hidden="true">{symbol}</span><span className="sp-sr">{status}</span>
  </div>;
}

export function ConversationView(props: ConversationViewProps) {
  const snapshot = useSyncExternalStore(props.runtime.subscribe, props.runtime.getSnapshot, props.runtime.getSnapshot);
  const titleId = useId();
  return <Theme mode={props.theme} accent="citron" style={{ height: "100%", minHeight: 0 }}>
    <AppShell view={props.view} navigationLabel="Samtalar" navigation={<ConversationNavigation {...props.navigation} />}
      header={<>{props.header}{snapshot.connection.status && <ConnectionIndicator {...snapshot.connection} />}</>}>
      <div className="sp-discussion" data-thread-open={props.thread ? "true" : "false"}>
        <section className="sp-channel-pane" aria-labelledby={titleId}>
          <header className="sp-context">
            <Button className="sp-only-compact" onClick={props.onBack}>← Samtalar</Button>
            <div className="sp-conversation-title">{props.context && <p className="sp-kicker">{props.context}</p>}<h1 id={titleId} className="sp-heading">{props.title}</h1></div>
            {props.contextActions}
          </header>
          <ConversationTimeline {...props.timeline} />
          {props.composer}
        </section>
        {props.thread}
      </div>
    </AppShell>
    {props.overlays}
  </Theme>;
}
