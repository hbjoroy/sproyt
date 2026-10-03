import { expect as baseExpect, test, type Page, type WebSocketRoute } from "@playwright/test";
import type { ChatMessage } from "../src/types";
import { readFileSync } from "node:fs";
import { transformSync } from "esbuild";
const expect = baseExpect.configure({ timeout: 15_000 });
test.setTimeout(60_000);

test("coalesced host publications retain bottom following until the DOM commit", async ({ page }) => {
  await page.setContent('<div id="timeline" style="height:400px;width:400px;overflow:auto"><div data-message-id="first" style="height:2000px"></div></div>');
  await page.addScriptTag({ content: transformSync(readFileSync(new URL("../src/ui/react/timeline-scroll.ts", import.meta.url), "utf8"),
    { loader: "ts", format: "iife", globalName: "ReadingController" }).code });
  const result = await page.evaluate(async () => {
    const api = (window as unknown as { ReadingController: typeof import("../src/ui/react/timeline-scroll") }).ReadingController;
    const controller = api.createTimelineScrollController();
    const timeline = document.getElementById("timeline")!;
    const frame = () => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    controller.prepare({ key: "channel:a", messageIds: ["first"] });
    controller.viewportRef(timeline);
    await frame();
    const next = { key: "channel:a", messageIds: ["first", "last"] };
    controller.prepare(next);
    controller.prepare(next);
    const last = document.createElement("div");
    last.dataset.messageId = "last"; last.style.height = "200px";
    timeline.append(last);
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    const distance = timeline.scrollHeight - timeline.scrollTop - timeline.clientHeight;
    const inputOffsets: number[] = [];
    for (const type of ["keydown", "touchstart", "pointerdown"]) {
      controller.goToLatest();
      await frame();
      const id = `input-${type}`;
      const ids = [...timeline.querySelectorAll<HTMLElement>("[data-message-id]")].map(element => element.dataset.messageId!);
      controller.prepare({ key: "channel:a", messageIds: [...ids, id] });
      const tail = document.createElement("div"); tail.dataset.messageId = id; tail.style.height = "40px";
      timeline.append(tail);
      timeline.dispatchEvent(type === "keydown" ? new KeyboardEvent(type, { key: "PageUp" }) : new Event(type));
      timeline.scrollTop -= 10;
      controller.onScroll();
      const firstTop = timeline.scrollTop;
      const published = { key: "channel:a", messageIds: [...ids, id] };
      controller.prepare(published);
      const incoming = document.createElement("div"); incoming.dataset.messageId = `incoming-${type}`; incoming.style.height = "20px";
      controller.prepare({ ...published, messageIds: [...published.messageIds, incoming.dataset.messageId] });
      timeline.append(incoming);
      await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
      inputOffsets.push(Math.abs(timeline.scrollTop - firstTop));
      timeline.scrollTop -= 290;
      controller.onScroll();
      const top = timeline.scrollTop;
      await frame();
      inputOffsets.push(Math.abs(timeline.scrollTop - top));
    }
    controller.dispose();
    return { distance, inputOffsets };
  });
  expect(result.distance).toBeLessThan(2);
  expect(result.inputOffsets.every(offset => offset < 2)).toBe(true);
});

type Command = { type: string; request_id: string; payload: { channel_id?: string; before?: number; limit?: number; sequence?: number; root_message_id?: string } };
const root = (channel: string, sequence: number): ChatMessage => ({ id: `${channel}-${sequence}`, channel_id: channel,
  parent_message_id: null, sender_id: "reader", sender_display_name: "Lesar", body: `Melding ${sequence} ${"kontekst ".repeat(55)}`,
  sequence, sent_at: "2025-01-01T12:00:00Z", edited_at: null, deleted_at: null });
const viewport = (page: Page) => page.locator("#sproyt-react-preview .sp-channel-pane > .sp-timeline");
async function select(page: Page, channel: string) {
  const back = page.getByRole("button", { name: "Samtalar", exact: true });
  if (await back.isVisible()) await back.click();
  await page.getByRole("button", { name: new RegExp(`^# ${channel}(?: \\d+ uleste)?$`) }).click();
}
async function anchor(page: Page) {
  return viewport(page).evaluate(element => {
    const bounds = element.getBoundingClientRect();
    const message = [...element.querySelectorAll<HTMLElement>("[data-message-id]")].find(item => item.getBoundingClientRect().bottom > bounds.top + 1)!;
    return { id: message.dataset.messageId!, offset: message.getBoundingClientRect().top - bounds.top };
  });
}
async function expectAnchor(page: Page, saved: { id: string; offset: number }) {
  await expect.poll(async () => {
    const item = viewport(page).locator(`[data-message-id="${saved.id}"]`);
    if (!await item.count()) return Infinity;
    return Math.abs(await item.evaluate(element => element.getBoundingClientRect().top - element.parentElement!.getBoundingClientRect().top) - saved.offset);
  }).toBeLessThan(3);
}

function readingServer(initialRead = 40, channelMessages?: ChatMessage[]) {
  const messages = new Map(["a", "b"].map(channel => [channel, Array.from({ length: 160 }, (_, index) => root(channel, index + 1))]));
  if (channelMessages) messages.set("a", channelMessages);
  const reads = new Map([["a", initialRead], ["b", 160]]);
  const sockets = new Map<Page, WebSocketRoute>();
  const acknowledgements: { page: Page; channel: string; sequence: number }[] = [];
  const pages: Command[] = [];
  const threadAcknowledgements: number[] = [];
  const replies = Array.from({ length: 20 }, (_, index) => ({ ...root("a", 161 + index), id: `reply-${index}`, parent_message_id: "a-41" }));
  let hold = false;
  const held: (() => void)[] = [];
  const emit = (socket: WebSocketRoute, type: string, payload: unknown, request_id?: string) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload, request_id }));
  return {
    reads, acknowledgements, threadAcknowledgements, pages,
    hold: (value: boolean) => { hold = value; },
    release: () => { hold = false; held.splice(0).forEach(respond => respond()); },
    disconnect: (page: Page) => sockets.get(page)!.close({ code: 1012, reason: "reading reconnect" }),
    marker: (channel: string, sequence: number) => {
      reads.set(channel, Math.max(reads.get(channel) ?? 0, sequence));
      for (const socket of sockets.values()) emit(socket, "chat", { event: { type: "read_marker_updated", channel_id: channel, user_id: "reader", sequence } });
    },
    append: (channel: string) => {
      const list = messages.get(channel)!;
      const message = root(channel, list.length + 1); list.push(message);
      for (const socket of sockets.values()) emit(socket, "chat", { event: { type: "message_accepted", message } });
      return message;
    },
    appendReply: () => {
      const message = { ...root("a", replies.at(-1)!.sequence + 1), id: `reply-${replies.length}`, parent_message_id: "a-41" };
      replies.push(message);
      for (const socket of sockets.values()) emit(socket, "chat", { event: { type: "message_accepted", message } });
      return message;
    },
    install: async (page: Page, link = "") => {
      await page.routeWebSocket(/\/ws(?:\?|$)/, socket => {
        sockets.set(page, socket);
        socket.onMessage(data => {
          const command = JSON.parse(String(data)) as Command;
          const reply = (type: string, payload: unknown) => emit(socket, type, payload, command.request_id);
          const channel = command.payload?.channel_id ?? "a";
          switch (command.type) {
            case "hello": reply("hello", { participant_id: "reader" }); break;
            case "ping": emit(socket, "pong", undefined); break;
            case "list_users": reply("users_listed", { users: [] }); break;
            case "list_my_circles": reply("circles_listed", { circles: [] }); break;
            case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
            case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
            case "list_my_channels": reply("channels_listed", { channels: [...messages].map(([id, list]) => ({
              id, slug: id, name: id, kind: "public", circle_id: null, direct_user_id: null, is_direct: false,
              description: "", role: "member", last_read_sequence: reads.get(id), latest_sequence: list.length
            })) }); break;
            case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: messages.get(channel)!.slice(-50) }); break;
            case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
            case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
            case "load_thread": {
              const respond = () => reply("thread_loaded", { root_message_id: "a-41", messages: [root("a", 41), ...replies] });
              if (hold) held.push(respond); else respond();
              break;
            }
            case "mark_thread_read":
              threadAcknowledgements.push(command.payload.sequence!);
              reply("thread_read_updated", { summary: { root_message_id: "a-41", reply_count: replies.length,
                unread_count: 0, latest_sequence: replies.at(-1)!.sequence } }); break;
            case "load_recent_messages": {
              pages.push(command);
              const respond = () => reply("messages_loaded", { channel_id: channel,
                messages: messages.get(channel)!.filter(item => item.sequence < (command.payload.before ?? Infinity)).slice(-(command.payload.limit ?? 50)) });
              if (hold) held.push(respond); else respond();
              break;
            }
            case "mark_read": {
              const sequence = command.payload.sequence!;
              acknowledgements.push({ page, channel, sequence });
              reads.set(channel, Math.max(reads.get(channel) ?? 0, sequence));
              reply("read_marker_updated", { membership: { channel_id: channel, user_id: "reader", role: "member", joined_at: "2025-01-01T00:00:00Z", last_read_sequence: reads.get(channel) } });
              for (const peer of sockets.values()) emit(peer, "chat", { event: { type: "read_marker_updated", channel_id: channel, user_id: "reader", sequence: reads.get(channel) } });
              break;
            }
          }
        });
      });
      await page.goto(`/?participant=reading-regression&channel=a${link}`);
      await expect(page.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
      await page.bringToFront();
    }
  };
}

test("opening pages to the unread boundary, acknowledges only visible messages and reload uses the saved watermark", async ({ page }) => {
  const server = readingServer(); server.hold(true);
  await server.install(page);
  expect(server.acknowledgements).toHaveLength(0);
  await expect.poll(() => server.pages.length).toBe(1);
  expect(server.acknowledgements).toHaveLength(0);
  server.release();
  await expect(viewport(page).locator('[data-message-id="a-41"]')).toHaveCount(1);
  await expect.poll(() => viewport(page).locator('[data-message-id="a-41"]').evaluate(element => element.getBoundingClientRect().top - element.parentElement!.getBoundingClientRect().top)).toBeGreaterThan(40);
  await expect.poll(() => server.reads.get("a")!).toBeGreaterThan(40);
  expect(server.reads.get("a")).toBeLessThan(50);
  await expect(viewport(page).getByRole("separator", { name: "Uleste meldingar" })).toBeVisible();
  const persisted = server.reads.get("a")!;
  await page.reload();
  await expect.poll(() => viewport(page).locator(`[data-message-id="a-${persisted + 1}"]`).evaluate(element => {
    const top = element.getBoundingClientRect().top - element.parentElement!.getBoundingClientRect().top;
    return top > 40 && top < 140;
  })).toBe(true);
});

test("loading a hidden thread does not mark replies read and a peer reply preserves the visible thread position", async ({ page }) => {
  const server = readingServer(); await server.install(page);
  await expect.poll(() => server.reads.get("a")!).toBeGreaterThan(40);
  server.hold(true);
  await viewport(page).locator('[data-message-id="a-41"]').getByRole("button", { name: "Svar i tråd", exact: true }).click();
  // Mobile and desktop both hide the conversation through the compact list view.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByRole("button", { name: "Samtalar", exact: true }).click();
  const before = server.reads.get("a");
  server.release();
  await expect(page.locator('.sp-thread-replies [data-message-id="reply-19"]')).toHaveCount(1);
  expect(server.threadAcknowledgements).toHaveLength(0);
  expect(server.reads.get("a")).toBe(before);
  await select(page, "a");
  await expect.poll(() => server.threadAcknowledgements.at(-1)).toBe(180);
  const thread = page.locator("#sproyt-react-preview .sp-thread-replies");
  await thread.evaluate(element => { element.scrollTop = element.scrollHeight / 3; element.dispatchEvent(new Event("scroll")); });
  const saved = await thread.evaluate(element => {
    const top = element.getBoundingClientRect().top;
    const item = [...element.querySelectorAll<HTMLElement>("[data-message-id]")].find(item => item.getBoundingClientRect().bottom > top + 1)!;
    return { id: item.dataset.messageId!, offset: item.getBoundingClientRect().top - top };
  });
  server.appendReply();
  await expect(thread.locator('[data-message-id="reply-20"]')).toHaveCount(1);
  await expect(viewport(page).getByRole("button", { name: "21 svar i tråd", exact: true, includeHidden: true })).toHaveCount(1);
  await expect.poll(async () => Math.abs(await thread.locator(`[data-message-id="${saved.id}"]`).evaluate(element =>
    element.getBoundingClientRect().top - element.closest(".sp-thread-replies")!.getBoundingClientRect().top) - saved.offset)).toBeLessThan(3);
  expect(server.threadAcknowledgements.at(-1)).toBe(180);
  expect(server.reads.get("a")).toBe(180);
});

test("returning across two channels restores an older message and offset, including reconnect and later layout changes", async ({ page }) => {
  const server = readingServer(); await server.install(page);
  await expect.poll(() => server.reads.get("a")!).toBeGreaterThan(40);
  const saved = await anchor(page);
  await select(page, "b");
  await expect.poll(() => anchor(page).then(value => value.id)).toContain("b-");
  await select(page, "a");
  await expectAnchor(page, saved);
  server.disconnect(page);
  await expect(page.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled({ timeout: 15_000 });
  await expectAnchor(page, saved);
  // Late media/reaction layout above the reader must retain the content anchor.
  await viewport(page).locator('[data-message-id="a-11"]').evaluate(element => {
    const image = document.createElement("img"); image.alt = "Seint bilete";
    image.src = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='200' height='300'%3E%3C/svg%3E";
    image.style.height = "300px"; element.append(image);
  });
  await expectAnchor(page, saved);
  const before = server.reads.get("a"); server.append("a");
  await expect(viewport(page).locator('[data-message-id="a-161"]')).toHaveCount(1);
  await expectAnchor(page, saved);
  expect(server.reads.get("a")).toBe(before);
  const latest = page.getByRole("button", { name: "Gå til siste", exact: true }).filter({ visible: true });
  if (!await latest.count()) await page.getByRole("button", { name: "Meny", exact: true }).click();
  await latest.click();
  await expect.poll(() => server.reads.get("a")).toBe(161);
  server.append("a");
  await expect.poll(() => server.reads.get("a")).toBe(162);
});

test("a second client and late lower read markers update badges without moving the first client", async ({ page, context }) => {
  const server = readingServer(); await server.install(page);
  await expect.poll(() => server.reads.get("a")!).toBeGreaterThan(40);
  const saved = await anchor(page);
  const second = await context.newPage();
  await server.install(second, "&message=a-80&sequence=80");
  await second.bringToFront();
  await expect.poll(() => server.reads.get("a")!).toBeGreaterThanOrEqual(80);
  expect(server.reads.get("a")).toBeLessThan(90);
  await page.bringToFront();
  await expectAnchor(page, saved);
  server.marker("a", 10);
  await expectAnchor(page, saved);
  // A new session follows the furthest confirmed marker, not the late lower one.
  const persisted = server.reads.get("a")!;
  await page.reload();
  await expect.poll(() => viewport(page).locator(`[data-message-id="a-${persisted + 1}"]`).evaluate(element => {
    const top = element.getBoundingClientRect().top - element.parentElement!.getBoundingClientRect().top;
    return top > 40 && top < 140;
  })).toBe(true);
  await second.close();
});

test("returning from another channel keeps an anchor near the old bottom when new messages arrived", async ({ page }) => {
  const server = readingServer(160); await server.install(page);
  await expect.poll(() => viewport(page).evaluate(element => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeLessThan(2);
  const saved = await anchor(page);
  await select(page, "b");
  await expect.poll(() => anchor(page).then(value => value.id)).toContain("b-");
  for (let index = 0; index < 5; index++) server.append("a");
  await select(page, "a");
  await expectAnchor(page, saved);
  expect(server.reads.get("a")).toBe(160);
  await expect(page.getByRole("button", { name: /^# a 5 uleste$/, includeHidden: true })).toHaveCount(1);
});

test("channel return waits for a media-clamped anchor instead of following the temporary bottom", async ({ page, isMobile }) => {
  const media = "00000000-0000-7000-8000-000000000045";
  const messages = Array.from({ length: 45 }, (_, index) => ({ ...root("a", index + 1),
    body: `Bilete ${index + 1} [[media:${media}|image/svg+xml|image.svg]]` }));
  const server = readingServer(45, messages);
  let holdMedia = false;
  const heldMedia: (() => void)[] = [];
  await page.route(/\/api\/v1\/media\//, async route => {
    if (holdMedia) await new Promise<void>(resolve => heldMedia.push(resolve));
    await route.fulfill({ contentType: "image/svg+xml", body: '<svg xmlns="http://www.w3.org/2000/svg" width="500" height="500"><rect width="500" height="500" fill="green"/></svg>' });
  });
  await server.install(page);
  await expect.poll(() => viewport(page).locator("img").last().evaluate(image => (image as HTMLImageElement).naturalHeight)).toBeGreaterThan(0);
  await expect.poll(() => viewport(page).evaluate(element => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeLessThan(2);
  // Model native scroll anchoring when media above and below the visible
  // content grows, before the ResizeObserver callback. No user input occurred.
  await viewport(page).evaluate(element => {
    const top = element.scrollTop;
    const messages = element.querySelectorAll<HTMLElement>("[data-message-id]");
    messages[0]!.style.paddingBottom = "100px";
    messages[messages.length - 1]!.style.paddingBottom = "160px";
    element.scrollTop = top + 100;
    element.dispatchEvent(new Event("scroll"));
  });
  await expect.poll(() => viewport(page).evaluate(element => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeLessThan(2);
  // Remove the fixture layout before saving the return anchor.
  await viewport(page).evaluate(element => element.querySelectorAll<HTMLElement>("[data-message-id]").forEach(message => { message.style.paddingBottom = ""; }));
  await expect.poll(() => viewport(page).evaluate(element => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeLessThan(2);
  const theme = page.getByRole("button", { name: "Byt tema", exact: true });
  if (!await theme.isVisible()) await page.getByRole("button", { name: "Meny", exact: true }).click();
  await expect(theme).toBeVisible();
  await expect.poll(() => viewport(page).evaluate(element => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeLessThan(2);
  // Theme changes publish synchronously through the real host. A publication
  // before ResizeObserver must not save this temporary distance from bottom.
  await theme.evaluate(button => {
    const timeline = document.querySelector<HTMLElement>("#sproyt-react-preview .sp-channel-pane > .sp-timeline")!;
    [...timeline.querySelectorAll<HTMLElement>("[data-message-id]")].at(-1)!.style.paddingBottom = "160px";
    (button as HTMLButtonElement).click();
  });
  await expect.poll(() => viewport(page).evaluate(element => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeLessThan(2);
  await viewport(page).evaluate(element => element.querySelectorAll<HTMLElement>("[data-message-id]").forEach(message => { message.style.paddingBottom = ""; }));
  if (await theme.isVisible()) await page.getByRole("button", { name: "Meny", exact: true }).click();
  await expect.poll(() => viewport(page).evaluate(element => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeLessThan(2);
  if (isMobile) {
    await viewport(page).evaluate(async element => {
      await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
      element.scrollTop -= 450;
      element.dispatchEvent(new Event("scroll"));
    });
  } else {
    // A real key must still win when a media resize happens during its input
    // event, before the scroll/resize callbacks can update their geometry.
    await viewport(page).evaluate(element => element.addEventListener("keydown", () => {
      (element.querySelectorAll<HTMLElement>("[data-message-id]").item(44)).style.paddingBottom = "100px";
    }, { once: true }));
    await viewport(page).locator("[data-message-id]").last().getByRole("button", { name: "Fleire meldingsval", exact: true }).press("PageUp");
  }
  await expect.poll(() => viewport(page).evaluate(element => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeGreaterThan(300);
  // PageUp is animated by the browser. Capture only after its scroll settles.
  await expect.poll(async () => {
    const before = await anchor(page);
    await page.waitForTimeout(100);
    const after = await anchor(page);
    return before.id === after.id && Math.abs(before.offset - after.offset) < 1;
  }).toBe(true);
  const saved = await anchor(page);
  await select(page, "b");
  await expect.poll(() => anchor(page).then(value => value.id)).toContain("b-");
  server.append("a");
  // A new media URL avoids WebKit's decoded-image cache while preserving the
  // same final geometry, so both engines exercise an actual delayed layout.
  messages.forEach(message => { message.body = message.body.replace(media, "00000000-0000-7000-8000-000000000046"); });
  holdMedia = true;
  await select(page, "a");
  await expect.poll(() => heldMedia.length).toBeGreaterThan(0);
  await expect(viewport(page).locator('[data-message-id="a-46"]')).toHaveCount(1);
  // Committed wrappers at a clamped bottom must not mark the new tail read.
  expect(server.reads.get("a")).toBe(45);
  holdMedia = false;
  heldMedia.splice(0).forEach(resolve => resolve());
  await expect.poll(() => viewport(page).locator("img").last().evaluate(image => (image as HTMLImageElement).naturalHeight)).toBeGreaterThan(0);
  await expectAnchor(page, saved);
  expect(server.reads.get("a")).toBe(45);
  // A genuinely shorter history may make the old offset impossible. Even a
  // wheel down at the already-clamped bottom must supersede the restore.
  await select(page, "b");
  await expect.poll(() => anchor(page).then(value => value.id)).toContain("b-");
  messages.forEach(message => { message.body = "Kort melding"; });
  await select(page, "a");
  await expect(viewport(page).locator('[data-message-id="a-46"]')).toHaveCount(1);
  expect(server.reads.get("a")).toBe(45);
  await expect.poll(() => viewport(page).evaluate(element => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeLessThan(2);
  const clampedTop = await viewport(page).evaluate(element => element.scrollTop);
  if (isMobile) {
    // Playwright's mobile WebKit has no wheel support; End at the same bottom
    // exercises the identical no-scroll-event input boundary.
    await viewport(page).locator("[data-message-id]").last().getByRole("button", { name: "Fleire meldingsval", exact: true }).press("End");
  } else {
    await viewport(page).hover();
    await page.mouse.wheel(0, 120);
  }
  await expect.poll(() => server.reads.get("a")).toBe(46);
  expect(await viewport(page).evaluate(element => element.scrollTop)).toBe(clampedTop);
  server.append("a");
  await expect.poll(() => server.reads.get("a")).toBe(47);
});
