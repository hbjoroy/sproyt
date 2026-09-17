import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createApplicationRuntime } from "../../application/runtime";
import { projectConversationSnapshot } from "../../application/conversation-snapshot";
import type { ConversationSnapshotSource } from "../../application/conversation-snapshot";
import type { ChatMessage } from "../../types";
import { ConversationHost, createConversationViewProps } from "./host-adapter";
import type { ComposerTarget, ConversationHostBindings } from "./host-adapter";

const message = (id: string, sequence: number, parent_message_id: string | null = null): ChatMessage => ({
  id, sequence, parent_message_id, channel_id: "channel", sender_id: "person",
  sender_display_name: "Historisk namn", body: id, sent_at: "2026-09-16T20:00:00Z",
  edited_at: null, deleted_at: null
});

function setup(overrides: Partial<ConversationSnapshotSource> = {}) {
  let events = 0;
  const runtime = createApplicationRuntime(() => { events++; });
  const root = message("root", 1);
  const source: ConversationSnapshotSource = {
    channels: [{ id: "channel", slug: "chat", name: "Prat", kind: "private", circle_id: "circle",
      direct_user_id: null, description: "", role: "owner", last_read_sequence: 1, latest_sequence: 3 }],
    circles: new Map([["circle", { id: "circle", slug: "friends", name: "Vener", role: "owner", created_by: "person", created_at: "" }]]),
    activeChannelId: "channel", activeCircleId: "circle", activeRootScope: "circle", activeInboxKind: null,
    query: "", timeline: [{ type: "message", message: root }, { type: "system", text: "<trygg status>" }],
    threadReplies: new Map([["root", [message("reply", 2, "root")]]]),
    threadRoots: new Map(), threadSummaries: new Map(), activeThreadRootId: "root",
    historyLoading: false, historyHasMore: true, connection: runtime.getSnapshot().connection,
    channelNotificationIds: new Set(), pendingChannelNotificationIds: new Set(["channel"]),
    directChannelLabel: channel => channel.name, ...overrides
  };
  const targets: ComposerTarget[] = [];
  const host: ConversationHostBindings = {
    runtime, theme: "dark", view: "detail", header: "Sprøyt", onBack() {}, onQueryChange() {}, onSelect() {}, onCloseThread() {},
    navigationActions: "Innboks og oppgåver", contextActions: "Medlemmer",
    renderGroupActions: group => group.conversations[0]?.notifications?.pending ? "Lagrar varsling" : null,
    renderConversationAction: conversation => conversation.notifications?.pending ? "Kanalvarsel ventar" : null,
    message: { formatTime: () => "22:00", renderContent: item => `media-${item.id}`,
      renderActions: item => `reaksjonar-${item.id}`, onReactionRequest() {} },
    renderComposer: target => { targets.push(target); return `vedlegg-${target.parentMessageId ?? "kanal"}`; },
    overlays: createElement("div", { role: "dialog" }, "Invitasjon og bilete"),
    timeline: { onLoadOlder() {}, onScroll() {} }
  };
  return { snapshot: projectConversationSnapshot(source), host, targets, events: () => events };
}

test("snapshot adapter preserves scoped composers, notifications, notices, callbacks and host slots", () => {
  const { snapshot, host, targets, events } = setup();
  const props = createConversationViewProps(snapshot, host);
  assert.deepEqual(targets, [{ channelId: "channel", parentMessageId: null }, { channelId: "channel", parentMessageId: "root" }]);
  assert.equal(props.navigation.selectedId, "channel");
  const group = props.navigation.groups[0];
  assert.ok(group);
  const conversation = group.conversations[0];
  assert.ok(conversation);
  assert.equal(group.id, "circle:circle");
  assert.equal(conversation.unread, 2);
  assert.equal(conversation.muted, undefined);
  assert.equal(group.actions, "Lagrar varsling");
  assert.equal(conversation.action, "Kanalvarsel ventar");
  assert.equal(props.navigation.onSelect, host.onSelect);
  assert.equal(props.navigation.onQueryChange, host.onQueryChange);
  assert.equal(props.timeline.onScroll, host.timeline?.onScroll);
  assert.equal(props.timeline.onLoadOlder, host.timeline?.onLoadOlder);
  assert.equal(props.timeline.onReactionRequest, host.message.onReactionRequest);
  assert.equal(props.timeline.messages, snapshot.timeline.messages);
  assert.equal(props.overlays, host.overlays);
  assert.equal(props.context, "Vener");
  assert.equal(events(), 0);
});

test("root and replies retain media/actions and overlays, with no text-only composer replacement", () => {
  const { snapshot, host } = setup();
  const html = renderToStaticMarkup(createElement(ConversationHost, { snapshot, host }));
  for (const expected of ["media-root", "media-reply", "reaksjonar-root", "reaksjonar-reply", "vedlegg-kanal", "vedlegg-root", "Invitasjon og bilete", "&lt;trygg status&gt;", "Last eldre meldingar"]) {
    assert.ok(html.includes(expected), expected);
  }
  assert.equal(html.match(/data-message-id="reply"/g)?.length, 1);
  assert.ok(html.includes('data-thread-open="true"'));
});

test("search cannot erase the selected conversation's circle context", () => {
  const { snapshot, host } = setup({ query: "no match" });
  assert.equal(snapshot.groups.length, 0);
  assert.equal(createConversationViewProps(snapshot, host).context, "Vener");
});

test("missing selection does not construct send targets or open a stale thread", () => {
  const { snapshot, host, targets } = setup({ activeChannelId: null });
  const props = createConversationViewProps(snapshot, host);
  assert.equal(props.composer, null);
  assert.equal(props.thread, null);
  assert.deepEqual(targets, []);
});

test("unavailable thread parent has recovery context while replies and draft stay present", () => {
  const { snapshot, host } = setup({ timeline: [] });
  const html = renderToStaticMarkup(createElement(ConversationHost, { snapshot, host }));
  assert.ok(html.includes("Den opphavlege meldinga er ikkje lasta."));
  assert.ok(html.includes("media-reply"));
  assert.ok(html.includes("vedlegg-root"));
});

test("deleted thread parent never reaches the host's media renderer", () => {
  const root = { ...message("root", 1), deleted_at: "2026-09-16T20:01:00Z" };
  const { snapshot, host } = setup({ timeline: [{ type: "message", message: root }] });
  const html = renderToStaticMarkup(createElement(ConversationHost, { snapshot, host }));
  assert.ok(!html.includes("media-root"));
  assert.ok(html.includes("Meldinga er sletta."));
  assert.ok(html.includes("media-reply"));
});
