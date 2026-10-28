import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";
import { destinationSurface } from "./support/destination-surface";

const channel = "00000000-0000-7000-8000-000000009101";
const circle = "00000000-0000-7000-8000-000000009001";
const owner = "thread-directory-reader";
const id = (n: number) => `00000000-0000-7000-8000-${String(n).padStart(12, "0")}`;
type Command = { type: string; request_id?: string; payload?: { channel_id?: string; root_message_id?: string; sequence?: number } };

async function fixture(page: Page) {
  const commands: Command[] = [];
  const batches: string[][] = [];
  const errors: string[] = [];
  let changedWhileOffline = false;
  const replies = Array.from({ length: 35 }, (_, i) => ({ root_message_id: id(i + 1), reply_count: 1,
    unread_count: 1, latest_sequence: 1001 + i }));
  const root = (n: number) => ({ id: id(n), channel_id: channel, parent_message_id: null, sender_id: "peer",
    sender_display_name: "Ada", sequence: n, sent_at: "2026-01-01T12:00:00Z", edited_at: null, deleted_at: null,
    body: `Gammal trådrot ${n}` });
  let socket: WebSocketRoute;
  const emit = (type: string, payload: unknown, requestId?: string) => socket.send(JSON.stringify({
    protocol: "sproyt.chat.v1", type, payload, request_id: requestId
  }));
  page.on("pageerror", error => errors.push(error.message));
  await page.route("**/api/v1/me/notifications?*", route => route.fulfill({ json: {
    enabled: false, subscriptions: 0, public_key: null, channel_ids: [],
    preferences: { mode: "instant", direct_messages: true, mentions: true }
  } }));
  await page.route("**/api/v1/channels/*/thread-root-previews**", route => {
    const request = route.request();
    const ids = (request.postDataJSON() as { root_ids: string[] }).root_ids;
    expect(request.method()).toBe("POST");
    expect(request.headers()["x-sproyt-account"]).toBe(owner);
    batches.push(ids);
    return route.fulfill({ json: { roots: ids.map(rootId => {
      const number = Number(rootId.slice(-12));
      const deleted = changedWhileOffline && number === 34;
      return { id: rootId, author: deleted ? null : "Ada",
        preview: changedWhileOffline && number === 35 ? "Redigert medan fråkopla" : deleted ? "" : `Gammal trådrot ${number}`,
        sent_at: "2026-01-01T12:00:00Z", deleted };
    }) } });
  });
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    socket = route;
    socket.onMessage(data => {
      const command = JSON.parse(String(data)) as Command;
      commands.push(command);
      const reply = (type: string, payload: unknown = {}) => emit(type, payload, command.request_id);
      switch (command.type) {
        case "hello": reply("hello", { participant_id: owner }); break;
        case "ping": reply("pong"); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [[{ id: circle, name: "Krets", slug: "krets",
          created_by: "peer", created_at: "2026-01-01T00:00:00Z" }, "member"]] }); break;
        case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, slug: "prat", name: "Prat",
          circle_id: circle, kind: "public", direct_user_id: null, is_direct: false, description: "",
          role: "member", last_read_sequence: 2000, latest_sequence: 2000 }] }); break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        // The older roots are deliberately absent from the currently loaded history.
        case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: [root(100)] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: replies }); break;
        case "load_thread": {
          const n = Number(command.payload?.root_message_id?.slice(-12));
          const parent = root(n);
          reply("thread_loaded", { root_message_id: parent.id, messages: [parent, { ...parent,
            id: id(5000 + n), parent_message_id: parent.id, sequence: 2000 + n, body: `Svar i gammal tråd ${n}` }] }); break;
        }
        case "mark_thread_read": reply("thread_read_updated", { summary: { root_message_id: command.payload?.root_message_id,
          reply_count: 1, unread_count: 0, latest_sequence: command.payload?.sequence } }); break;
      }
    });
  });
  await page.goto(`/?participant=thread-directory&ui=react&channel=${channel}`);
  const app = page.locator("#sproyt-react-preview");
  await expect(app.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  return { app, batches, commands, errors, disconnectWithChanges: () => { changedWhileOffline = true; socket.close({ code: 1012, reason: "test reconnect" }); } };
}

for (const mobile of [false, true]) {
  test.describe(mobile ? "mobile thread overview" : "desktop thread overview", () => {
    test.use({ hasTouch: mobile, isMobile: mobile, viewport: { width: mobile ? 390 : 1440, height: 900 } });
    test("fetches 30 then 5 older roots without marking them read, and opens an older thread", async ({ page }) => {
      const { app, batches, commands, errors } = await fixture(page);
      if (mobile) await app.locator(".sp-mobile-conversation-title").tap();
      const trigger = mobile
        ? app.locator(".sp-mobile-conversation-name").getByRole("button", { name: /^Trådar i samtalen/ })
        : app.locator(".sp-context").getByRole("button", { name: /^Trådar i samtalen/ });
      await expect(trigger).toBeVisible();
      if (mobile) await trigger.tap(); else await trigger.click();
      const directory = destinationSurface(app, "Trådar i samtalen", true);
      await expect(directory).toBeVisible();
      await expect.poll(() => batches.map(batch => batch.length)).toEqual([30]);
      await expect(directory.getByText("Gammal trådrot 35", { exact: true })).toBeVisible();
      await expect(directory.getByText("Gammal trådrot 1", { exact: true })).toHaveCount(0);
      expect(commands.filter(command => command.type === "mark_thread_read")).toEqual([]);
      await directory.getByRole("button", { name: "Last fleire trådar", exact: true }).click();
      await expect.poll(() => batches.map(batch => batch.length)).toEqual([30, 5]);
      expect(new Set(batches.flat()).size).toBe(35);
      await expect(directory.getByText("Gammal trådrot 1", { exact: true })).toBeVisible();
      expect(commands.filter(command => command.type === "mark_thread_read")).toEqual([]);
      await directory.getByRole("button", { name: /Gammal trådrot 1(?:\s|$)/ }).click();
      await expect.poll(() => commands.filter(command => command.type === "load_thread").map(command => command.payload?.root_message_id)).toContain(id(1));
      await expect(app.getByText("Svar i gammal tråd 1", { exact: true })).toBeVisible();
      expect(new URL(page.url()).searchParams.get("thread")).toBe(id(1));
      expect(errors).toEqual([]);
    });
  });
}


test("reconnection refreshes thread previews edited or deleted while offline", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const { app, batches, commands, errors, disconnectWithChanges } = await fixture(page);
  await app.locator(".sp-context").getByRole("button", { name: /^Trådar i samtalen/ }).click();
  const directory = destinationSurface(app, "Trådar i samtalen", true);
  await expect(directory.getByText("Gammal trådrot 35", { exact: true })).toBeVisible();
  await expect(directory.getByText("Gammal trådrot 34", { exact: true })).toBeVisible();
  expect(batches).toHaveLength(1);
  disconnectWithChanges();
  await expect.poll(() => commands.filter(command => command.type === "subscribe_channel").length).toBeGreaterThanOrEqual(2);
  await expect.poll(() => batches.length).toBeGreaterThanOrEqual(2);
  await expect(directory.getByText("Redigert medan fråkopla", { exact: true })).toBeVisible();
  await expect(directory.getByText("Sletta melding", { exact: true })).toBeVisible();
  await expect(directory.getByText("Gammal trådrot 35", { exact: true })).toHaveCount(0);
  await expect(directory.getByText("Gammal trådrot 34", { exact: true })).toHaveCount(0);
  expect(commands.filter(command => command.type === "mark_thread_read")).toEqual([]);
  expect(errors).toEqual([]);
});
