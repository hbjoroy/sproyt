import assert from "node:assert/strict";
import test from "node:test";
import { projectConversationSnapshot, type ConversationSnapshotSource } from "../src/application/conversation-snapshot";
import type { ConversationNavigationProps, TimelineProps } from "../src/ui/react/conversation-view";
import type { Channel, ChatMessage, Circle } from "../src/types";

const channel = (id: string, overrides: Partial<Channel> = {}): Channel => ({ id, slug: id, name: "Prat", kind: "public",
  circle_id: null, direct_user_id: null, is_direct: false, description: "", role: "member", latest_sequence: 8,
  last_read_sequence: 5, ...overrides });
const circle = (id: string): Circle => ({ id, name: "Venner", slug: id, created_by: "owner", created_at: "2026-09-16", role: "member" });
const message = (id: string, sequence: number, overrides: Partial<ChatMessage> = {}): ChatMessage => ({ id, sequence,
  channel_id: "shared", parent_message_id: null, sender_id: "author", sender_display_name: "Historisk namn",
  body: id, sent_at: "2026-09-16T20:00:00Z", edited_at: null, deleted_at: null, ...overrides });
const source = (overrides: Partial<ConversationSnapshotSource> = {}): ConversationSnapshotSource => ({
  channels: [channel("shared")], circles: new Map(), activeChannelId: "shared", activeCircleId: null,
  activeRootScope: "shared", activeInboxKind: null, query: "", timeline: [], threadReplies: new Map(),
  threadRoots: new Map(), threadSummaries: new Map(), activeThreadRootId: null, historyLoading: false,
  historyHasMore: false, connection: { connected: true, status: "Tilkopla" }, channelNotificationIds: new Set(),
  pendingChannelNotificationIds: new Set(), directChannelLabel: item => item.name, ...overrides
});

test("domain projection preserves stable circle/channel identities, empty circles and human DM labels", () => {
  const snapshot = projectConversationSnapshot(source({
    channels: [channel("shared"), channel("c1", { circle_id: "one" }), channel("c2", { circle_id: "two" }),
      channel("dm", { is_direct: true, direct_user_id: "peer" }), channel("orphan", { circle_id: "pending-circle" })],
    circles: new Map([['one', circle("one")], ['two', circle("two")], ['empty', circle("empty")]]),
    directChannelLabel: () => "Ada Lovelace", channelNotificationIds: new Set(["c1"]), pendingChannelNotificationIds: new Set(["c2"]),
    channelNotificationErrors: new Map([["shared", "Kunne ikkje endre kanalvarsel: Prøv igjen"]])
  }));
  assert.deepEqual(snapshot.groups.map(group => group.id), ["scope:shared", "circle:one", "circle:two", "circle:empty", "circle:pending-circle", "scope:direct"]);
  assert.equal(snapshot.groups[1]?.conversations[0]?.notifications?.enabled, true);
  assert.equal(snapshot.groups[2]?.conversations[0]?.notifications?.pending, true);
  assert.equal(snapshot.groups[0]?.conversations[0]?.notifications?.error, "Kunne ikkje endre kanalvarsel: Prøv igjen");
  assert.equal(snapshot.groups[3]?.conversations.length, 0);
  assert.equal(snapshot.groups[5]?.conversations[0]?.name, "Ada Lovelace");
  assert.equal(snapshot.groups[5]?.conversations[0]?.notifications, null);
  // Compile-time contracts: domain data can feed the existing adapter directly.
  const groups: ConversationNavigationProps["groups"] = snapshot.groups;
  const messages: TimelineProps["messages"] = snapshot.timeline.messages;
  assert.equal(groups, snapshot.groups);
  assert.equal(messages, snapshot.timeline.messages);
});

test("search retains active selection and matches group context without altering unread state", () => {
  const snapshot = projectConversationSnapshot(source({ channels: [channel("shared", { last_read_sequence: 12 }),
    channel("circle-channel", { circle_id: "one" })], circles: new Map([["one", circle("one")]]), query: " venner " }));
  assert.equal(snapshot.selection.channelId, "shared");
  assert.equal(snapshot.activeChannel?.id, "shared");
  assert.deepEqual(snapshot.groups.map(group => group.id), ["circle:one"]);
  assert.equal(snapshot.groups[0]?.conversations[0]?.unread, 3);
  assert.equal(snapshot.channels[0]?.last_read_sequence, 12);
});

test("root timeline isolates replies and channels, preserves notices and sorts without mutating host data", () => {
  const original = [message("late", 9), message("reply", 3, { parent_message_id: "early" }),
    message("foreign", 2, { channel_id: "other" }), message("early", 1)];
  const snapshot = projectConversationSnapshot(source({ timeline: [
    ...original.map(item => ({ type: "message" as const, message: item })), { type: "system", text: "Prøv igjen" }
  ], historyLoading: true, historyHasMore: true }));
  assert.deepEqual(snapshot.timeline.messages.map(item => item.id), ["early", "late"]);
  assert.deepEqual(snapshot.timeline.notices, ["Prøv igjen"]);
  assert.deepEqual(snapshot.timeline.items.map(item => item.type === "message" ? item.message.id : item.text),
    ["late", "early", "Prøv igjen"]);
  assert.equal(snapshot.timeline.loading, true);
  assert.equal(snapshot.timeline.hasOlder, true);
  assert.deepEqual(original.map(item => item.id), ["late", "reply", "foreign", "early"]);
});

test("thread can use a separately loaded root and never admits foreign replies", () => {
  const snapshot = projectConversationSnapshot(source({ activeThreadRootId: "root", threadRoots: new Map([["root", message("root", 1)]]),
    threadReplies: new Map([["root", [message("r2", 5, { parent_message_id: "root" }), message("r1", 3, { parent_message_id: "root" }),
      message("foreign", 2, { parent_message_id: "root", channel_id: "other" }), message("different", 4, { parent_message_id: "other-root" })]]]),
    threadSummaries: new Map([["root", { root_message_id: "root", reply_count: 7, unread_count: 4, latest_sequence: 12 }]]) }));
  assert.equal(snapshot.thread?.root?.id, "root");
  assert.deepEqual(snapshot.thread?.replies.map(item => item.id), ["r1", "r2"]);
  assert.equal(snapshot.thread?.summary?.reply_count, 7);
  assert.equal(snapshot.thread?.summary?.unread_count, 4);
  assert.equal(snapshot.timeline.messages.length, 0);
});

test("snapshot is detached from subsequent host mutations and immutable to consumers", () => {
  const root = message("root", 1);
  const shared = channel("shared");
  const connection = { connected: false, status: "Fråkopla" };
  const snapshot = projectConversationSnapshot(source({ channels: [shared], connection, timeline: [{ type: "message", message: root }] }));
  root.body = "changed";
  shared.name = "Renamed";
  connection.status = "Tilkopla";
  assert.equal(snapshot.timeline.messages[0]?.body, "root");
  assert.equal(snapshot.activeChannel?.name, "Prat");
  assert.equal(snapshot.connection.status, "Fråkopla");
  assert.ok(Object.isFrozen(snapshot));
  assert.ok(Object.isFrozen(snapshot.groups[0]?.conversations[0]?.channel));
  assert.ok(Object.isFrozen(snapshot.timeline.messages));
  assert.ok(Object.isFrozen(snapshot.timeline.messages[0]));
});

test("missing selection and unloaded or stale thread roots are explicit", () => {
  const unloaded = projectConversationSnapshot(source({ activeThreadRootId: "root", activeChannelId: null }));
  assert.equal(unloaded.activeChannel, null);
  assert.equal(unloaded.thread?.root, null);
  assert.equal(unloaded.title, "Prat");
  const stale = projectConversationSnapshot(source({ activeThreadRootId: "root", threadRoots: new Map([["root", message("root", 1, { channel_id: "other" })]]) }));
  assert.equal(stale.thread?.root, null);
});
