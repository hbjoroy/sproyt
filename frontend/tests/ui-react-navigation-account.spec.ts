import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";

test.setTimeout(45_000);
const channelId = "00000000-0000-7000-8000-000000007501";
const mediaId = "00000000-0000-7000-8000-000000007502";
const image = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==", "base64");
type Send = { owner: string; payload: { channel_id: string; body: string; parent_message_id?: string | null } };

async function accountFixture(page: Page, holdUpload: boolean) {
  let identity = "alice";
  let socket: WebSocketRoute;
  let started = false;
  let completed = false;
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  const sends: Send[] = [];
  const errors: string[] = [];
  const uploads: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  const emit = (type: string, payload: unknown = {}, request_id?: string) => socket.send(JSON.stringify({
    protocol: "sproyt.chat.v1", type, payload, ...(request_id ? { request_id } : {})
  }));
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    socket = route;
    route.onMessage(data => {
      const command = JSON.parse(String(data));
      const reply = (type: string, payload: unknown = {}) => emit(type, payload, command.request_id);
      switch (command.type) {
        case "hello": reply("hello", { participant_id: identity }); break;
        case "ping": reply("pong"); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [] }); break;
        case "list_my_channels": reply("channels_listed", { channels: [{ id: channelId, slug: "shared-account-channel",
          name: "Same kanal", kind: "public", circle_id: null, direct_user_id: null, is_direct: false,
          description: "", role: "member", last_read_sequence: 0, latest_sequence: 0 }] }); break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "subscribe_channel": reply("subscription_started", { channel_id: channelId, history: [] }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channelId, summaries: [] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channelId, reactions: [] }); break;
        case "load_recent_messages": reply("messages_loaded", { channel_id: channelId, messages: [] }); break;
        case "send_message": {
          sends.push({ owner: identity, payload: command.payload });
          reply("message_accepted", { message: { id: `account-message-${sends.length}`, channel_id: channelId,
            parent_message_id: command.payload.parent_message_id ?? null, body: command.payload.body,
            sender_id: identity, sender_display_name: identity, sequence: sends.length,
            sent_at: "2026-10-26T09:00:00Z", edited_at: null, deleted_at: null } });
          break;
        }
      }
    });
  });
  await page.route(/\/api\/v1\/media-uploads\/config(?:\?|$)/, route => route.fulfill({ json: { enabled: false } }));
  await page.route(/\/channels\/[^/]+\/media(?:\?|$)/, async route => {
    started = true;
    uploads.push(route.request().url());
    if (holdUpload) await pending;
    try {
      await route.fulfill({ json: { media: { id: mediaId, channel_id: channelId,
        original_filename: "alice-private.png", content_type: "image/png" } } });
    } catch { /* Account fencing may abort the obsolete request. */ }
    completed = true;
  });
  await page.route(/\/api\/v1\/media\/[^/?]+(?:\?|$)/, route => route.fulfill({ contentType: "image/png", body: image }));
  await page.goto(`/?participant=account-navigation&channel=${channelId}`);
  const app = page.locator("#sproyt-react-preview");
  const composer = app.locator(".sp-channel-pane").getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(composer).toBeEnabled();
  await composer.fill("Alice sitt private utkast");
  await app.locator('.sp-channel-pane input[type="file"]').setInputFiles({ name: "alice-private.png", mimeType: "image/png", buffer: image });
  await expect.poll(() => started).toBe(true);
  return { app, composer, sends, errors, uploads, release, completed: () => completed,
    switchAccount: () => { identity = "bob"; emit("hello", { participant_id: identity }); } };
}

for (const holdUpload of [false, true]) test(`${holdUpload ? "late" : "completed"} Alice attachment cannot enter Bob's composer or send in the same channel`, async ({ page }) => {
  const fixture = await accountFixture(page, holdUpload);
  const attachments = fixture.app.getByRole("region", { name: "Valde vedlegg" });
  if (!holdUpload) {
    await expect(attachments.getByRole("button", { name: "Fjern alice-private.png", exact: true })).toBeEnabled();
    await expect.poll(fixture.completed).toBe(true);
  }
  fixture.switchAccount();
  await expect(fixture.composer).toBeEnabled();
  await expect(fixture.composer).toHaveValue("");
  await expect(attachments).toHaveCount(0);
  if (holdUpload) {
    fixture.release();
    await expect.poll(fixture.completed).toBe(true);
  }
  await fixture.composer.fill("Bob si eiga melding");
  await fixture.composer.press("Enter");
  await expect(fixture.app.locator(".sp-channel-pane [data-message-id]").getByText("Bob si eiga melding", { exact: true })).toBeVisible();
  await expect(fixture.composer).toHaveValue("");
  await expect(attachments).toHaveCount(0);
  await expect(fixture.app.getByRole("button", { name: "Send vedlegg", exact: true })).toHaveCount(0);
  expect(fixture.sends).toEqual([{ owner: "bob", payload: { channel_id: channelId, body: "Bob si eiga melding" } }]);
  expect(fixture.sends.some(send => send.payload.body.includes(mediaId) || send.payload.body.includes("[[media:"))).toBe(false);
  expect(fixture.uploads).toHaveLength(1);
  expect(fixture.errors).toEqual([]);
});
