import { expect, test, type Page } from "@playwright/test";

const circle = "00000000-0000-7000-8000-000000002401";
const channel = "00000000-0000-7000-8000-000000002402";
const agent = "00000000-0000-7000-8000-000000002403";

async function conversation(page: Page) {
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => route.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: "location-user" }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [[{ id: circle, slug: "tur", name: "Turfolk", created_by: "location-user", created_at: "2026-01-01T00:00:00Z" }, "member"]] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, slug: "turprat", name: "Turprat", kind: "private", circle_id: circle, direct_user_id: null, is_direct: false,
        description: "", role: "member", last_read_sequence: 0, latest_sequence: 0 }] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: [] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
      case "load_recent_messages": reply("messages_loaded", { channel_id: channel, messages: [] }); break;
    }
  }));
}

async function openFromChannelMenu(page: Page) {
  const preview = page.locator("#sproyt-react-preview");
  const desktopChannelMenu = preview.getByRole("button", { name: "Kanalval", exact: true });
  if (await desktopChannelMenu.isVisible()) await desktopChannelMenu.click();
  else {
    const menu = preview.getByRole("button", { name: "Meny", exact: true });
    if (await menu.getAttribute("aria-expanded") !== "true") await menu.click();
    await preview.locator(".sp-mobile-channel-menu")
      .getByRole("button", { name: "Kanalval", exact: true }).click();
  }
  await preview.getByRole("dialog", { name: "Kanalval: Turprat", exact: true })
    .getByRole("button", { name: "Del posisjon med agent", exact: true }).click();
  return preview.getByRole("dialog", { name: "Del posisjon med agent", exact: true });
}

test("one-shot agent location shows consent, accuracy and expiry, supports removal, and preserves the draft", async ({ page }) => {
  await conversation(page);
  const writes: Array<{ method: string; body?: unknown }> = [];
  let location: unknown = null;
  await page.route(/\/api\/v1\/channels\/[^/]+\/agent-locations(?:\/[^?]+)?(?:\?|$)/, async route => {
    const method = route.request().method();
    if (method === "GET") { await route.fulfill({ json: { agents: [{ id: agent, name: "Vegvisar", location }] } }); return; }
    const body = route.request().postData();
    writes.push({ method, ...(body ? { body: JSON.parse(body) } : {}) });
    if (method === "DELETE") { location = null; await route.fulfill({ status: 204 }); return; }
    location = { latitude: 60.3913, longitude: 5.3221, accuracy_m: 17,
      observed_at: "2026-10-10T10:00:00.000Z", expires_at: "2026-10-10T10:30:00.000Z" };
    await route.fulfill({ json: location });
  });
  await page.addInitScript(() => {
    const state = window as typeof window & { __geoCalls: Array<PositionOptions | undefined> };
    state.__geoCalls = [];
    Object.defineProperty(navigator, "geolocation", { configurable: true, value: {
      getCurrentPosition(success: PositionCallback, _error: PositionErrorCallback, options?: PositionOptions) {
        state.__geoCalls.push(options);
        success({ coords: { latitude: 60.39131, longitude: 5.32211, accuracy: 17, altitude: null, altitudeAccuracy: null, heading: null, speed: null },
          timestamp: Date.now() } as GeolocationPosition);
      },
      watchPosition() { throw new Error("watchPosition must not be used"); }, clearWatch() {}
    } });
  });
  await page.goto(`/?participant=location-user&channel=${channel}`);
  const composer = page.getByRole("textbox", { name: "Skriv melding", exact: true });
  await composer.fill("Utkastet blir verande");
  // Composer tools are mounted but may stay collapsed until the user opens
  // the writing-tools disclosure. The channel menu below exercises the same
  // dialog without coupling this flow to the package-owned disclosure state.
  await expect(page.locator(".sp-composer-dock").getByRole("button", {
    name: "Del posisjon med agent", exact: true, includeHidden: true
  })).toHaveCount(1);
  const dialog = await openFromChannelMenu(page);
  await expect(dialog).toContainText("Agenten kan bruke posisjonen i svar her. Andre i kanalen kan då forstå kvar du er.");
  await expect(dialog).toContainText("sporar deg ikkje vidare");
  await dialog.getByRole("button", { name: "Del posisjonen min", exact: true }).click();
  await expect(dialog).toContainText("Posisjonen er delt med Vegvisar");
  await expect(dialog).toContainText("om lag 17 meter");
  await expect(dialog.locator("time")).toHaveCount(2);
  expect(writes[0]).toMatchObject({ method: "PUT", body: { latitude: 60.39131, longitude: 5.32211, accuracy_m: 17 } });
  const options = await page.evaluate(() => (window as typeof window & { __geoCalls: PositionOptions[] }).__geoCalls);
  expect(options).toEqual([{ enableHighAccuracy: true, maximumAge: 0, timeout: 15_000 }]);
  await dialog.getByRole("button", { name: "Fjern delinga", exact: true }).click();
  await expect(dialog).toContainText("ikkje lenger delt");
  expect(writes.map(item => item.method)).toEqual(["PUT", "DELETE"]);
  await dialog.getByRole("button", { name: "Lukk posisjonsdeling", exact: true }).click();
  await expect(composer).toHaveValue("Utkastet blir verande");
  expect(await page.evaluate(() => Object.keys(localStorage).filter(key => /location|posisjon/i.test(key)))).toEqual([]);
});

test("permission denial is recoverable and a late position after close cannot be shared", async ({ page }) => {
  await conversation(page);
  let puts = 0;
  await page.route(/\/api\/v1\/channels\/[^/]+\/agent-locations(?:\/[^?]+)?(?:\?|$)/, async route => {
    if (route.request().method() === "GET") await route.fulfill({ json: { agents: [{ id: agent, name: "Vegvisar", location: null }] } });
    else { puts++; await route.fulfill({ json: { latitude: 60, longitude: 5, accuracy_m: 10, observed_at: new Date().toISOString(), expires_at: new Date(Date.now() + 1_800_000).toISOString() } }); }
  });
  await page.addInitScript(() => {
    type GeoState = { success?: PositionCallback; error?: PositionErrorCallback };
    const state = window as typeof window & { __geo: GeoState };
    state.__geo = {};
    Object.defineProperty(navigator, "geolocation", { configurable: true, value: {
      getCurrentPosition(success: PositionCallback, error: PositionErrorCallback) { state.__geo = { success, error }; },
      watchPosition() { throw new Error("watchPosition must not be used"); }, clearWatch() {}
    } });
  });
  await page.goto(`/?participant=location-denied&channel=${channel}`);
  let dialog = await openFromChannelMenu(page);
  await dialog.getByRole("button", { name: "Del posisjonen min", exact: true }).click();
  await page.evaluate(() => {
    const callback = (window as typeof window & { __geo: { error?: PositionErrorCallback } }).__geo.error;
    callback?.({ code: 1, message: "denied", PERMISSION_DENIED: 1, POSITION_UNAVAILABLE: 2, TIMEOUT: 3 } as GeolocationPositionError);
  });
  await expect(dialog).toContainText("Tillat posisjon for Sprøyt i nettlesarinnstillingane, og prøv igjen");
  await expect(dialog.getByRole("button", { name: "Del posisjonen min", exact: true })).toBeEnabled();
  await dialog.getByRole("button", { name: "Del posisjonen min", exact: true }).click();
  await dialog.getByRole("button", { name: "Lukk posisjonsdeling", exact: true }).click();
  await page.evaluate(() => {
    const callback = (window as typeof window & { __geo: { success?: PositionCallback } }).__geo.success;
    callback?.({ coords: { latitude: 60, longitude: 5, accuracy: 10, altitude: null, altitudeAccuracy: null, heading: null, speed: null }, timestamp: Date.now() } as GeolocationPosition);
  });
  await page.waitForTimeout(50);
  expect(puts).toBe(0);
  dialog = page.getByRole("dialog", { name: "Del posisjon med agent", exact: true });
  await expect(dialog).toHaveCount(0);
});
