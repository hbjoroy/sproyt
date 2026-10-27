import { destinationSurface } from "./support/destination-surface";
import { expect, test, type Locator, type Page, type WebSocketRoute } from "@playwright/test";
import type { ChatMessage } from "../src/types";

test.setTimeout(45_000);
const circleId = "00000000-0000-7000-8000-000000008001";
const channelA = "00000000-0000-7000-8000-000000008101";
const channelB = "00000000-0000-7000-8000-000000008102";
const owner = "view-history-reader";
const messageId = (sequence: number, channel = channelA) => `view-${channel === channelA ? "a" : "b"}-${sequence}`;
const messageHref = (sequence: number) => `/?channel=${channelA}&message=${messageId(sequence)}&sequence=${sequence}`;
type Command = { type: string; request_id?: string; payload?: {
  channel_id?: string; root_message_id?: string; before?: number; limit?: number; sequence?: number;
} };

function root(sequence: number, channel = channelA): ChatMessage {
  return { id: messageId(sequence, channel), channel_id: channel, parent_message_id: null,
    sender_id: "peer", sender_display_name: "Ada", sequence, sent_at: "2026-01-01T12:00:00Z",
    edited_at: null, deleted_at: null,
    body: `[Til melding 20](${messageHref(20)}) · [Til melding 60](${messageHref(60)})\n\nMelding ${sequence}. ${"Her er samanhengen i samtalen. ".repeat(24)}` };
}

/** A real application/DOM fixture. The socket exposes only this account's two
 * member channels; history pages, thread data and read receipts use real wire
 * messages. Layout and reading offsets are never replaced by fake geometry. */
async function fixture(page: Page, start = `channel=${channelA}`) {
  await page.setViewportSize({ width: 1440, height: 900 });
  const messages = new Map([channelA, channelB].map(channel => [channel, Array.from({ length: 100 }, (_, index) => root(index + 1, channel))]));
  const reads = new Map([[channelA, 100], [channelB, 100]]);
  const commands: Command[] = [];
  const errors: string[] = [];
  let socket: WebSocketRoute;
  const emit = (type: string, payload: unknown = {}, request_id?: string) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload, request_id }));
  const channels = () => [...messages].map(([id, list]) => ({ id, slug: id, name: id === channelA ? "Prat" : "Tur",
    circle_id: circleId, kind: "public", direct_user_id: null, is_direct: false, description: "",
    role: "member", last_read_sequence: reads.get(id), latest_sequence: list.at(-1)!.sequence }));
  page.on("pageerror", error => errors.push(error.message));
  await page.route("**/api/v1/me/notifications?*", route => route.fulfill({ json: {
    enabled: false, subscriptions: 0, public_key: null, channel_ids: [],
    preferences: { mode: "instant", direct_messages: true, mentions: true }
  } }));
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    socket = route;
    socket.onMessage(data => {
      const command = JSON.parse(String(data)) as Command;
      commands.push(command);
      const reply = (type: string, payload: unknown = {}) => emit(type, payload, command.request_id);
      const channel = command.payload?.channel_id ?? channelA;
      switch (command.type) {
        case "hello": reply("hello", { participant_id: owner }); break;
        case "ping": reply("pong"); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [[{
          id: circleId, name: "Paros", slug: "paros", created_by: "peer", created_at: "2026-01-01T00:00:00Z"
        }, "member"]] }); break;
        case "list_my_channels": reply("channels_listed", { channels: channels() }); break;
        case "list_mentions": reply("mentions_listed", { mentions: [{ read: false, channel_name: "Prat", message: messages.get(channelA)![19] }] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "subscribe_channel": {
          const list = messages.get(channel);
          if (!list) throw new Error(`Fixture denies channel ${channel}`);
          reply("subscription_started", { channel_id: channel, history: list.slice(-50) }); break;
        }
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
        case "load_recent_messages": {
          const list = messages.get(channel);
          if (!list) throw new Error(`Fixture denies channel ${channel}`);
          reply("messages_loaded", { channel_id: channel, messages: list.filter(message => message.sequence < (command.payload?.before ?? Infinity)).slice(-(command.payload?.limit ?? 50)) }); break;
        }
        case "load_thread": {
          const parent = [...messages.values()].flat().find(message => message.id === command.payload?.root_message_id);
          if (!parent) throw new Error("Fixture denies unknown thread");
          reply("thread_loaded", { root_message_id: parent.id, messages: [parent, ...Array.from({ length: 12 }, (_, index) => ({
            ...parent, id: `${parent.id}-reply-${index + 1}`, parent_message_id: parent.id, sequence: 201 + index,
            body: `Svar ${index + 1} i tråden. ${"Meir samanheng. ".repeat(35)}`
          }))] }); break;
        }
        case "mark_read": {
          reads.set(channel, Math.max(reads.get(channel) ?? 0, command.payload?.sequence ?? 0));
          reply("read_marker_updated", { membership: { channel_id: channel, user_id: owner, role: "member", last_read_sequence: reads.get(channel), joined_at: "2026-01-01T00:00:00Z" } }); break;
        }
        case "mark_thread_read": reply("thread_read_updated", { summary: { root_message_id: command.payload?.root_message_id,
          reply_count: 12, unread_count: 0, latest_sequence: 212 } }); break;
      }
    });
  });
  await page.goto(`/?participant=view-history&ui=react&${start}`);
  const app = page.locator("#sproyt-react-preview");
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(composer).toBeEnabled();
  return { app, composer, commands, errors,
    append(channel = channelA) {
      const list = messages.get(channel)!;
      const message = root(list.at(-1)!.sequence + 1, channel); list.push(message);
      emit("chat", { event: { type: "message_accepted", message } });
      return message;
    },
    refresh() { emit("channels_listed", { channels: channels() }); }
  };
}

const timeline = (page: Page) => page.locator("#sproyt-react-preview .sp-channel-pane > .sp-timeline");
const article = (page: Page, sequence: number) => timeline(page).locator(`[data-message-id="${messageId(sequence)}"]`);
const frame = (page: Page) => page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
async function anchor(viewport: Locator) {
  return viewport.evaluate(element => {
    const bounds = element.getBoundingClientRect();
    const message = [...element.querySelectorAll<HTMLElement>("[data-message-id]")].find(item => item.getBoundingClientRect().bottom > bounds.top + 1);
    if (!message) throw new Error("No real DOM message intersects the reading viewport");
    return { id: message.dataset.messageId!, offset: message.getBoundingClientRect().top - bounds.top };
  });
}
async function expectAnchor(viewport: Locator, saved: { id: string; offset: number }) {
  await expect.poll(async () => {
    const item = viewport.locator(`[data-message-id="${saved.id}"]`);
    if (!await item.count()) return Infinity;
    return Math.abs(await item.evaluate(element => element.getBoundingClientRect().top - element.closest(".sp-timeline")!.getBoundingClientRect().top) - saved.offset);
  }).toBeLessThan(3);
}
async function readingAt(page: Page, sequence: number) {
  await expect(article(page, sequence)).toBeAttached();
  await frame(page);
  await timeline(page).evaluate((element, id) => {
    const message = element.querySelector<HTMLElement>(`[data-message-id="${id}"]`)!;
    element.dispatchEvent(new WheelEvent("wheel", { deltaY: -400 }));
    element.scrollTop += message.getBoundingClientRect().top - element.getBoundingClientRect().top - 24;
    element.dispatchEvent(new Event("scroll"));
  }, messageId(sequence));
  await frame(page);
}
async function expectRevealed(page: Page, sequence: number) {
  await expect(article(page, sequence)).toBeAttached();
  await expect.poll(() => article(page, sequence).evaluate(element => {
    const viewport = element.closest(".sp-timeline")!.getBoundingClientRect();
    const top = element.getBoundingClientRect().top - viewport.top;
    return top >= -1 && top < Math.min(130, viewport.height);
  })).toBe(true);
}
async function openSettings(app: Locator) {
  const trigger = app.getByRole("button", { name: "Innstillingar", exact: true });

  await trigger.click();
}

test("browser Back and Forward restore distinct reading positions within the same channel", async ({ page }) => {
  const { composer, errors } = await fixture(page);
  await composer.fill("Utkast ved to lesestader");
  await readingAt(page, 95);
  const link = article(page, 95).getByRole("link", { name: "Til melding 20", exact: true });
  await link.scrollIntoViewIfNeeded(); await frame(page);
  const first = await anchor(timeline(page));
  const firstUrl = page.url();
  await link.click();
  await expect.poll(() => new URL(page.url()).searchParams.get("message")).toBe(messageId(20));
  await expectRevealed(page, 20);
  const second = await anchor(timeline(page));
  expect(first.id).not.toBe(second.id);
  await page.goBack();
  await expect(page).toHaveURL(firstUrl);
  await expectAnchor(timeline(page), first);
  await page.goForward();
  await expect.poll(() => new URL(page.url()).searchParams.get("message")).toBe(messageId(20));
  await expectAnchor(timeline(page), second);
  await expect(composer).toHaveValue("Utkast ved to lesestader");
  expect(errors).toEqual([]);
});

test("browser thread traversal restores the channel position and both unsent drafts", async ({ page }) => {
  const { app, composer, errors } = await fixture(page);
  await composer.fill("Uferdig kanalutkast");
  await readingAt(page, 95);
  const trigger = article(page, 95).locator(`[data-thread-trigger="${messageId(95)}"]`);
  await trigger.scrollIntoViewIfNeeded(); await frame(page);
  const saved = await anchor(timeline(page));
  await trigger.click();
  const thread = app.locator(".sp-thread-pane");
  const reply = thread.getByRole("textbox", { name: "Svar i tråden", exact: true });
  await expect(reply).toBeEnabled();
  await reply.fill("Uferdig svarutkast");
  await expect.poll(() => new URL(page.url()).searchParams.get("thread")).toBe(messageId(95));
  await page.goBack();
  await expect(thread).toHaveCount(0);
  await expectAnchor(timeline(page), saved);
  await expect(composer).toHaveValue("Uferdig kanalutkast");
  await page.goForward();
  await expect(reply).toHaveValue("Uferdig svarutkast");
  await expect(composer).toHaveValue("Uferdig kanalutkast");
  expect(errors).toEqual([]);
});

test("canonical circle settings survive reload with one modal and no mutation", async ({ page }) => {
  const { app, commands, errors } = await fixture(page);
  await openSettings(app);
  await destinationSurface(app, "Innstillingar", true).getByRole("button", { name: "Administrer krets", exact: true }).click();
  await destinationSurface(app, "Vel krets", true).getByRole("button", { name: "Paros", exact: true }).click();
  const canonical = page.url();
  expect(new URL(canonical).searchParams.get("view")).toBe("settings");
  expect(new URL(canonical).searchParams.get("section")).toBe("circle");
  expect(new URL(canonical).searchParams.get("circle")).toBe(circleId);
  await page.reload();
  await expect(destinationSurface(app, "Paros", true)).toBeVisible();
  await expect(app.locator("[data-workspace-page], dialog[open]")).toHaveCount(1);
  await expect(page).toHaveURL(canonical);
  await page.keyboard.press("Escape");
  await expect(destinationSurface(app, "Vel krets", true)).toBeVisible();
  await expect(app.locator("[data-workspace-page], dialog[open]")).toHaveCount(1);
  expect(commands.filter(command => /^(create_|accept_|enable_|start_)/.test(command.type))).toEqual([]);
  expect(errors).toEqual([]);
});

test("inbox source navigation and browser return preserve the conversation draft and reading context", async ({ page }) => {
  const { app, composer, errors } = await fixture(page);
  await composer.fill("Ikkje mist innboksutkastet");
  await readingAt(page, 95);
  const saved = await anchor(timeline(page));
  await app.getByRole("button", { name: /^Innboks,/ }).click();
  const inbox = destinationSurface(app, "Innboks", true);
  await expect.poll(() => new URL(page.url()).searchParams.get("view")).toBe("inbox");
  await expect.poll(() => new URL(page.url()).searchParams.get("inbox")).toBe("mentions");
  await inbox.getByRole("button", { name: /^Uleste/ }).click();
  await expect.poll(() => new URL(page.url()).searchParams.get("inbox")).toBe("unread");
  await inbox.getByRole("button", { name: /^Omtalar/ }).click();
  await expect.poll(() => new URL(page.url()).searchParams.get("inbox")).toBe("mentions");
  await expect(inbox.getByRole("button", { name: "Opne kjelda", exact: true })).toBeVisible();
  await inbox.getByRole("button", { name: "Opne kjelda", exact: true }).click();
  await expect(inbox).toHaveCount(0);
  await expectRevealed(page, 20);
  await expect(composer).toHaveValue("Ikkje mist innboksutkastet");
  await page.goBack();
  await expect(inbox).toBeVisible();
  await expect.poll(() => new URL(page.url()).searchParams.get("inbox")).toBe("mentions");
  await expect(inbox.getByRole("button", { name: /^Omtalar/ })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect.poll(() => new URL(page.url()).searchParams.get("inbox")).toBe("unread");
  await expect(inbox.getByRole("button", { name: /^Uleste/ })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect.poll(() => new URL(page.url()).searchParams.get("inbox")).toBe("mentions");
  await expect(inbox.getByRole("button", { name: /^Omtalar/ })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect(inbox).toHaveCount(0);
  await expectAnchor(timeline(page), saved);
  await expect(composer).toHaveValue("Ikkje mist innboksutkastet");
  expect(errors).toEqual([]);
});

test("a second same-channel message link reveals its own target without resubscribing", async ({ page }) => {
  const { commands, errors } = await fixture(page, `channel=${channelA}&message=${messageId(20)}&sequence=20`);
  await expectRevealed(page, 20);
  const subscriptions = commands.filter(command => command.type === "subscribe_channel").length;
  await article(page, 20).getByRole("link", { name: "Til melding 60", exact: true }).click();
  await expect.poll(() => new URL(page.url()).searchParams.get("message")).toBe(messageId(60));
  await expectRevealed(page, 60);
  expect(commands.filter(command => command.type === "subscribe_channel")).toHaveLength(subscriptions);
  expect(errors).toEqual([]);
});

test("incoming events, channel refresh and reload do not add browser history entries", async ({ page }) => {
  const server = await fixture(page);
  await readingAt(page, 95);
  const saved = await anchor(timeline(page));
  const before = await page.evaluate(() => ({ length: history.length, id: history.state?.sproytView?.id, href: location.href }));
  const active = server.append(); server.append(channelB); server.refresh();
  await expect(timeline(page).locator(`[data-message-id="${active.id}"]`)).toHaveCount(1);
  await expectAnchor(timeline(page), saved);
  await expect.poll(() => page.evaluate(() => ({ length: history.length, id: history.state?.sproytView?.id, href: location.href }))).toEqual(before);
  await page.reload();
  await expect(server.composer).toBeEnabled();
  await expectAnchor(timeline(page), saved);
  expect(await page.evaluate(() => ({ length: history.length, id: history.state?.sproytView?.id, href: location.href }))).toEqual(before);
  expect(server.errors).toEqual([]);
});
