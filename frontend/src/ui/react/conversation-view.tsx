import { useCallback, useId, useRef, useSyncExternalStore } from "react";
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
  /** Use the existing safe renderer (LegacyContent is available as a bridge). */
  readonly renderContent: (message: ChatMessage) => ReactNode;
  /** Permission checks, replies, reaction counts, edit and delete stay with the host. */
  readonly renderActions?: (message: ChatMessage) => ReactNode;
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
      : <ConversationMessage key={item.message.id} message={item.message} {...props} />)}
  </div>;
}

export type MessagePresentation = Pick<TimelineProps, "formatTime" | "formatAuthor" | "renderContent" | "renderActions" | "messageStatus" | "onReactionRequest">;

/** The thread parent uses the same safe rendering and permission policy as replies. */
export function ConversationMessage(props: MessagePresentation & { readonly message: ChatMessage }) {
  const message = props.message;
  const latestMessage = useRef(message);
  latestMessage.current = message;
  const requestReaction = props.onReactionRequest;
  const onReactionRequest = useCallback((anchor: HTMLElement) => {
    if (!latestMessage.current.deleted_at) requestReaction?.(latestMessage.current, anchor);
  }, [requestReaction]);
  return <div data-message-id={message.id}>
    <Message author={props.formatAuthor?.(message) ?? message.sender_display_name} dateTime={message.sent_at}
        time={props.formatTime(message.sent_at)}
        status={message.deleted_at ? "Sletta" : props.messageStatus?.(message) ?? (message.edited_at ? "Redigert" : undefined)}
        actions={props.renderActions?.(message)}
        onReactionRequest={!message.deleted_at && props.onReactionRequest
          ? onReactionRequest : undefined}>
        {message.deleted_at ? <p>Meldinga er sletta.</p> : props.renderContent(message)}
    </Message>
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

export function ConversationView(props: ConversationViewProps) {
  const snapshot = useSyncExternalStore(props.runtime.subscribe, props.runtime.getSnapshot, props.runtime.getSnapshot);
  const titleId = useId();
  return <Theme mode={props.theme} accent="citron" style={{ height: "100%", minHeight: 0 }}>
    <AppShell view={props.view} navigationLabel="Samtalar" navigation={<ConversationNavigation {...props.navigation} />}
      header={<>{props.header}{snapshot.connection.status && <Status>{snapshot.connection.status}</Status>}</>}>
      <div className="sp-discussion" data-thread-open={props.thread ? "true" : "false"}>
        <section className="sp-channel-pane" aria-labelledby={titleId}>
          <header className="sp-context">
            <Button className="sp-only-compact" onClick={props.onBack}>← Samtalar</Button>
            <div>{props.context && <p className="sp-kicker">{props.context}</p>}<h1 id={titleId} className="sp-heading">{props.title}</h1></div>
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
