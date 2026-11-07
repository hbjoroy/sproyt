import { readFile } from "node:fs/promises";
import { expect, test, type Page } from "@playwright/test";

const owner = "events-viewer";
const channel = "00000000-0000-7000-8000-000000009911";
const eventId = "00000000-0000-7000-8000-000000009912";
const mediaId = "00000000-0000-7000-8000-000000009913";
const date = "2026-01-01T00:00:00Z";
const row = (id: string, sequence: number, body: string) => ({ id, channel_id: channel, parent_message_id: null, sequence, body, sent_at: date, sender_id: owner, sender_display_name: "Nora" });
async function setup(page: Page, deep = false) {
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url), "utf8");
  await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  let event = { id: eventId, channel_id: channel, creator_id: channel, title: "Konsert med eit langt og lesbart namn", code: "konsert", aliases: ["gammal"],
    starts_at: "2027-01-01T18:00:00Z", timezone: "Europe/Oslo", ends_at: null, location: "Grendahuset", description: "**Ta med vener**", cancelled: false,
    revision: 2, updated_at: date, cover_media_id: mediaId as string | null, my_rsvp: "following", can_manage: true, needs_attention: true, images: [{ media_id: mediaId, contributor_id: owner }], images_next_after: null };
  let revoked = false;
  const changes: string[] = [];
  await page.route(new RegExp(`/api/v1/channels/${channel}/events(?:[/?]|$)`), route => {
    if (revoked) return route.fulfill({ status: 403, json: { error: "Tilgangen er fjerna" } });
    const path = new URL(route.request().url()).pathname;
    expect(route.request().headers()["x-sproyt-account"]).toBe(owner);
    if (path.endsWith("/responses")) return route.fulfill({ json: { responses: [{ user_id: owner, display_name: "Nora", status: event.my_rsvp }], next_after: null } });
    if (path.endsWith("/references")) return route.fulfill({ json: { events: [event] } });
    if (path.endsWith("/messages")) return route.fulfill({ json: { messages: [row("match", 8, `**Rik melding** #arr:konsert\n\n[[media:${mediaId}|image/png|konsert.png]]`)], next_before: null } });
    if (route.request().method() !== "GET") {
      changes.push(path.split("/").at(-1)!);
      if (path.endsWith("/rsvp")) event = { ...event, my_rsvp: route.request().postDataJSON().status };
      else if (path.endsWith("/acknowledge")) event = { ...event, needs_attention: false };
      else if (path.endsWith("/cancel")) event = { ...event, cancelled: true };
      else if (route.request().method() === "PUT") event = { ...event, ...route.request().postDataJSON() };
      else if (path.endsWith("/images")) event = { ...event, images: [{ media_id: mediaId, contributor_id: owner }] };
      return route.fulfill({ json: event });
    }
    return route.fulfill({ json: path.endsWith("/events") ? { events: [event], next_after: null } : event });
  });
  await page.route(new RegExp(`/api/v1/channels/${channel}/topics(?:[/?]|$)`), route => route.fulfill({ json: { enabled: false, can_manage: true, topics: [] } }));
  await page.route(new RegExp(`/api/v1/media/${mediaId}(?:[/?]|$)`), route => new URL(route.request().url()).pathname.endsWith("/status")
    ? route.fulfill({ json: { state: "legacy" } }) : route.fulfill({ contentType: "image/png", body: Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a6aQAAAAASUVORK5CYII=", "base64") }));
  await page.routeWebSocket(/\/ws(?:\?|$)/, socket => socket.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: owner }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, slug: "events", name: "Arrangementskanalen", kind: "public", circle_id: null, direct_user_id: null, description: "", role: "owner", last_read_sequence: 10, latest_sequence: 10 }] }); break;
      case "list_channel_users": reply("channel_users_listed", { channel_id: channel, users: [] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: [row("latest", 10, "Hei #arr:gammal")] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
    }
  }));
  await page.goto(`/?participant=${owner}&channel=${channel}&ui=react${deep ? `&event=${eventId}` : ""}`);
  const app = page.locator("#sproyt-react-preview");
  await expect(app.getByRole("tab", { name: /Arrangement/ })).toBeVisible();
  return { app, changes, revoke: () => { revoked = true; } };
}

for (const [width, theme] of [[320, "light"], [390, "dark"], [1440, "light"]] as const) {
  test(`event tabs preserve drafts, rich references and independent filters at ${width} ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.addInitScript(mode => localStorage.setItem("sproyt.theme.v1", mode), theme);
    const { app } = await setup(page);
    const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
    await composer.fill("Utkast #arr:ko");
    await expect(app.getByRole("listbox", { name: "Arrangementsforslag" })).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(composer).toHaveValue("Utkast #arr:konsert ");
    await app.getByRole("link", { name: "#arr:gammal", exact: true }).click();
    await expect(app.getByRole("heading", { level: 2, name: /Konsert med/ })).toBeVisible();
    await expect(app.getByText("Ta med vener", { exact: true })).toBeVisible();
    await expect(app.getByText(/^Arrangementet er endra sidan du svarte\./u)).toBeVisible();
    const screenshot = async (part: string) => {
      if (process.env.SPROYT_EVENT_SCREENSHOTS) await app.screenshot({ path: `${process.env.SPROYT_EVENT_SCREENSHOTS}/events-${part}-${width}-${theme}.png` });
    };
    await screenshot("detail");
    await app.getByRole("heading", { name: "Biletbibliotek", exact: true }).scrollIntoViewIfNeeded();
    await screenshot("gallery");
    await app.getByRole("button", { name: "Rediger", exact: true }).click();
    await expect(app.getByRole("textbox", { name: "Tittel", exact: true })).toBeVisible();
    await screenshot("editor");
    await app.getByRole("button", { name: "Avbryt", exact: true }).click();
    await app.getByRole("button", { name: "Markér som lesen" }).click();
    await app.getByRole("radio", { name: "Deltek", exact: true }).click();
    await expect(app.getByRole("radio", { name: "Deltek", exact: true })).toBeChecked();
    await app.getByRole("button", { name: "Alle arrangement", exact: false }).click();
    await screenshot("list");
    await app.getByRole("tab", { name: "Meldingar", exact: true }).click();
    await expect(composer).toHaveValue("Utkast #arr:konsert ");
    const pills = app.getByRole("group", { name: "Arrangement i meldingane" });
    await pills.getByRole("button", { name: "#arr:konsert", exact: true }).click();
    const results = app.getByLabel("Meldingar om arrangementet");
    await expect(results.getByText("Rik melding", { exact: true })).toBeVisible();
    await expect(results.getByRole("img", { name: "konsert.png", exact: true })).toBeVisible();
    expect(await app.evaluate(element => element.scrollWidth <= element.clientWidth)).toBeTruthy();
    await pills.getByRole("button", { name: "Alle meldingar" }).click();
    await expect(composer).toHaveValue("Utkast #arr:konsert ");
    if (process.env.SPROYT_EVENT_SCREENSHOTS) await app.screenshot({ path: `${process.env.SPROYT_EVENT_SCREENSHOTS}/events-${width}-${theme}.png` });
  });
}
test("event deep link reload, editing failure and membership revocation clear private detail", async ({ page }) => {
  const { app, revoke } = await setup(page, true);
  await expect(app.getByRole("heading", { level: 2, name: /Konsert med/ })).toBeVisible();
  await page.reload();
  await expect(app.getByRole("heading", { level: 2, name: /Konsert med/ })).toBeVisible();
  await app.getByRole("button", { name: "Rediger", exact: true }).click();
  const title = app.getByRole("textbox", { name: "Tittel", exact: true });
  await title.fill("Ny konsert");
  await app.getByRole("button", { name: "Lagre arrangement" }).click();
  await expect(app.getByRole("heading", { level: 2, name: "Ny konsert" })).toBeVisible();
  revoke();
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  await expect(app.getByText(/Arrangementet kunne ikkje lastast/)).toBeVisible();
  await expect(app.getByRole("heading", { level: 2, name: "Ny konsert" })).toHaveCount(0);
});
