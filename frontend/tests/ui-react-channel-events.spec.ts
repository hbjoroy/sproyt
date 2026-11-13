import { readFile } from "node:fs/promises";
import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";
import { eventImage } from "./support/event-image";

const owner = "events-viewer";
const channel = "00000000-0000-7000-8000-000000009911";
const eventId = "00000000-0000-7000-8000-000000009912";
const mediaId = "00000000-0000-7000-8000-000000009913";
const date = "2026-01-01T00:00:00Z";
const row = (id: string, sequence: number, body: string) => ({ id, channel_id: channel, parent_message_id: null, sequence, body, sent_at: date, sender_id: owner, sender_display_name: "Nora" });
async function setup(page: Page, deep = false, offPage = false, initialEnabled = true, topics = false) {
  let enabled = initialEnabled;
  let socket: WebSocketRoute | undefined;
  const image = await eventImage(page);
  await page.route("**/api/v1/me/favorite-circles?**", route => route.fulfill({ json: [] }));
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url), "utf8");
  if (process.env.SPROYT_E2E_EMBEDDED_CLIENT !== "1") await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  let event = { id: eventId, channel_id: channel, creator_id: channel, events_enabled: enabled, title: "Konsert med eit langt og lesbart namn", code: "konsert", aliases: ["gammal"],
    starts_at: "2027-01-01T18:00:00Z", timezone: "Europe/Oslo", ends_at: null, location: "Grendahuset", description: "**Ta med vener**", cancelled: false,
    revision: 2, updated_at: date, cover_media_id: mediaId as string | null, my_rsvp: "following", can_manage: true, needs_attention: true, images: [{ media_id: mediaId, contributor_id: owner }], images_next_after: null };
  let summaryOverride: Record<string, unknown> | undefined;
  const summary = () => summaryOverride ?? { as_of: "2026-11-13T12:00:00Z", upcoming_count: event.cancelled ? 0 : offPage ? 101 : 1, ongoing_count: 0, next_event: event.cancelled ? null : event };
  let revoked = false;
  const changes: string[] = [];
  const prefixes: string[] = [];
  const collectionReads: (string | null)[] = [];
  await page.route(new RegExp(`/api/v1/channels/${channel}/events(?:[/?]|$)`), route => {
    if (revoked) return route.fulfill({ status: 403, json: { error: "Tilgangen er fjerna" } });
    const path = new URL(route.request().url()).pathname;
    const prefix = new URL(route.request().url()).searchParams.get("prefix");
    if (prefix !== null) { prefixes.push(prefix); return route.fulfill({ json: { events: [event], next_after: null } }); }
    const after = new URL(route.request().url()).searchParams.get("after");
    if (path.endsWith("/events")) collectionReads.push(after);
    if (offPage && after) return route.fulfill({ json: { events: [event], next_after: null, attention_count: 7, summary: summary() } });
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
    return route.fulfill({ json: path.endsWith("/events") ? offPage ? { events: Array.from({ length: 100 }, (_, index) => ({ ...event, id: `00000000-0000-7000-8000-${String(index + 20000).padStart(12, "0")}`, title: `Andre arrangement ${index}`, code: `event-${index}`, aliases: [], needs_attention: false })), next_after: "next-page", attention_count: 7, summary: summary() } : { events: [event], next_after: null, attention_count: event.needs_attention ? 1 : 0, summary: summary() } : event });
  });
  await page.route(new RegExp(`/api/v1/channels/${channel}/topics(?:[/?]|$)`), route => route.fulfill({ json:
    new URL(route.request().url()).pathname.endsWith("/matches")
      ? { matches: [{ message_id: "topic-match", parent_message_id: null, sequence: 7, body: "Turprat", sender_display_name: "Nora", created_at: date, message: row("topic-match", 7, "**Turprat**") }], next_before: null }
      : { enabled: topics, can_manage: true, topics: topics ? [{ key: "tur", label: "Tur", count: 2 }, { key: "mat", label: "Mat og oppskrifter", count: 3 }, { key: "hus", label: "Hus og hage", count: 2 }] : [] }
  }));
  await page.route(new RegExp(`/api/v1/media/${mediaId}(?:[/?]|$)`), route => new URL(route.request().url()).pathname.endsWith("/status")
    ? route.fulfill({ json: { state: "legacy" } }) : route.fulfill({ contentType: "image/png", body: image }));
  await page.routeWebSocket(/\/ws(?:\?|$)/, connection => { socket = connection; connection.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => connection.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: owner }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, slug: "events", events_enabled: enabled, name: "Arrangementskanalen", kind: "public", circle_id: null, direct_user_id: null, description: "", role: "owner", last_read_sequence: 10, latest_sequence: 10 }] }); break;
      case "list_channel_users": reply("channel_users_listed", { channel_id: channel, users: [] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: [row("latest", 10, offPage ? "Hei kanalen" : "Hei #arr:gammal")] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
    }
  }); });
  await page.goto(`/?participant=${owner}&channel=${channel}&ui=react${deep ? `&event=${eventId}` : ""}`);
  const app = page.locator("#sproyt-react-preview");
  if (enabled) await expect(app.getByRole("tab", { name: /Arrangement/ })).toBeVisible();
  else if (deep) await expect(app.getByRole("heading", { name: /Konsert med/, level: 2 })).toBeVisible();
  else await expect(app.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  return { app, changes, prefixes, collectionReads, getEvent: () => event, setSummary: (value: Record<string, unknown>) => { summaryOverride = value; }, revoke: () => { revoked = true; }, setEnabled(value: boolean) {
    enabled = value; event = { ...event, events_enabled: value };
    socket!.send(JSON.stringify({ protocol: "sproyt.chat.v1", type: "circles_changed" }));
  } };
}

for (const [width, theme] of [[320, "light"], [320, "dark"], [390, "light"], [390, "dark"], [1440, "light"], [1440, "dark"]] as const) {
  test(`event tabs preserve drafts, rich references and independent filters at ${width} ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.addInitScript(mode => localStorage.setItem("sproyt.theme.v1", mode), theme);
    const { app } = await setup(page);
    const coarse = await page.evaluate(() => matchMedia("(pointer: coarse)").matches);
    const tabs = app.getByRole("tablist", { name: "Innhald i kanalen" });
    for (const tab of await tabs.getByRole("tab").all()) expect((await tab.boundingBox())!.height).toBe(coarse ? 44 : 36);
    expect((await app.locator(".sp-channel-navigation").boundingBox())!.height).toBe(coarse ? 45 : 37);
    if (process.env.SPROYT_EVENT_SCREENSHOTS) await app.screenshot({ path: `${process.env.SPROYT_EVENT_SCREENSHOTS}/events-nav-${width}-${theme}.png` });
    const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
    await composer.fill("Utkast #arr:ko");
    await expect(app.getByRole("listbox", { name: "Arrangementsforslag" })).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(composer).toHaveValue("Utkast #arr:konsert ");
    await app.getByRole("link", { name: "Konsert med eit langt og lesbart namn", exact: true }).click();
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
    await app.getByRole("tab", { name: /Arrangement/ }).focus();
    await page.keyboard.press("ArrowLeft");
    await expect(app.getByRole("tab", { name: "Meldingar", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(app.getByRole("tab", { name: "Meldingar", exact: true })).toHaveCSS("outline-width", "2px");
    await expect(composer).toHaveValue("Utkast #arr:konsert ");
    const pills = app.getByRole("group", { name: "Filtrer meldingar" });
    await pills.getByRole("button", { name: /^Konsert med eit langt og lesbart namn, / }).click();
    const results = app.getByLabel("Meldingar om arrangementet");
    await expect(results.getByText("Rik melding", { exact: true })).toBeVisible();
    await expect(results.getByRole("img", { name: "konsert.png", exact: true })).toBeVisible();
    expect(await app.evaluate(element => element.scrollWidth <= element.clientWidth)).toBeTruthy();
    await pills.getByRole("button", { name: "Alle", exact: true }).click();
    await expect(composer).toHaveValue("Utkast #arr:konsert ");
    if (process.env.SPROYT_EVENT_SCREENSHOTS) await app.screenshot({ path: `${process.env.SPROYT_EVENT_SCREENSHOTS}/events-${width}-${theme}.png` });
  });
}
for (const from of ["detail", "filter"] as const) test(`availability invalidation closes ${from} without remounting chat and keeps the off archive readable`, async ({ page }) => {
  const server = await setup(page); const { app } = server;
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await composer.fill("Utkastet skal bevarast"); const field = await composer.elementHandle();
  const timeline = app.locator(".sp-timeline[aria-label='Meldingar']"); const viewport = await timeline.elementHandle();
  if (from === "filter") await app.getByRole("group", { name: "Filtrer meldingar" }).getByRole("button", { name: /^Konsert med eit langt og lesbart namn, / }).click();
  else await app.getByRole("link", { name: "Konsert med eit langt og lesbart namn", exact: true }).click();
  server.setEnabled(false);
  await expect(app.getByRole("tablist", { name: "Innhald i kanalen" })).toHaveCount(0);
  await expect(composer).toBeVisible(); await expect(composer).toHaveValue("Utkastet skal bevarast");
  await composer.fill("Utkastet skal bevarast #arr:ko");
  await expect(app.getByRole("listbox", { name: "Arrangementsforslag" })).toHaveCount(0);
  await composer.fill("Utkastet skal bevarast");
  expect(await field!.evaluate(element => element.isConnected)).toBe(true);
  expect(await viewport!.evaluate(element => element.isConnected)).toBe(true);
  await app.getByRole("button", { name: "Lukk", exact: true }).click();
  await app.getByRole("link", { name: "Konsert med eit langt og lesbart namn", exact: true }).click();
  await expect(app.getByRole("heading", { level: 2, name: /Konsert med/ })).toBeVisible();
  await expect(app.getByText(/^Arrangement er slått av i denne kanalen\./u)).toBeVisible();
  await expect(app.getByText("Lastar arrangement …", { exact: true })).toHaveCount(0);
  await expect(app.getByRole("button", { name: "Rediger", exact: true })).toHaveCount(0);
  await expect(app.getByRole("radio", { name: "Deltek", exact: true })).toBeDisabled();
  server.setEnabled(true);
  await expect(app.getByRole("button", { name: "Rediger", exact: true })).toBeVisible();
  await app.getByRole("tab", { name: "Meldingar", exact: true }).click();
  await expect(composer).toHaveValue("Utkastet skal bevarast");
});
test("an off deep link opens a read-only archive without an irrelevant navigation row", async ({ page }) => {
  const { app } = await setup(page, true, false, false);
  await expect(app.getByRole("heading", { name: /Konsert med/, level: 2 })).toBeVisible();
  await expect(app.getByRole("tablist", { name: "Innhald i kanalen" })).toHaveCount(0);
  await expect(app.getByText("Lastar arrangement …", { exact: true })).toHaveCount(0);
  await expect(app.getByRole("radio", { name: "Deltek", exact: true })).toBeDisabled();
  await expect(app.getByRole("button", { name: "Legg til bilete", exact: true })).toHaveCount(0);
  await app.getByRole("button", { name: "Tilbake til meldingane", exact: false }).click();
  await expect(app.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeVisible();
});
for (const [width, theme] of [[320, "light"], [320, "dark"], [390, "light"], [390, "dark"], [1440, "light"], [1440, "dark"]] as const) {
  test(`disabled events leave ordinary chat full height at ${width} ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width, height: width === 320 ? 360 : 900 });
    await page.addInitScript(mode => localStorage.setItem("sproyt.theme.v1", mode), theme);
    const { app, collectionReads } = await setup(page, false, false, false);
    await expect(app.getByRole("tablist", { name: "Innhald i kanalen" })).toHaveCount(0);
    await expect(app.getByRole("group", { name: "Filtrer meldingar" })).toHaveCount(0);
    expect(collectionReads).toEqual([]);
    if (process.env.SPROYT_EVENT_SCREENSHOTS) await app.screenshot({ path: `${process.env.SPROYT_EVENT_SCREENSHOTS}/events-off-${width}-${theme}.png` });
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
test("prefix autocomplete finds an event beyond the first hundred without loading the whole channel", async ({ page }) => {
  const { app, prefixes } = await setup(page, false, true);
  await expect(app.getByText("Hei kanalen", { exact: true })).toBeVisible();
  await expect(app.getByRole("tab", { name: "Arrangement, 101 komande arrangement, 7 arrangement endra", exact: true })).toBeVisible();
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await composer.fill("#arr:ko");
  await expect(app.getByRole("listbox", { name: "Arrangementsforslag" }).getByRole("option", { name: /Konsert med/ })).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(composer).toHaveValue("#arr:konsert ");
  expect(prefixes).toEqual(["ko"]);
});
test("background refresh preserves explicitly loaded event pages and reading position", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => {
    localStorage.setItem("sproyt.device-notice.install.dismissed", "1");
    localStorage.setItem("sproyt.device-notice.notifications.dismissed", "1");
  });
  await page.clock.install();
  const { app, collectionReads } = await setup(page, false, true);
  await app.getByRole("tab", { name: /Arrangement/ }).click();
  await app.getByRole("button", { name: "Last fleire arrangement", exact: true }).click();
  const later = app.getByRole("link", { name: "Konsert med eit langt og lesbart namn", exact: true });
  await later.scrollIntoViewIfNeeded();
  await expect(later).toBeVisible();
  const panel = app.locator(".sp-events-panel");
  const before = await panel.evaluate(element => element.scrollTop);
  expect(before).toBeGreaterThan(0);
  await page.clock.fastForward(31_000);
  await expect(later).toBeVisible();
  expect(await panel.evaluate(element => element.scrollTop)).toBe(before);
  expect(collectionReads).toEqual([null, "next-page", null]);
  await app.getByRole("button", { name: "Oppdater arrangement", exact: true }).click();
  await expect.poll(() => collectionReads.length).toBe(4);
});
for (const [width, height, theme] of [[320, 360, "light"], [667, 375, "dark"]] as const) test(`quiet event navigation remains usable on a short ${width}x${height} viewport`, async ({ page }) => {
  await page.setViewportSize({ width, height });
  await page.addInitScript(mode => localStorage.setItem("sproyt.theme.v1", mode), theme);
  const { app } = await setup(page);
  const tabs = app.getByRole("tablist", { name: "Innhald i kanalen" });
  const coarse = await page.evaluate(() => matchMedia("(pointer:coarse)").matches);
  expect((await app.locator(".sp-channel-navigation").boundingBox())!.height).toBe(coarse ? 45 : 37);
  const send = app.getByRole("button", { name: "Send ↑", exact: true });
  await expect(send).toBeInViewport({ ratio: 1 });
  expect(await app.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  if (process.env.SPROYT_EVENT_SCREENSHOTS) await app.screenshot({ path: `${process.env.SPROYT_EVENT_SCREENSHOTS}/events-short-${width}-${theme}.png` });
});

for (const [width, theme] of [[390, "light"], [390, "dark"], [1440, "light"], [1440, "dark"]] as const) {
  test(`event and topic filters share one compact row and reset at ${width} ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.addInitScript(mode => {
      localStorage.setItem("sproyt.theme.v1", mode);
      localStorage.setItem("sproyt.device-notice.install.dismissed", "1");
      localStorage.setItem("sproyt.device-notice.notifications.dismissed", "1");
    }, theme);
    const { app } = await setup(page, false, false, true, true);
    const filters = app.getByRole("group", { name: "Filtrer meldingar" });
    await expect(filters).toHaveCount(1);
    const all = filters.getByRole("button", { name: "Alle", exact: true });
    const event = filters.getByRole("button", { name: /^Konsert med eit langt og lesbart namn, / });
    await expect(event).toContainText("Konsert med eit langt og lesbart namn");
    await expect(event).toHaveAttribute("title", /#arr:konsert/u);
    expect((await event.boundingBox())!.width).toBeLessThanOrEqual(180);
    const topic = filters.getByRole("button", { name: "Tur", exact: true });
    await expect(topic).toBeVisible();
    const coarse = await page.evaluate(() => matchMedia("(pointer: coarse)").matches);
    const boxes = await filters.getByRole("button").evaluateAll(buttons => buttons.map(button => ({ y: button.getBoundingClientRect().y, height: button.getBoundingClientRect().height })));
    expect(new Set(boxes.map(box => box.y)).size).toBe(1);
    expect(boxes.every(box => box.height === (coarse ? 44 : 32))).toBe(true);
    expect((await filters.boundingBox())!.height).toBeLessThan(coarse ? 60 : 48);
    expect(await app.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
    const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
    await composer.fill("Utkastet mitt");
    const field = await composer.elementHandle();
    if (process.env.SPROYT_EVENT_SCREENSHOTS) await app.screenshot({ path: `${process.env.SPROYT_EVENT_SCREENSHOTS}/unified-filters-${width}-${theme}.png` });
    await topic.focus();
    await page.keyboard.press("Enter");
    await expect(app.getByLabel("Meldingar om Tur").getByText("Turprat", { exact: true })).toBeVisible();
    await expect(topic).toHaveAttribute("aria-pressed", "true");
    await event.click();
    await expect(topic).toHaveAttribute("aria-pressed", "false");
    await expect(app.getByLabel("Meldingar om arrangementet").getByText("Rik melding", { exact: true })).toBeVisible();
    await expect(app.getByLabel("Meldingar om arrangementet").getByRole("img", { name: "konsert.png", exact: true })).toBeVisible();
    await topic.click();
    await expect(event).toHaveAttribute("aria-pressed", "false");
    await expect(app.getByLabel("Meldingar om Tur").getByText("Turprat", { exact: true })).toBeVisible();
    await all.click();
    await expect(all).toHaveAttribute("aria-pressed", "true");
    await expect(app.getByRole("link", { name: "Konsert med eit langt og lesbart namn", exact: true })).toBeVisible();
    await expect(composer).toHaveValue("Utkastet mitt");
    expect(await field!.evaluate(element => element.isConnected)).toBe(true);
    await event.click();
    await all.click();
    await expect(all).toHaveAttribute("aria-pressed", "true");
    await expect(event).toHaveAttribute("aria-pressed", "false");
  });
}


test("more event filters expand in the same row and retain keyboard focus", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const { app } = await setup(page, false, true);
  const filters = app.getByRole("group", { name: "Filtrer meldingar" });
  const more = filters.getByRole("button", { name: "Fleire arrangement (97)", exact: true });
  await more.focus();
  await page.keyboard.press("Enter");
  const less = filters.getByRole("button", { name: "Færre arrangement", exact: true });
  await expect(less).toBeFocused();
  await expect(less).toHaveAttribute("aria-expanded", "true");
  await expect(filters.getByRole("button")).toHaveCount(103);
  expect(await filters.getByRole("button").evaluateAll(buttons => new Set(buttons.map(button => button.getBoundingClientRect().y)).size)).toBe(1);
  expect(await app.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await page.keyboard.press("Enter");
  await expect(more).toBeFocused();
  await expect(more).toHaveAttribute("aria-expanded", "false");
  await expect(filters.getByRole("button")).toHaveCount(6);
});


test("a second same-title event gets its own identity, tab opens list, and cancellation returns to its origin", async ({ page }) => {
  const server = await setup(page, true); const { app } = server;
  const first = server.getEvent();
  const secondId = "00000000-0000-7000-8000-000000009914";
  let second: typeof first | undefined;
  const created: Record<string, unknown>[] = [];
  await page.route(new RegExp(`/api/v1/channels/${channel}/events(?:[/?]|$)`), route => {
    const request = route.request(); const path = new URL(request.url()).pathname;
    if (path.endsWith("/events") && request.method() === "POST") {
      const input = request.postDataJSON(); created.push(input);
      expect(request.headers()["idempotency-key"]).toMatch(/^[0-9a-f-]{36}$/u);
      second = { ...first, ...input, id: secondId, code: input.code || "e-unique", images: [], cover_media_id: null, needs_attention: false };
      return route.fulfill({ json: second });
    }
    if (path.endsWith("/events") && request.method() === "GET") return route.fulfill({ json: { events: [first, ...(second ? [second] : [])], next_after: null, attention_count: 1,
      summary: { as_of: "2026-11-13T12:00:00Z", upcoming_count: second ? 2 : 1, ongoing_count: 0, next_event: first } } });
    if (second && path.endsWith(`/${secondId}`)) {
      if (request.method() === "PUT") second = { ...second, ...request.postDataJSON() };
      return route.fulfill({ json: second });
    }
    return route.fallback();
  });
  await app.getByRole("button", { name: "Nytt arrangement", exact: true }).click();
  await expect(app.getByRole("textbox", { name: "Tittel", exact: true })).toHaveValue("");
  await app.getByRole("button", { name: "Avbryt", exact: true }).click();
  await expect(app.getByRole("heading", { name: first.title, level: 2 })).toBeVisible();
  await app.getByRole("tab", { name: "Meldingar", exact: true }).click();
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await composer.fill("Utkastet mitt blir verande");
  await app.getByRole("tab", { name: /Arrangement/ }).click();
  await expect(app.getByRole("heading", { name: "Arrangement", level: 2, exact: true })).toBeVisible();
  await app.getByRole("button", { name: "Nytt arrangement", exact: true }).click();
  await app.getByRole("button", { name: "Avbryt", exact: true }).click();
  await expect(app.getByRole("heading", { name: "Arrangement", level: 2, exact: true })).toBeVisible();
  await expect(app.getByRole("button", { name: "Nytt arrangement", exact: true })).toBeFocused();
  await app.getByRole("button", { name: "Nytt arrangement", exact: true }).click();
  await app.getByRole("textbox", { name: "Tittel", exact: true }).fill(first.title);
  await app.getByLabel("Start", { exact: true }).fill("2027-02-01T19:00");
  await app.getByRole("button", { name: "Lagre arrangement", exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`event=${secondId}`));
  expect(created).toHaveLength(1); expect(created[0]!.code).toBe("");
  await expect(app.locator(".sp-event-detail code")).toHaveText("#arr:e-unique");
  const sameTitlePills = app.locator(".sp-event-pill");
  await expect(sameTitlePills).toHaveCount(2);
  const labels = await sameTitlePills.evaluateAll(buttons => buttons.map(button => button.getAttribute("aria-label")));
  expect(new Set(labels).size).toBe(2); // Equal titles retain distinct date/time identities.
  expect(await sameTitlePills.allTextContents()).toEqual([first.title, first.title]);
  await app.getByRole("button", { name: "Rediger", exact: true }).click();
  await app.getByRole("textbox", { name: "Tittel", exact: true }).fill("Endra berre nummer to");
  await app.locator(".sp-event-form").getByText("Fleire val", { exact: true }).click();
  await expect(app.getByRole("textbox", { name: "Kode i kanalen", exact: true })).toHaveValue("e-unique");
  await app.getByRole("button", { name: "Lagre arrangement", exact: true }).click();
  await expect(app.getByRole("heading", { name: "Endra berre nummer to", level: 2 })).toBeVisible();
  await app.getByRole("tab", { name: /Arrangement/ }).click();
  await expect(app.getByRole("link", { name: first.title, exact: true })).toBeVisible();
  await expect(app.getByRole("link", { name: "Endra berre nummer to", exact: true })).toBeVisible();
  await app.getByRole("tab", { name: "Meldingar", exact: true }).click();
  await expect(composer).toHaveValue("Utkastet mitt blir verande");
});

for (const width of [320, 390, 1440]) test(`whole-channel summary stays in the existing navigation row at ${width}px`, async ({ page }) => {
  await page.setViewportSize({ width, height: 900 });
  const server = await setup(page, false, true); const { app } = server;
  const navigation = app.locator(".sp-channel-navigation");
  const before = (await navigation.boundingBox())!.height;
  await expect(app.getByRole("tab", { name: /101 komande arrangement/ })).toBeVisible();
  expect(server.collectionReads).toEqual([null]);
  server.setSummary({ as_of: "2026-11-13T12:00:00Z", upcoming_count: 2, ongoing_count: 1,
    next_event: { ...server.getEvent(), title: "Samlinga som pågår", starts_at: "2026-11-13T11:00:00Z", ends_at: "2026-11-13T13:00:00Z" } });
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  await expect(app.getByRole("tab", { name: /2 komande arrangement, 1 pågåande arrangement/ })).toBeVisible();
  const next = navigation.getByRole("button", { name: /Pågår.*Samlinga som pågår/ });
  if (width === 1440) {
    await expect(next).toBeVisible();
    expect(await next.evaluate(element => Boolean(element.closest('[role="tablist"]')))).toBe(false);
    await next.click();
    await expect(page).toHaveURL(new RegExp(`event=${eventId}`));
  } else {
    await expect(next).toBeHidden();
    await expect(navigation.locator(".sp-events-ongoing")).toHaveText(" · Pågår");
    await expect(navigation.locator(".sp-events-ongoing")).toBeVisible();
  }
  expect((await navigation.boundingBox())!.height).toBe(before);
  expect(await app.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  server.setSummary({ as_of: "2026-11-13T14:00:00Z", upcoming_count: 0, ongoing_count: 0, next_event: null });
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  await expect(app.getByRole("tab", { name: /0 komande arrangement/ })).toBeVisible();
  await expect(navigation.locator(".sp-events-count")).toHaveCount(0);
  await expect(navigation.locator(".sp-events-next")).toHaveCount(0);
});

test("a summary load failure keeps its last confirmed count with an explicit error", async ({ page }) => {
  const { app } = await setup(page, false, true);
  await expect(app.getByRole("tab", { name: /101 komande arrangement/ })).toBeVisible();
  await page.route(new RegExp(`/api/v1/channels/${channel}/events(?:[?]|$)`), route => route.fulfill({ status: 503, json: { error: "Mellombels feil" } }));
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  await expect(app.getByRole("tab", { name: /101 komande arrangement.*kunne ikkje oppdaterast/ })).toBeVisible();
  await app.getByRole("tab", { name: /Arrangement/ }).click();
  await expect(app.getByRole("button", { name: "Prøv igjen", exact: true })).toBeVisible();
});


test("uncertain creation retries keep the same payload and key without creating an extra event", async ({ page }) => {
  const { app, getEvent } = await setup(page);
  let attempts = 0; const keys: string[] = []; const inputs: unknown[] = [];
  const createdId = "00000000-0000-7000-8000-000000009915";
  let saved: ReturnType<typeof getEvent> | undefined;
  await page.route(new RegExp(`/api/v1/channels/${channel}/events/${createdId}(?:[?]|$)`), route => route.fulfill({ json: saved }));
  await page.route(new RegExp(`/api/v1/channels/${channel}/events(?:[?]|$)`), route => {
    if (route.request().method() !== "POST") return route.fallback();
    attempts++; keys.push(route.request().headers()["idempotency-key"]!); inputs.push(route.request().postDataJSON());
    if (attempts === 1) return route.abort("connectionreset");
    saved = { ...getEvent(), ...route.request().postDataJSON(), id: createdId, code: "e-retry" };
    return route.fulfill({ json: saved });
  });
  await app.getByRole("tab", { name: /Arrangement/ }).click();
  await app.getByRole("button", { name: "Nytt arrangement", exact: true }).click();
  const title = app.getByRole("textbox", { name: "Tittel", exact: true });
  await title.fill("Eitt arrangement");
  await app.getByLabel("Start", { exact: true }).fill("2027-02-01T19:00");
  await app.getByRole("button", { name: "Lagre arrangement", exact: true }).click();
  await expect(app.getByText(/Kvitteringa er uviss/)).toBeVisible();
  await expect(title).toHaveValue("Eitt arrangement"); await expect(title).toBeDisabled();
  await app.getByRole("button", { name: "Lagre arrangement", exact: true }).click();
  await expect(app.getByRole("heading", { name: "Eitt arrangement", level: 2 })).toBeVisible();
  expect(attempts).toBe(2); expect(keys[0]).toBeTruthy(); expect(keys[0]).toBe(keys[1]); expect(inputs[0]).toEqual(inputs[1]);
});

test("detail chat actions preserve the draft and make composition context explicit and removable", async ({ page }) => {
  const { app } = await setup(page);
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await composer.fill("Utkastet var her først");
  const field = await composer.elementHandle();
  await app.getByRole("link", { name: "Konsert med eit langt og lesbart namn", exact: true }).click();
  await app.getByRole("button", { name: "Vis samtalen", exact: true }).click();
  await expect(app.getByLabel("Meldingar om arrangementet").getByText("Rik melding", { exact: true })).toBeVisible();
  await expect(composer).toHaveValue("Utkastet var her først");
  await expect(app.getByRole("group", { name: "Arrangement for meldinga", exact: true })).toHaveCount(0);
  await app.getByRole("tab", { name: /Arrangement/ }).click();
  await app.getByRole("link", { name: "Konsert med eit langt og lesbart namn", exact: true }).click();
  await app.getByRole("button", { name: "Skriv om arrangementet", exact: true }).click();
  const context = app.getByRole("group", { name: "Arrangement for meldinga", exact: true });
  await expect(context).toBeVisible(); await expect(context).toContainText("Konsert med eit langt og lesbart namn");
  await expect(composer).toHaveValue("Utkastet var her først");
  expect(await field!.evaluate(element => element.isConnected)).toBe(true);
  await context.getByRole("button", { name: /Fjern/ }).click();
  await expect(context).toHaveCount(0); await expect(composer).toHaveValue("Utkastet var her først");
});


test("an explicit HTTP disable clears navigation and context before channel invalidation and can recover", async ({ page }) => {
  const { app, getEvent } = await setup(page);
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await composer.fill("Bevar dette utkastet"); const field = await composer.elementHandle();
  await app.getByRole("link", { name: "Konsert med eit langt og lesbart namn", exact: true }).click();
  await app.getByRole("button", { name: "Skriv om arrangementet", exact: true }).click();
  await expect(app.getByRole("group", { name: "Arrangement for meldinga", exact: true })).toBeVisible();
  let enabled = false;
  await page.route(new RegExp(`/api/v1/channels/${channel}/events(?:[?]|$)`), route => route.fulfill({ json: {
    events_enabled: enabled, events: enabled ? [getEvent()] : [], next_after: null, attention_count: 0,
    summary: { as_of: "2026-11-13T12:00:00Z", upcoming_count: enabled ? 1 : 0, ongoing_count: 0, next_event: enabled ? getEvent() : null }
  } }));
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  await expect(app.getByRole("tablist", { name: "Innhald i kanalen" })).toHaveCount(0);
  await expect(app.locator(".sp-channel-navigation")).toHaveCount(0);
  await expect(app.getByRole("group", { name: "Arrangement for meldinga", exact: true })).toHaveCount(0);
  await expect(composer).toHaveValue("Bevar dette utkastet"); expect(await field!.evaluate(element => element.isConnected)).toBe(true);
  enabled = true;
  await page.evaluate(() => document.dispatchEvent(new Event("visibilitychange")));
  await expect(app.getByRole("tab", { name: /1 komande arrangement/ })).toBeVisible();
  await expect(composer).toHaveValue("Bevar dette utkastet");
});
