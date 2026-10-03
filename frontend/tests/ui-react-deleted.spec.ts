import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";
import type { ChatMessage } from "../src/types";

const message = (sequence: number, deleted = false, parent: string | null = null): ChatMessage => ({
  id: `deleted-${sequence}`, channel_id: "deleted", parent_message_id: parent,
  sender_id: "reader", sender_display_name: "Lesar", body: `Melding ${sequence} ${"kontekst ".repeat(35)}`,
  sequence, sent_at: `2025-01-${sequence === 1 ? "01" : "02"}T12:00:00Z`, edited_at: null,
  deleted_at: deleted ? "2025-02-01T12:00:00Z" : null
});
const timeline = (page: Page) => page.locator("#sproyt-react-preview .sp-channel-pane > .sp-timeline");

async function server(page: Page, messages: ChatMessage[], link = "", replies: ChatMessage[] = [], initialRead = messages.at(-1)?.sequence ?? 0) {
  let socket: WebSocketRoute;
  const cursors: number[] = [];
  const reads: number[] = [];
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    socket = route;
    route.onMessage(data => {
      const command = JSON.parse(String(data));
      const reply = (type: string, payload: unknown) => route.send(JSON.stringify({
        protocol: "sproyt.chat.v1", request_id: command.request_id, type, payload
      }));
      switch (command.type) {
        case "hello": reply("hello", { participant_id: "reader" }); break;
        case "ping": route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type: "pong" })); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [] }); break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "list_my_channels": reply("channels_listed", { channels: [{ id: "deleted", slug: "deleted", name: "deleted",
          kind: "public", circle_id: null, direct_user_id: null, is_direct: false, description: "", role: "member",
          last_read_sequence: initialRead, latest_sequence: messages.at(-1)?.sequence ?? 0 }] }); break;
        case "subscribe_channel": reply("subscription_started", { channel_id: "deleted", history: messages.slice(-50) }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: "deleted", summaries: replies.length
          ? [{ root_message_id: "deleted-10", reply_count: replies.length, unread_count: 0, latest_sequence: replies.at(-1)!.sequence }] : [] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: "deleted", reactions: [] }); break;
        case "load_recent_messages": cursors.push(command.payload.before); reply("messages_loaded", { channel_id: "deleted",
          messages: messages.filter(item => item.sequence < command.payload.before).slice(-command.payload.limit) }); break;
        case "load_thread": reply("thread_loaded", { root_message_id: "deleted-10", messages: [messages.find(item => item.id === "deleted-10"), ...replies] }); break;
        case "mark_read": reads.push(command.payload.sequence); reply("read_marker_updated", { membership: {
          channel_id: "deleted", user_id: "reader", role: "member", joined_at: "2025-01-01T00:00:00Z", last_read_sequence: command.payload.sequence
        } }); break;
      }
    });
  });
  await page.goto(`/?participant=deleted-regression&channel=deleted${link}`);
  await expect(page.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  await page.bringToFront();
  return { cursors, reads, remove: (item: ChatMessage) => {
    const deleted = { ...item, deleted_at: "2025-02-01T12:00:00Z", body: "" };
    socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type: "chat", payload: { event: { type: "message_deleted", message: deleted } } }));
  } };
}

test("deleted timeline entries vanish while a removed reading anchor keeps adjacent context", async ({ page }) => {
  const messages = Array.from({ length: 50 }, (_, index) => message(index + 1, index === 0 || index === 49));
  const fixture = await server(page, messages);
  await expect(timeline(page).locator("[data-message-id]")).toHaveCount(48);
  await expect(timeline(page)).not.toContainText("Meldinga er sletta");
  await expect(timeline(page).locator(".sp-date")).toHaveCount(1);
  // A committed message count can precede initial scroll restoration.
  await timeline(page).evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(() => resolve())))));
  await timeline(page).evaluate(element => {
    element.dispatchEvent(new KeyboardEvent("keydown", { key: "PageUp" }));
    const target = element.querySelector<HTMLElement>('[data-message-id="deleted-25"]')!;
    element.scrollTop += target.getBoundingClientRect().top - element.getBoundingClientRect().top;
    element.dispatchEvent(new Event("scroll"));
  });
  // Delete after the reading gesture settles; active native scrolling owns its
  // position and deliberately suppresses restoration during the gesture.
  await timeline(page).evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(() => resolve())))));
  const next = timeline(page).locator('[data-message-id="deleted-26"]');
  const offset = await next.evaluate(element => element.getBoundingClientRect().top - element.parentElement!.getBoundingClientRect().top);
  fixture.remove(messages[24]!);
  await expect(timeline(page).locator('[data-message-id="deleted-25"]')).toHaveCount(0);
  await expect.poll(async () => Math.abs(await next.evaluate(element => element.getBoundingClientRect().top - element.parentElement!.getBoundingClientRect().top) - offset)).toBeLessThan(3);
  fixture.remove(messages[24]!);
  await expect(timeline(page).locator("[data-message-id]")).toHaveCount(47);
});

test("deleted roots retain only usable thread navigation and deleted replies stay hidden", async ({ page }) => {
  const messages = Array.from({ length: 15 }, (_, index) => message(index + 1, index === 9));
  const replies = [message(16, false, "deleted-10"), message(17, true, "deleted-10")];
  const fixture = await server(page, messages, "", replies);
  const root = timeline(page).locator('[data-message-id="deleted-10"]');
  await expect(root.getByRole("button", { name: "Opne tråd", exact: true })).toBeVisible();
  await expect(root).not.toContainText("Meldinga er sletta");
  await root.getByRole("button", { name: "Opne tråd", exact: true }).click();
  const thread = page.locator("#sproyt-react-preview .sp-thread-pane");
  await expect(thread).toContainText("Meldinga er sletta");
  await expect(thread.locator('[data-message-id="deleted-16"]')).toHaveCount(1);
  await expect(thread.locator('[data-message-id="deleted-17"]')).toHaveCount(0);
  fixture.remove(replies[0]!);
  await expect(root).toHaveCount(0);
});

test("a deleted notification target explains deletion and reveals nearby visible context", async ({ page }) => {
  const messages = Array.from({ length: 50 }, (_, index) => message(index + 1, index === 24));
  await server(page, messages, "&message=deleted-25&sequence=25");
  await expect(page.locator("#sproyt-react-preview")).toContainText("Meldinga frå lenkja er sletta.");
  await expect(timeline(page).locator('[data-message-id="deleted-25"]')).toHaveCount(0);
  await expect.poll(() => timeline(page).locator('[data-message-id="deleted-26"]').evaluate(element => {
    const offset = element.getBoundingClientRect().top - element.parentElement!.getBoundingClientRect().top;
    return offset >= 0 && offset < 40;
  })).toBe(true);
});

test("tombstone-only history pages advance raw cursors until visible messages arrive", async ({ page }) => {
  const messages = Array.from({ length: 120 }, (_, index) => message(index + 1, index >= 20));
  const fixture = await server(page, messages);
  await expect(timeline(page).locator("[data-message-id]")).toHaveCount(20);
  expect(fixture.cursors).toEqual([71, 21]);
  await expect(timeline(page)).toContainText("Starten på samtalen");
  await expect(timeline(page)).not.toContainText("Meldinga er sletta");
});

test("read markers pass known deleted messages, including an empty timeline, and stop at unseen live replies", async ({ context }) => {
  for (const scenario of ["trailing", "live-reply", "all-deleted"]) {
    const page = await context.newPage();
    const withReply = scenario === "live-reply";
    const messages = [message(1, scenario === "all-deleted"), message(2, true), message(3, !withReply, withReply ? "deleted-1" : null), message(4, true)];
    const fixture = await server(page, messages, "", [], 0);
    await expect(timeline(page).locator("[data-message-id]")).toHaveCount(scenario === "all-deleted" ? 0 : 1);
    await expect.poll(() => fixture.reads.at(-1)).toBe(withReply ? 2 : 4);
    expect(Math.max(...fixture.reads)).toBe(withReply ? 2 : 4);
    await page.close();
  }
});
