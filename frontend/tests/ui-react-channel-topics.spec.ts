import { expect, test, type Locator, type Page } from "@playwright/test";
import { openChannelMenu } from "./support/channel-menu";

const owner = "channel-topics-viewer";
const channel = "00000000-0000-7000-8000-000000008811";
const date = "2026-01-01T00:00:00Z";
const message = (id: string, sequence: number, body: string, parent_message_id: string | null = null) => ({
  id, channel_id: channel, parent_message_id, sequence, body, sent_at: date,
  sender_id: "other", sender_display_name: "Nora"
});
const hit = (row: ReturnType<typeof message>, summary = row.body) => ({
  message_id: row.id, parent_message_id: row.parent_message_id,
  sequence: row.sequence, body: summary, sender_display_name: row.sender_display_name, created_at: date, message: row
});
const latest = message("latest", 10000, "Dette er den nyaste meldinga.");
const older = message("older", 8, "Tur frå den eldre historia.");
const neighbor = message("neighbor", 9, "Nabomelding frå same samtale.");
const replyText = "Fjelltur i tråden.";
const mediaId = "00000000-0000-7000-8000-000000008814";
const image = (name: string) => `[[media:${mediaId}|image/png|${name}]]`;
const reply = message("reply", 10, `${replyText}\n\n${image("tur.png")}`, older.id);
const sourceChannel = "00000000-0000-7000-8000-000000008812";
const sourceId = "00000000-0000-7000-8000-000000008813";
const source = { ...message(sourceId, 4, `${older.body}\n\n${image("lagra.png")}`), channel_id: sourceChannel };
const saved = { ...older, body: `[Lagra melding](/?channel=${sourceChannel}&message=${sourceId}&sequence=4)` };

async function setup(page: Page, canManage = true, personal = false) {
  let enabled = true;
  const writes: unknown[] = [];
  const cursors: (string | null)[] = [];
  if (personal) await page.route(`**/api/v1/me/personal-space/sources/${sourceChannel}/${sourceId}/4?**`, route =>
    route.fulfill({ json: source }));
  await page.route(new RegExp(`/api/v1/media/${mediaId}(?:[/?]|$)`), route =>
    new URL(route.request().url()).pathname.endsWith("/status") ? route.fulfill({ json: { state: "legacy" } })
      : route.fulfill({ contentType: "image/png",
        body: Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6aQAAAAASUVORK5CYII=", "base64") }));
  await page.route(new RegExp(`/api/v1/channels/${channel}/topics(?:[/?]|$)`), route => {
    const url = new URL(route.request().url());
    expect(route.request().headers()["x-sproyt-account"]).toBe(owner);
    if (url.pathname.endsWith("/matches")) {
      expect(url.searchParams.get("topic")).toBe("tur");
      const before = url.searchParams.get("before");
      cursors.push(before);
      return route.fulfill({ json: before ? { matches: [hit(personal ? saved : older, older.body)], next_before: null }
        : { matches: [hit(reply, replyText)], next_before: 10 } });
    }
    if (url.pathname.endsWith("/context")) {
      expect(url.searchParams.get("message_id")).toBe(older.id);
      expect(url.searchParams.get("sequence")).toBe("8");
      return route.fulfill({ json: { messages: [personal ? saved : older, neighbor] } });
    }
    if (route.request().method() === "PUT") {
      const request = route.request().postDataJSON();
      writes.push(request);
      enabled = request.enabled;
    }
    return route.fulfill({ json: { enabled, can_manage: canManage, topics: enabled ? [
      { key: "tur", label: "Tur", count: 2 }, { key: "mat", label: "Mat og oppskrifter", count: 2 }, { key: "hus", label: "Hus og hage", count: 2 }
    ] : [] } });
  });
  await page.routeWebSocket(/\/ws(?:\?|$)/, socket => socket.onMessage(data => {
    const command = JSON.parse(String(data));
    const replyTo = (type: string, payload: unknown = {}) => socket.send(JSON.stringify({
      protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload
    }));
    switch (command.type) {
      case "hello": replyTo("hello", { participant_id: owner }); break;
      case "ping": replyTo("pong"); break;
      case "list_users": replyTo("users_listed", { users: [] }); break;
      case "list_my_circles": replyTo("circles_listed", { circles: [] }); break;
      case "list_my_channels": replyTo("channels_listed", { channels: [{ id: channel, slug: "topics", name: personal ? "Området mitt" : "Turprat",
        kind: personal ? "private" : "public", is_personal: personal, circle_id: null, direct_user_id: null, description: "", role: canManage ? "owner" : "member",
        last_read_sequence: 10000, latest_sequence: 10000 }] }); break;
      case "list_channel_users": replyTo("channel_users_listed", { channel_id: channel, users: [] }); break;
      case "list_mentions": replyTo("mentions_listed", { mentions: [] }); break;
      case "list_tasks": replyTo("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": replyTo("subscription_started", { channel_id: channel, history: [latest] }); break;
      case "list_thread_summaries": replyTo("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
      case "list_channel_reactions": replyTo("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
      case "load_recent_messages": replyTo("messages_loaded", { channel_id: channel, messages: [] }); break;
      case "load_thread": replyTo("thread_loaded", { root_message_id: older.id, messages: [older, reply] }); break;
    }
  }));
  await page.goto(`/?participant=${owner}&channel=${channel}&ui=react`);
  const app = page.locator("#sproyt-react-preview");
  await expect(app.getByText(latest.body)).toBeVisible();
  return { app, cursors, writes };
}

async function capturePreview(page: Page, app: Locator, path: string | undefined) {
  if (!path) return;
  const installNotice = app.getByRole("button", { name: "Lukk påminning om Sprøyt som app" });
  if (await installNotice.isVisible()) await installNotice.click();
  await page.mouse.move(0, 0);
  await app.screenshot({ path, scale: "css" });
}

test("topic pills page server history and open old context with the composer draft intact", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => localStorage.setItem("sproyt.theme.v1", "dark"));
  const { app, cursors, writes } = await setup(page, true, true);
  const pills = app.getByRole("group", { name: "Emne i samtalen" });
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(pills.getByRole("button")).toHaveCount(4);
  expect(await app.evaluate(element => element.scrollWidth <= element.clientWidth)).toBeTruthy();
  await composer.fill("Utkast");
  await pills.getByRole("button", { name: "Tur", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(pills.getByRole("button", { name: "Tur", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(app.getByText(latest.body)).toBeHidden();
  const results = app.getByLabel("Meldingar om Tur");
  const directImage = results.getByRole("img", { name: "tur.png", exact: true });
  await expect(directImage).toBeVisible();
  await expect.poll(() => directImage.evaluate(node => (node as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
  await results.getByRole("button", { name: "Vis tur.png i full storleik" }).click();
  const viewer = app.getByRole("dialog", { name: "tur.png" });
  await expect(viewer).toBeVisible();
  await viewer.getByRole("button", { name: "Lukk bilete" }).click();
  await app.getByRole("button", { name: "Last eldre treff" }).click();
  const oldResult = results.locator('[data-message-id="older"]');
  const oldHit = results.getByRole("button", { name: "Opne i samtalen", exact: true });
  await oldResult.scrollIntoViewIfNeeded();
  await expect(oldResult.getByText(older.body)).toBeVisible();
  await expect(oldResult.getByRole("link", { name: /^Opne originalmeldinga/ })).toHaveAttribute("href", new RegExp(sourceId));
  const savedImage = oldResult.getByRole("img", { name: "lagra.png", exact: true });
  await savedImage.scrollIntoViewIfNeeded();
  await expect.poll(() => savedImage.evaluate(node => (node as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
  await expect(oldHit).toBeVisible();
  await capturePreview(page, app, process.env.SPROYT_TOPICS_SCREENSHOT);
  await oldHit.click();
  expect(cursors).toEqual([null, "10"]);
  const context = app.getByRole("region", { name: "Meldinga i samtalen" });
  await expect(context.getByText(neighbor.body)).toBeVisible();
  await expect(context.locator('[data-message-id="older"]')).toBeFocused();
  await expect(context.getByText(older.body)).toBeVisible();
  await expect(context.getByRole("link", { name: /^Opne originalmeldinga/ })).toHaveAttribute("href", new RegExp(sourceId));
  await context.getByRole("button", { name: "Tilbake til emnetreffa" }).click();
  await expect(oldHit).toBeFocused();
  await expect(pills.getByRole("button", { name: "Tur", exact: true })).toHaveAttribute("aria-pressed", "true");
  await pills.getByRole("button", { name: "Alle", exact: true }).click();
  await expect(app.getByText(latest.body)).toBeVisible();
  await expect(composer).toHaveValue("Utkast");
  await pills.getByRole("button", { name: "Tur", exact: true }).click();
  await results.getByRole("button", { name: "Opne i tråden", exact: true }).click();
  const thread = app.getByRole("region", { name: "Tråd", exact: true });
  await expect(thread.getByRole("button", { name: "Lukk tråden" })).toBeVisible();
  await expect(thread.getByText(replyText)).toBeVisible();
  await thread.getByRole("button", { name: "Lukk tråden" }).click();
  await app.getByRole("button", { name: /Privat chatlogg, Området mitt/ }).click();
  await app.getByRole("button", { name: "Chatlogginnstillingar" }).click();
  const setting = app.getByRole("switch", { name: "Vis automatiske emne" });
  await expect(setting).toBeChecked();
  await setting.click();
  await expect(setting).not.toBeChecked();
  expect(writes).toEqual([{ enabled: false }]);
});

test("channel owners can disable topics", async ({ page }) => {
  const { app, writes } = await setup(page);
  await expect(app.getByRole("group", { name: "Emne i samtalen" })).toBeVisible();
  await capturePreview(page, app, process.env.SPROYT_TOPICS_LIGHT_SCREENSHOT);
  await (await openChannelMenu(app)).getByRole("button", { name: "Kanaldetaljar", exact: true }).click();
  const control = app.getByRole("switch", { name: "Vis automatiske emne" });
  await expect(control).toBeChecked();
  await control.click();
  await expect(control).not.toBeChecked();
  expect(writes).toEqual([{ enabled: false }]);
  await page.goBack();
  await expect(app.getByText(latest.body)).toBeVisible();
  await expect(app.getByRole("group", { name: "Emne i samtalen" })).toHaveCount(0);
});

test("members can filter with no channel topic setting control", async ({ page }) => {
  const { app, writes } = await setup(page, false);
  await expect(app.getByRole("button", { name: "Tur", exact: true })).toBeVisible();
  const summary = page.waitForResponse(response => response.url().includes("/topics?") && response.request().method() === "GET");
  await (await openChannelMenu(app)).getByRole("button", { name: "Kanaldetaljar", exact: true }).click();
  await summary;
  await expect(app.getByRole("switch", { name: "Vis automatiske emne" })).toHaveCount(0);
  expect(writes).toEqual([]);
});
