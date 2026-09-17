import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createApplicationRuntime } from "../../application/runtime";
import type { ChatMessage } from "../../types";
import { ConversationNavigation, ConversationTimeline, ConversationView, messagesForTimeline } from "./conversation-view";
import type { TimelineProps } from "./conversation-view";

function message(id: string, sequence: number, overrides: Partial<ChatMessage> = {}): ChatMessage {
  return { id, sequence, channel_id: "channel", parent_message_id: null, sender_id: "author",
    sender_display_name: "Historisk namn", body: `<script>${id}</script>`, sent_at: "2026-09-16T20:00:00Z",
    edited_at: null, deleted_at: null, ...overrides };
}

test("timeline separates channels and thread replies without mutating application state", () => {
  const messages = [message("later", 4), message("reply", 3, { parent_message_id: "earlier" }),
    message("foreign", 1, { channel_id: "other" }), message("earlier", 2)];
  assert.deepEqual(messagesForTimeline({ messages, channelId: "channel" }).map(item => item.id), ["earlier", "later"]);
  assert.deepEqual(messagesForTimeline({ messages, channelId: "channel", parentMessageId: "earlier" }).map(item => item.id), ["reply"]);
  assert.deepEqual(messages.map(item => item.id), ["later", "reply", "foreign", "earlier"]);
});

test("deleted body never reaches content renderer and ordinary text stays escaped", () => {
  const rendered: string[] = [];
  const props: TimelineProps = {
    channelId: "channel", messages: [message("deleted", 1, { deleted_at: "2026-09-16T20:01:00Z" }), message("visible", 2)],
    formatTime: () => "22:00", renderContent: item => { rendered.push(item.id); return item.body; }
  };
  const html = renderToStaticMarkup(createElement(ConversationTimeline, props));
  assert.deepEqual(rendered, ["visible"]);
  assert.ok(html.includes("Meldinga er sletta."));
  assert.ok(html.includes("Historisk namn"));
  assert.ok(html.includes("&lt;script&gt;visible&lt;/script&gt;"));
  assert.ok(!html.includes("<script>"));
});

test("ordered timeline keeps notices between messages and formats the author", () => {
  const first = message("first", 1);
  const second = message("second", 2);
  const html = renderToStaticMarkup(createElement(ConversationTimeline, {
    channelId: "channel", messages: [first, second],
    items: [{ type: "message", message: first }, { type: "system", text: "Tilkopling attoppretta" },
      { type: "message", message: second }],
    formatTime: () => "22:00", formatAuthor: item => item.id === "first" ? "Du · 🌱 · Arbeider" : item.sender_display_name,
    renderContent: item => item.body
  }));
  assert.ok(html.indexOf("first") < html.indexOf("Tilkopling attoppretta"));
  assert.ok(html.indexOf("Tilkopling attoppretta") < html.indexOf("second"));
  assert.ok(html.includes("Du · 🌱 · Arbeider"));
});

test("circles with identical names retain distinct groups and channel identities", () => {
  const html = renderToStaticMarkup(createElement(ConversationNavigation, {
    groups: [
      { id: "first", name: "Venner", conversations: [{ id: "c1", name: "Prat", unread: 2 }] },
      { id: "second", name: "Venner", conversations: [{ id: "c2", name: "Prat" }] }
    ], selectedId: "c2", query: "", onSelect() {}, onQueryChange() {}
  }));
  assert.ok(html.includes('data-conversation-group="first"'));
  assert.ok(html.includes('data-conversation-group="second"'));
  assert.equal(html.match(/aria-current="page"/g)?.length, 1);
  assert.ok(html.includes('aria-label="2 uleste"'));
});

test("shell reads the existing runtime without enqueuing events or taking its lifecycle", () => {
  let delivered = 0;
  const runtime = createApplicationRuntime(() => { delivered++; });
  runtime.store.updateConnection({ connected: false, status: "Fråkopla — prøver igjen" });
  const html = renderToStaticMarkup(createElement(ConversationView, {
    runtime, theme: "dark", view: "detail", header: "Sprøyt", title: "Prat", onBack() {}, composer: null,
    navigation: { groups: [], selectedId: null, query: "", onQueryChange() {}, onSelect() {} },
    timeline: { channelId: "channel", messages: [], formatTime: value => value, renderContent: item => item.body }
  }));
  assert.ok(html.includes("Fråkopla — prøver igjen"));
  assert.ok(html.includes('data-state="disconnected"'));
  assert.ok(html.includes("○"));
  assert.ok(html.includes('data-theme="dark"'));
  assert.equal(delivered, 0);
  assert.equal(runtime.getSnapshot().transport.processedEvents, 0);
  runtime.store.updateConnection({ connected: true, status: "Tilkopla" });
  assert.equal(runtime.getSnapshot().connection.connected, true);
});
