import type { Channel, ChatMessage, Circle, ThreadSummary } from "../types";
import type { RuntimeSnapshot } from "./runtime";

export type ConversationTimelineItem =
  | Readonly<{ type: "message"; message: ChatMessage }>
  | Readonly<{ type: "system"; text: string }>;

/** The existing application remains the state owner. This input has no commands,
 * transport, DOM nodes, or presentation-library types. */
export interface ConversationSnapshotSource {
  readonly channels: readonly Channel[];
  readonly circles: ReadonlyMap<string, Circle>;
  readonly activeChannelId: string | null;
  readonly activeCircleId: string | null;
  readonly activeRootScope: "shared" | "circle" | "direct";
  readonly activeInboxKind: "unread" | "mentions" | "tasks" | null;
  readonly query: string;
  readonly timeline: readonly ConversationTimelineItem[];
  readonly threadReplies: ReadonlyMap<string, readonly ChatMessage[]>;
  readonly threadRoots: ReadonlyMap<string, ChatMessage>;
  readonly threadSummaries: ReadonlyMap<string, ThreadSummary>;
  readonly activeThreadRootId: string | null;
  readonly historyLoading: boolean;
  readonly historyHasMore: boolean;
  readonly connection: RuntimeSnapshot["connection"];
  readonly channelNotificationIds: ReadonlySet<string>;
  readonly pendingChannelNotificationIds: ReadonlySet<string>;
  readonly channelNotificationErrors?: ReadonlyMap<string, string>;
  readonly directChannelLabel: (channel: Channel) => string;
}

export interface ConversationEntry {
  readonly id: string;
  readonly name: string;
  readonly unread: number;
  readonly channel: Readonly<Channel>;
  readonly notifications: Readonly<{ enabled: boolean; pending: boolean; error?: string }> | null;
}

/** Structurally compatible with the React adapter's ConversationGroup. */
export interface ConversationSnapshotGroup {
  readonly id: string;
  readonly name: string;
  readonly circle: Readonly<Circle> | null;
  readonly conversations: readonly ConversationEntry[];
}

function copyMessage(message: ChatMessage): Readonly<ChatMessage> {
  return Object.freeze({ ...message });
}

function orderedMessages(messages: readonly ChatMessage[], channelId: string | null, parentId: string | null) {
  return Object.freeze(messages
    .filter(message => message.channel_id === channelId && message.parent_message_id === parentId)
    .slice().sort((left, right) => left.sequence - right.sequence)
    .map(copyMessage));
}

/** A detached, immutable point-in-time projection. Call after state changes and
 * pass the resulting props to the view; this is not a useSyncExternalStore getter
 * (each call produces a new snapshot). No unread/read markers are advanced here. */
export function projectConversationSnapshot(source: ConversationSnapshotSource) {
  const channels = Object.freeze(source.channels.map(channel => Object.freeze({ ...channel })));
  const circles = Object.freeze([...source.circles.values()].map(circle => Object.freeze({ ...circle })));
  const query = source.query.trim().toLocaleLowerCase();
  const matches = (text: string) => !query || text.toLocaleLowerCase().includes(query);
  const groups: ConversationSnapshotGroup[] = [];
  const appendGroup = (id: string, name: string, circle: Readonly<Circle> | null, members: readonly Channel[]) => {
    const conversations = members.map(channel => {
      const label = channel.is_direct ? source.directChannelLabel(channel) : `# ${channel.name}`;
      return Object.freeze({
        id: channel.id, name: label, unread: Math.max(0, channel.latest_sequence - channel.last_read_sequence),
        channel, notifications: channel.is_direct ? null : Object.freeze({
          enabled: source.channelNotificationIds.has(channel.id),
          pending: source.pendingChannelNotificationIds.has(channel.id),
          error: source.channelNotificationErrors?.get(channel.id)
        })
      });
    }).filter(item => matches(`${item.name} ${name}`));
    // Empty member circles still have create/discover actions in the host.
    if (conversations.length || (circle && matches(name))) {
      groups.push(Object.freeze({ id, name, circle, conversations: Object.freeze(conversations) }));
    }
  };
  appendGroup("scope:shared", "Felles", null, channels.filter(channel => !channel.circle_id && !channel.is_direct));
  for (const circle of circles) {
    appendGroup(`circle:${circle.id}`, circle.name, circle,
      channels.filter(channel => channel.circle_id === circle.id && !channel.is_direct));
  }
  // Membership and channel lists can arrive independently. Keep such channels
  // reachable without using their eventual display name as identity.
  for (const circleId of new Set(channels.filter(channel => channel.circle_id && !channel.is_direct).map(channel => channel.circle_id!))) {
    if (!source.circles.has(circleId)) appendGroup(`circle:${circleId}`, "Vennekrets", null,
      channels.filter(channel => channel.circle_id === circleId && !channel.is_direct));
  }
  appendGroup("scope:direct", "Direkte", null, channels.filter(channel => channel.is_direct));

  const timelineMessages = source.timeline.flatMap(item => item.type === "message" ? [item.message] : []);
  const timelineItems: readonly ConversationTimelineItem[] = Object.freeze(source.timeline.reduce<ConversationTimelineItem[]>((items, item) => {
    if (item.type === "system") items.push(Object.freeze({ type: "system", text: item.text }));
    else if (item.message.channel_id === source.activeChannelId && item.message.parent_message_id === null) {
      items.push(Object.freeze({ type: "message", message: copyMessage(item.message) }));
    }
    return items;
  }, []));
  const activeChannel = channels.find(channel => channel.id === source.activeChannelId) ?? null;
  const rootId = source.activeThreadRootId;
  const rootCandidate = rootId ? timelineMessages.find(message => message.id === rootId) ?? source.threadRoots.get(rootId) : undefined;
  const root = rootCandidate && rootCandidate.parent_message_id === null && rootCandidate.channel_id === source.activeChannelId
    ? copyMessage(rootCandidate) : null;
  const thread = rootId ? Object.freeze({
    rootMessageId: rootId,
    channelId: source.activeChannelId,
    root,
    replies: orderedMessages(source.threadReplies.get(rootId) ?? [], source.activeChannelId, rootId),
    summary: source.threadSummaries.has(rootId) ? Object.freeze({ ...source.threadSummaries.get(rootId)! }) : null
  }) : null;

  return Object.freeze({
    channels, circles, groups: Object.freeze(groups), query: source.query,
    selection: Object.freeze({ channelId: source.activeChannelId, circleId: source.activeCircleId,
      rootScope: source.activeRootScope, inboxKind: source.activeInboxKind }),
    activeChannel,
    title: activeChannel ? (activeChannel.is_direct ? source.directChannelLabel(activeChannel) : `# ${activeChannel.name}`) : "Prat",
    timeline: Object.freeze({ channelId: source.activeChannelId,
      messages: orderedMessages(timelineMessages, source.activeChannelId, null),
      items: timelineItems,
      notices: Object.freeze(source.timeline.flatMap(item => item.type === "system" ? [item.text] : [])),
      loading: source.historyLoading, hasOlder: source.historyHasMore }),
    thread,
    threadSummaries: Object.freeze([...source.threadSummaries.values()].map(summary => Object.freeze({ ...summary }))),
    connection: Object.freeze({ ...source.connection })
  });
}

export type ConversationSnapshot = ReturnType<typeof projectConversationSnapshot>;
