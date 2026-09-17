import { useLayoutEffect, useRef } from "react";
import type { ReactNode } from "react";
import { Status, ThreadPane } from "@sproyt/ui/react";
import type { ConversationSnapshot, ConversationSnapshotGroup } from "../../application/conversation-snapshot";
import { ConversationMessage, ConversationTimeline, ConversationView } from "./conversation-view";
import type { ConversationViewProps, MessagePresentation, TimelineProps } from "./conversation-view";

export interface ComposerTarget {
  readonly channelId: string;
  readonly parentMessageId: string | null;
}

export type TimelineControls = Pick<TimelineProps,
  "viewportRef" | "onScroll" | "error" | "onRetry" | "onLoadOlder">;

/** Commands and feature renderers belong to the host. The adapter neither sends
 * messages nor replaces the host's draft, attachment, reaction or modal state. */
export interface ConversationHostBindings {
  readonly runtime: ConversationViewProps["runtime"];
  readonly theme: ConversationViewProps["theme"];
  readonly view: ConversationViewProps["view"];
  readonly header: ReactNode;
  readonly onBack: () => void;
  readonly onQueryChange: (query: string) => void;
  readonly onSelect: (channelId: string) => void;
  readonly onCloseThread: () => void;
  /** Scope/inbox/create/discover commands remain available outside channel rows. */
  readonly navigationActions: ReactNode;
  /** Includes circle membership and each channel's notification controls. */
  readonly renderGroupActions?: (group: ConversationSnapshotGroup) => ReactNode;
  readonly renderConversationAction?: (conversation: ConversationSnapshotGroup["conversations"][number]) => ReactNode;
  readonly contextActions?: ReactNode;
  readonly message: MessagePresentation;
  readonly timeline?: TimelineControls;
  readonly threadTimeline?: TimelineControls & { readonly loading?: boolean; readonly hasOlder?: boolean };
  /** Must preserve accepted-send clearing, retries and attachment-only sending.
   * No bundled text-only Composer is silently substituted by this adapter. */
  readonly renderComposer: (target: ComposerTarget) => ReactNode;
  /** Dialogs, invitations, media viewers and reaction pickers remain explicit. */
  readonly overlays: ReactNode;
}

/** ThreadPane scrolls its parent+replies wrapper, not the inner timeline. Expose
 * that actual viewport to the same host read-marker and scroll-anchor policy. */
function ThreadTimeline(props: TimelineProps) {
  const inner = useRef<HTMLDivElement>(null);
  const { viewportRef, onScroll } = props;
  useLayoutEffect(() => {
    const viewport = inner.current?.parentElement;
    if (!(viewport instanceof HTMLDivElement)) return;
    const releaseRef = typeof viewportRef === "function" ? viewportRef(viewport) : undefined;
    if (viewportRef && typeof viewportRef !== "function") viewportRef.current = viewport;
    if (onScroll) viewport.addEventListener("scroll", onScroll);
    return () => {
      if (onScroll) viewport.removeEventListener("scroll", onScroll);
      if (typeof releaseRef === "function") releaseRef();
      else if (typeof viewportRef === "function") viewportRef(null);
      else if (viewportRef) viewportRef.current = null;
    };
  }, [viewportRef, onScroll]);
  return <ConversationTimeline {...props} viewportRef={inner} onScroll={undefined} />;
}

/** Pass getConversationSnapshot() after host changes, not as a store getter.
 * The only subscribed store remains the existing application's stable runtime. */
export function createConversationViewProps(snapshot: ConversationSnapshot, host: ConversationHostBindings): ConversationViewProps {
  const channelId = snapshot.selection.channelId;
  const channel = snapshot.activeChannel;
  const context = channel?.is_direct ? "Direkte" : channel?.circle_id
    ? snapshot.circles.find(circle => circle.id === channel.circle_id)?.name ?? "Vennekrets"
    : channel ? "Felles" : undefined;
  const thread = snapshot.thread;
  const threadVisible = thread !== null && channelId !== null && thread.channelId === channelId;
  return {
    runtime: host.runtime, theme: host.theme, view: host.view, header: host.header,
    title: snapshot.title, context,
    contextActions: host.contextActions, onBack: host.onBack,
    navigation: {
      groups: snapshot.groups.map(group => ({
        id: group.id, name: group.name,
        // Notification subscriptions are not a mute policy. Do not map disabled
        // notifications to the library's `muted` appearance.
        conversations: group.conversations.map(item => ({ id: item.id, name: item.name, unread: item.unread,
          action: host.renderConversationAction?.(item) })),
        actions: host.renderGroupActions?.(group)
      })),
      selectedId: channelId, query: snapshot.query, onQueryChange: host.onQueryChange,
      onSelect: host.onSelect, actions: host.navigationActions
    },
    timeline: {
      ...host.message, ...host.timeline,
      channelId: snapshot.timeline.channelId, messages: snapshot.timeline.messages,
      items: snapshot.timeline.items,
      loading: snapshot.timeline.loading, hasOlder: snapshot.timeline.hasOlder, notices: snapshot.timeline.notices
    },
    composer: channelId !== null && snapshot.activeChannel !== null
      ? host.renderComposer({ channelId, parentMessageId: null }) : null,
    thread: threadVisible ? <ThreadPane title="Tråd" context={snapshot.title} closeLabel="Lukk tråden"
      onClose={host.onCloseThread}
      parent={thread.root ? <ConversationMessage message={thread.root} {...host.message} threadParent />
        : <Status>Den opphavlege meldinga er ikkje lasta.</Status>}
      composer={snapshot.activeChannel !== null ? host.renderComposer({ channelId, parentMessageId: thread.rootMessageId }) : null}>
      <ThreadTimeline {...host.message} {...host.threadTimeline}
        channelId={channelId} parentMessageId={thread.rootMessageId} messages={thread.replies} />
    </ThreadPane> : null,
    overlays: host.overlays
  };
}

export function ConversationHost({ snapshot, host }: { readonly snapshot: ConversationSnapshot; readonly host: ConversationHostBindings }) {
  return <ConversationView {...createConversationViewProps(snapshot, host)} />;
}
