import { expect, test, type Page } from "@playwright/test";

const channel = "00000000-0000-7000-8000-000000003201";
const circle = "00000000-0000-7000-8000-000000003202";
const messages = ["Kort melding med **utheving**.\n\nEit nytt avsnitt.", "Langt namn og status skal ikkje flytte symbola ut på ei tilfeldig rad."];

async function fixture(page: Page, theme: string) {
  await page.addInitScript(mode => localStorage.setItem("sproyt.theme.v1", mode), theme);
  await page.route("**/api/v1/me/favorite-circles", route => route.fulfill({ json: [] }));
  const profiles = ["Rolf Tverbakk", "Alexandra Marie Langt Mellomnamn"].map((name, i) => ({
    id: `layout-peer-${i}`, kind: "human", display_name: name, handle: null,
    external_provider: null, external_subject: null, created_at: "2026-01-01T00:00:00Z",
    status_text: i ? "På ferie med gode vener på Paros" : "", status_emoji: i ? "🏖️" : "",
    status_expires_at: null, early_adopter: true
  }));
  await page.routeWebSocket(/\/ws(?:\?|$)/, socket => socket.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload, request_id: command.request_id }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: "layout-owner" }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: profiles }); break;
      case "list_my_circles": reply("circles_listed", { circles: [[{ id: circle, name: "Reiseglede", slug: "reiseglede", created_by: "layout-owner", created_at: "2026-01-01T00:00:00Z" }, "member"]] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, circle_id: circle, name: "26-10 Ølesund", slug: "olesund", description: "", kind: "public", is_direct: false, direct_user_id: null, role: "member", last_read_sequence: 2, latest_sequence: 2 }] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: messages.map((body, i) => ({
        id: `00000000-0000-7000-8000-00000000320${i + 3}`, channel_id: channel, parent_message_id: null,
        sender_id: profiles[i]!.id, sender_display_name: profiles[i]!.display_name, body,
        sequence: i + 1, sent_at: "2026-10-28T12:00:00Z", edited_at: null, deleted_at: null
      })) }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
    }
  }));
  await page.goto(`/?participant=layout-owner&channel=${channel}`);
  const app = page.locator("#sproyt-react-preview");
  await expect(app.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  await expect(app.locator(".sp-message-meta strong").first()).toHaveText("Rolf Tverbakk");
  return app;
}

for (const width of [320, 390, 430, 820, 900, 901, 1440]) for (const theme of ["light", "dark"]) {
  test(`identity and grouped actions at ${width}px ${theme}`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    const app = await fixture(page, theme);
    if (width <= 900) {
      const title = app.locator(".sp-mobile-conversation-title");
      await expect(title).toBeVisible();
      await expect(title.locator(".sp-mobile-context-name")).toHaveText("Reiseglede");
      await expect(title.locator(".sp-mobile-conversation-short-name")).toHaveText("# 26-10 Ølesund");
      expect(await title.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
      await expect(app.getByRole("navigation", { name: "Samtalar", exact: true })).toBeHidden();
      await title.click();
      const menu = app.getByRole("group", { name: "Samtaleinformasjon", exact: true });
      await expect(menu.getByRole("button", { name: "Kanaldetaljar", exact: true })).toBeVisible();
      await expect(menu.getByRole("button", { name: "Kanalval", exact: true })).toHaveCount(0);
      await page.keyboard.press("Escape");
      await expect(menu).toBeHidden();
      await expect(title).toBeFocused();
    } else {
      const nav = app.getByRole("navigation", { name: "Samtalar", exact: true });
      expect((await nav.boundingBox())!.width).toBeGreaterThanOrEqual(240);
    }
    const rows = app.locator(".sp-channel-pane [data-message-id]");
    for (const row of await rows.all()) {
      const meta = (await row.locator(".sp-message-meta").boundingBox())!;
      const body = (await row.locator(".sp-message-body").boundingBox())!;
      const buttons = await row.locator(".sp-message-primary-actions > button").all();
      const bounds = await Promise.all(buttons.map(button => button.boundingBox()));
      expect(Math.max(...bounds.map(b => b!.y)) - Math.min(...bounds.map(b => b!.y))).toBeLessThan(1);
      expect(body.y).toBeGreaterThanOrEqual(meta.y + meta.height - 1);
      expect(bounds[0]!.x).toBeGreaterThanOrEqual(meta.x + meta.width - 1);
    }
    const first = rows.first();
    expect((await first.locator(".sp-message-body").boundingBox())!.y - (await first.boundingBox())!.y).toBeLessThanOrEqual(56);
    await expect(first.locator(".sp-message-content")).toHaveCSS("white-space", "normal");
    await expect(first.locator(".sp-message-content")).toHaveCSS("font-size", "16px");
    expect(await app.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
    await page.screenshot({ path: testInfo.outputPath("layout.png") });
  });
}

test("coarse pointer and enlarged text retain visible actions and timestamp access", async ({ browser, baseURL }, testInfo) => {
  const context = await browser.newContext({ baseURL, serviceWorkers: "block", viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  try {
    const page = await context.newPage();
    const app = await fixture(page, "light");
    const row = app.locator(".sp-channel-pane [data-message-id]").first();
    for (const button of await row.locator(".sp-message-primary-actions > button").all()) {
      const rect = (await button.boundingBox())!;
      expect(rect.width).toBeGreaterThanOrEqual(44); expect(rect.height).toBeGreaterThanOrEqual(44);
    }
    await row.locator("time").tap();
    await expect(row.locator(".sp-message-time-tooltip")).toBeVisible();
    await app.getByRole("textbox", { name: "Skriv melding" }).tap();
    await expect(row.locator(".sp-message-time-tooltip")).toBeHidden();
    await page.addStyleTag({ content: "html { font-size: 24px !important; }" });
    expect(await app.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
    await expect(app.locator(".sp-mobile-conversation-title")).toContainText("26-10 Ølesund");
    await page.screenshot({ path: testInfo.outputPath("enlarged-touch.png") });
  } finally { await context.close(); }
});
