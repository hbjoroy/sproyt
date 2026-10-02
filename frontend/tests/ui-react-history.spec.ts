import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";
import type { ChatMessage } from "../src/types";

type Command = { type: string; request_id: string; payload: { channel_id: string; before?: number; limit?: number } };
const message = (sequence: number, reply = false): ChatMessage => ({ id: `history-${sequence}`, channel_id: "history",
  parent_message_id: reply ? "history-1" : null, sender_id: "reader", sender_display_name: "Lesar",
  body: `Historisk melding ${sequence} ${"lang melding ".repeat(30)}`, sequence, sent_at: "2025-01-01T12:00:00Z",
  edited_at: null, deleted_at: null });

async function selectConversation(page: Page, name: string) {
  const app = page.locator("#sproyt-react-preview");
  const back = app.getByRole("button", { name: "Samtalar", exact: true });
  if (await back.isVisible()) await back.click();
  await app.getByRole("button", { name, exact: true }).click();
}

async function historyServer(page: Page, messages: ChatMessage[], options: { link?: string; hold?: boolean } = {}) {
  const requests: Command[] = [];
  let socket: WebSocketRoute;
  let held: Command | null = null;
  let mode: "normal" | "error" | "hold" = options.hold ? "hold" : "normal";
  const emit = (command: Command, type: string, payload: unknown) => socket.send(JSON.stringify({
    protocol: "sproyt.chat.v1", request_id: command.request_id, type, payload
  }));
  const respond = (command: Command) => emit(command, "messages_loaded", { channel_id: command.payload.channel_id,
    messages: messages.filter(item => item.sequence < (command.payload.before ?? Infinity)).slice(-(command.payload.limit ?? 50)) });
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    socket = route;
    route.onMessage(data => {
      const command = JSON.parse(String(data)) as Command;
      switch (command.type) {
        case "hello": emit(command, "hello", { participant_id: "reader" }); break;
        case "ping": socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type: "pong" })); break;
        case "list_users": emit(command, "users_listed", { users: [] }); break;
        case "list_my_circles": emit(command, "circles_listed", { circles: [] }); break;
        case "list_mentions": emit(command, "mentions_listed", { mentions: [] }); break;
        case "list_tasks": emit(command, "tasks_listed", { tasks: [] }); break;
        case "list_my_channels": emit(command, "channels_listed", { channels: ["history", "empty"].map(id => ({
          id, slug: id, name: id, kind: "public", circle_id: null, direct_user_id: null, is_direct: false,
          description: "", role: "member", last_read_sequence: messages.length, latest_sequence: messages.length
        })) }); break;
        case "subscribe_channel": emit(command, "subscription_started", { channel_id: command.payload.channel_id,
          history: command.payload.channel_id === "history" ? messages.slice(-50) : [] }); break;
        case "list_thread_summaries": emit(command, "thread_summaries_listed", { channel_id: command.payload.channel_id, summaries: [] }); break;
        case "list_channel_reactions": emit(command, "channel_reactions_listed", { channel_id: command.payload.channel_id, reactions: [] }); break;
        case "load_recent_messages":
          requests.push(command);
          if (mode === "hold") { held = command; break; }
          if (mode === "error") { mode = "normal"; emit(command, "error", { code: "unavailable", message: "Nettfeil" }); break; }
          respond(command); break;
      }
    });
  });
  await page.goto(`/?participant=history-regression&channel=history${options.link ?? ""}`);
  const timeline = page.locator("#sproyt-react-preview .sp-channel-pane > .sp-timeline");
  await expect(page.locator("#sproyt-react-preview").getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  return { timeline, requests, mode: (next: typeof mode) => { mode = next; },
    release: () => { if (!held) throw new Error("No held page"); respond(held); held = null; mode = "normal"; },
    disconnect: () => socket.close({ code: 1012, reason: "test reconnect" }) };
}

test("all-read first opening walks reply-only pages and reaches real history end", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const messages = Array.from({ length: 171 }, (_, index) => message(index + 1, index >= 61));
  const { timeline, requests } = await historyServer(page, messages);
  await expect(timeline.locator("[data-message-id]")).toHaveCount(40);
  expect(requests.map(command => command.payload.before)).toEqual([122, 72]);
  await timeline.getByRole("button", { name: "Last eldre meldingar" }).evaluate(button => (button as HTMLButtonElement).click());
  await expect(timeline.locator("[data-message-id]")).toHaveCount(61);
  await expect(timeline).toContainText("Starten på samtalen");
  await expect(timeline.getByRole("button", { name: "Last eldre meldingar" })).toHaveCount(0);
  expect(requests.map(command => command.payload.before)).toEqual([122, 72, 22]);
  const ids = await timeline.locator("[data-message-id]").evaluateAll(elements => elements.map(element => (element as HTMLElement).dataset.messageId));
  expect(new Set(ids).size).toBe(61);
});

test("older pages skip reply-only gaps, retain own-message anchor and handle image resize", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 650 });
  const messages = Array.from({ length: 211 }, (_, index) => message(index + 1, index >= 60 && index < 160));
  const server = await historyServer(page, messages);
  // Wait for initial React layout/scroll restoration before beginning a new
  // reading gesture. DOM message count alone does not imply restored geometry.
  await server.timeline.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  server.mode("hold");
  const anchor = await server.timeline.evaluate((element: HTMLElement) => {
    element.scrollTop = 0;
    const message = element.querySelector<HTMLElement>("[data-message-id]")!;
    const anchor = { id: message.dataset.messageId!,
      offset: message.getBoundingClientRect().top - element.getBoundingClientRect().top };
    element.dispatchEvent(new Event("scroll"));
    // Explicit action also covers an initial React commit still restoring its
    // position; user paging must work independently of automatic edge events.
    element.querySelector<HTMLButtonElement>("button")!.click();
    return anchor;
  });
  await expect.poll(() => server.requests.length).toBe(1);
  server.release();
  await expect(server.timeline.locator("[data-message-id]")).toHaveCount(51);
  await server.timeline.getByRole("button", { name: "Last eldre meldingar" }).evaluate(button => (button as HTMLButtonElement).click());
  // WebKit can publish an additional top-edge scroll after restoration. It may
  // fetch the final page too; the same message content must remain anchored.
  await expect.poll(() => server.timeline.locator("[data-message-id]").count()).toBeGreaterThanOrEqual(100);
  expect(server.requests.slice(0, 3).map(command => command.payload.before)).toEqual([162, 112, 62]);
  const anchorOffset = () => server.timeline.locator(`[data-message-id="${anchor.id}"]`).evaluate(element =>
    element.getBoundingClientRect().top - element.parentElement!.getBoundingClientRect().top);
  await expect.poll(async () => Math.abs(await anchorOffset() - anchor.offset)).toBeLessThan(3);
  // Loading media above the viewport changes layout after the page was inserted.
  await server.timeline.locator("[data-message-id]").first().evaluate(element => {
    const image = document.createElement("img"); image.alt = "Historisk bilete";
    image.src = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='240' height='300'%3E%3C/svg%3E";
    image.style.height = "300px"; element.append(image);
  });
  await expect.poll(async () => Math.abs(await anchorOffset() - anchor.offset)).toBeLessThan(3);
  const older = server.timeline.getByRole("button", { name: "Last eldre meldingar" });
  if (await older.count()) await older.evaluate(button => (button as HTMLButtonElement).click());
  await expect(server.timeline.locator("[data-message-id]")).toHaveCount(111);
  await expect(server.timeline).toContainText("Starten på samtalen");
});

test("history errors retry the same cursor and late pages cannot contaminate channel changes", async ({ page }) => {
  const server = await historyServer(page, Array.from({ length: 110 }, (_, index) => message(index + 1)));
  server.mode("error");
  await server.timeline.getByRole("button", { name: "Last eldre meldingar" }).evaluate(button => (button as HTMLButtonElement).click());
  await expect(server.timeline).toContainText("Kunne ikkje laste eldre meldingar");
  await server.timeline.getByRole("button", { name: "Prøv igjen" }).evaluate(button => (button as HTMLButtonElement).click());
  await expect(server.timeline.locator("[data-message-id]")).toHaveCount(100);
  expect(server.requests.slice(0, 2).map(command => command.payload.before)).toEqual([61, 61]);
  server.mode("hold");
  await server.timeline.getByRole("button", { name: "Last eldre meldingar" }).evaluate(button => (button as HTMLButtonElement).click());
  await expect.poll(() => server.requests.length).toBe(3);
  await selectConversation(page, "# empty");
  await expect(server.timeline).toContainText("Ingen meldingar enno.");
  // Return to the same channel before the stale response: channel id alone is
  // insufficient to correlate a response with the current selection.
  await selectConversation(page, "# history");
  await expect(server.timeline.locator("[data-message-id]")).toHaveCount(50);
  server.release();
  await expect(server.timeline.locator("[data-message-id]")).toHaveCount(50);
  await server.timeline.getByRole("button", { name: "Last eldre meldingar" }).evaluate(button => (button as HTMLButtonElement).click());
  await expect(server.timeline.locator("[data-message-id]")).toHaveCount(100);
});

test("a lost history request does not lock paging after reconnect", async ({ page }) => {
  const server = await historyServer(page, Array.from({ length: 110 }, (_, index) => message(index + 1)));
  server.mode("hold");
  await server.timeline.getByRole("button", { name: "Last eldre meldingar" }).evaluate(button => (button as HTMLButtonElement).click());
  await expect.poll(() => server.requests.length).toBe(1);
  server.mode("normal"); server.disconnect();
  await expect(server.timeline.getByRole("button", { name: "Last eldre meldingar" })).toBeEnabled({ timeout: 15_000 });
  await server.timeline.getByRole("button", { name: "Last eldre meldingar" }).evaluate(button => (button as HTMLButtonElement).click());
  await expect(server.timeline.locator("[data-message-id]")).toHaveCount(100);
  expect(server.requests.map(command => command.payload.before)).toEqual([61, 61]);
});

test("a timed-out notification history search resumes and reveals its target on retry", async ({ page }) => {
  await page.clock.install();
  const server = await historyServer(page, Array.from({ length: 110 }, (_, index) => message(index + 1)), {
    link: "&message=history-1&sequence=1", hold: true
  });
  await expect.poll(() => server.requests.length).toBe(1);
  // The route observes the outbound frame before the client's send call has
  // necessarily returned and installed its timeout. Let that task finish.
  await page.evaluate(() => new Promise<void>(resolve => setTimeout(resolve, 0)));
  await page.clock.fastForward(20_001);
  await page.clock.runFor(100);
  await expect(server.timeline).toContainText("Lastinga tok for lang tid");
  server.mode("normal");
  await server.timeline.getByRole("button", { name: "Prøv igjen" }).click();
  await expect(server.timeline.locator("[data-message-id]")).toHaveCount(110);
  expect(server.requests.map(command => command.payload.before)).toEqual([61, 61]);
  await expect.poll(() => server.timeline.locator('[data-message-id="history-1"]').evaluate(element => {
    const top = element.getBoundingClientRect().top - element.parentElement!.getBoundingClientRect().top;
    return top >= 0 && top < 40;
  })).toBe(true);
});
