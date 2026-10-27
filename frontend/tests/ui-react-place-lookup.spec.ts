import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";

const channelA = "00000000-0000-7000-8000-000000009101";
const channelB = "00000000-0000-7000-8000-000000009102";
const circle = "00000000-0000-7000-8000-000000009001";
const owner = "place-lookup-reader";
const id = (n: number) => `00000000-0000-7000-8000-${String(n).padStart(12, "0")}`;
const marker = "Opne stadssøket for ferske barforslag.";
type Command = { type: string; request_id?: string; payload?: { channel_id?: string; root_message_id?: string } };

async function fixture(page: Page, geoMode: "named" | "shared" | "forbidden" | "retry" | "geo-only" = "named") {
  const batches: string[][] = [];
  const searches: { id: string; body: unknown; account: string | undefined }[] = [];
  const routes: unknown[] = [];
  const geoCalls: { method: string; body: unknown }[] = [];
  let geoState: unknown = geoMode === "retry" ? { id: "roll-1", status: "failed", choices: [], error_code: "geocode_failed", result_message_id: null, origin_text: "Grand Hotel, København", can_retry_geocoding: true } : null;
  const errors: string[] = [];
  let hold = false, pending: (() => Promise<void>) | undefined;
  let socket: WebSocketRoute;
  const emit = (type: string, payload: unknown, requestId?: string) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload, request_id: requestId }));
  const message = (n: number, channelId = channelA, parent: string | null = null) => ({ id: id(n), channel_id: channelId,
    parent_message_id: parent, sender_id: n === 35 || n === 37 ? "agent" : "peer", sender_display_name: n === 35 || n === 37 ? "Maria" : "Ada",
    sequence: n, sent_at: "2026-01-01T12:00:00Z", body: n === 35 || n === 37 ? marker : `Vanleg melding ${n}` });
  page.on("pageerror", error => errors.push(error.message));
  await page.route("**/api/v1/me/notifications?*", route => route.fulfill({ json: {
    enabled: false, subscriptions: 0, public_key: null, channel_ids: [], preferences: { mode: "instant", direct_messages: true, mentions: true }
  } }));
  await page.route("**/api/v1/channels/*/place-lookups**", route => {
    const request = route.request();
    expect(request.headers()["x-sproyt-account"]).toBe(owner);
    const ids = (request.postDataJSON() as { message_ids: string[] }).message_ids;
    batches.push(ids);
    const approved = ids.filter(value => value === id(35) || value === id(37));
    return route.fulfill({ json: { message_ids: approved, google_message_ids: geoMode === "geo-only" ? [] : approved, geoapify_message_ids: geoMode === "forbidden" ? [] : approved } });
  });
  await page.route("**/api/v1/channels/*/messages/*/place-lookup**", async route => {
    const request = route.request();
    if (request.method() === "GET") return route.fulfill({ json: { query: "craft beer nær hotellet", origin_mode: "shared", named_origin: null, uses_shared_location: true, preview_binding: "a".repeat(64) } });
    searches.push({ id: new URL(request.url()).pathname.split("/").at(-2)!, body: request.postDataJSON(), account: request.headers()["x-sproyt-account"] });
    const resolve = () => route.fulfill({ json: { search: { summary: "Ferskt testresultat [0]", places: [{ id: "ChIJ-test",
      attribution: { title: "Kjelde A", url: "https://www.google.com/search?q=bar" }, googleMapsLinks: { placeUrl: "https://www.google.com/maps/place/A" } }] }, walking: null, route_tokens: [{ index: 0, token: "opaque-token" }] } }).catch(() => {});
    if (hold) { pending = resolve; return; }
    await resolve();
  });
  await page.route("**/api/v1/channels/*/messages/*/place-route**", route => {
    routes.push(route.request().postDataJSON());
    return route.fulfill({ json: { route: { routes: [{ distanceMeters: 1200, duration: "600s", attribution: { title: "Google Maps", url: "https://www.google.com/maps" } }] }, warning: "Gangruter er i beta." } });
  });
  await page.route("**/api/v1/channels/*/messages/*/geoapify-roll**", route => {
    const request = route.request(), method = request.method();
    geoCalls.push({ method, body: method === "POST" ? request.postDataJSON() : null });
    expect(request.headers()["x-sproyt-account"]).toBe(owner);
    if (method === "GET" && geoMode === "forbidden") return route.fulfill({ status: 403 });
    if (method === "GET") return route.fulfill({ json: { provider: "geoapify", preview_binding: "a".repeat(64), origin_mode: geoMode === "retry" ? "named" : geoMode, suggested_origin: geoMode === "shared" ? null : "Nørreport", radius_m: 1500,
      categories: ["catering.bar", "catering.pub"], state: geoState } });
    geoState = { id: "roll-1", status: "choose_origin", choices: [{ index: 0, label: "Nørreport, København" }], error_code: null, result_message_id: null };
    return route.fulfill({ json: geoState });
  });
  await page.route("**/api/v1/channels/*/messages/*/geoapify-roll/origin**", route => {
    geoCalls.push({ method: "ORIGIN", body: route.request().postDataJSON() });
    geoState = { id: "roll-1", status: "published", choices: [], error_code: null, result_message_id: id(40) };
    return route.fulfill({ json: geoState });
  });
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    socket = route;
    socket.onMessage(data => {
      const command = JSON.parse(String(data)) as Command;
      const reply = (type: string, payload: unknown = {}) => emit(type, payload, command.request_id);
      switch (command.type) {
        case "hello": reply("hello", { participant_id: owner }); break;
        case "ping": reply("pong"); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [[{ id: circle, name: "Krets", slug: "krets", created_by: "peer", created_at: "2026-01-01T00:00:00Z" }, "member"]] }); break;
        case "list_my_channels": reply("channels_listed", { channels: [channelA, channelB].map((channelId, i) => ({ id: channelId,
          slug: i ? "andre" : "prat", name: i ? "Andre" : "Prat", circle_id: circle, kind: "public", direct_user_id: null,
          is_direct: false, description: "", role: "member", last_read_sequence: 100, latest_sequence: 100 })) }); break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "subscribe_channel": reply("subscription_started", { channel_id: command.payload?.channel_id,
          history: command.payload?.channel_id === channelA ? Array.from({ length: 35 }, (_, i) => message(i + 1)) : [message(38, channelB)] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: command.payload?.channel_id, reactions: [] }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: command.payload?.channel_id, summaries: command.payload?.channel_id === channelA
          ? [{ root_message_id: id(35), reply_count: 1, unread_count: 0, latest_sequence: 37 }] : [] }); break;
        case "load_thread": reply("thread_loaded", { root_message_id: id(35), messages: [message(35), message(37, channelA, id(35))] }); break;
      }
    });
  });
  await page.goto(`/?participant=place-lookup&ui=react&channel=${channelA}`);
  const app = page.locator("#sproyt-react-preview");
  await expect(app.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  return { app, batches, searches, routes, geoCalls, errors, hold: () => { hold = true; }, resolve: async () => { await pending?.(); } };
}

for (const width of [320, 1280]) test(`place discovery is batched and requires deliberate consent at ${width}px`, async ({ page }) => {
  await page.setViewportSize({ width, height: 900 });
  const { app, batches, searches, routes, errors } = await fixture(page);
  await expect.poll(() => batches.reduce((sum, batch) => sum + batch.length, 0)).toBe(1);
  expect(batches.flat()).toEqual([id(35)]); // Ordinary messages trigger no discovery calls.
  expect(batches.every(batch => batch.length <= 30)).toBe(true);
  expect(searches).toHaveLength(0);
  const root = app.locator(`[data-message-id="${id(35)}"]`);
  const open = root.getByRole("button", { name: "Vis stadssøk" });
  await expect(open).toBeVisible(); await open.click();
  await expect(root.getByText("craft beer nær hotellet")).toBeVisible();
  await expect(root.getByText(/delte posisjonen blir òg send til Google/)).toBeVisible();
  expect(searches).toHaveLength(0);
  await root.getByRole("button", { name: "Søk med Google" }).click();
  await expect(root.getByText("Ferskt testresultat [0]")).toBeVisible();
  expect(searches).toEqual([{ id: id(35), body: { allow_shared_location: true, preview_binding: "a".repeat(64) }, account: owner }]);
  await expect(root.getByRole("link", { name: /Kjelde A · Google Maps/ })).toHaveAttribute("href", "https://www.google.com/search?q=bar");
  await expect(root.getByRole("link", { name: /Opne Kjelde A i Google Maps/ })).toHaveAttribute("href", "https://www.google.com/maps/place/A");
  expect(routes).toHaveLength(0);
  await root.getByRole("button", { name: "Vis gangavstand" }).click();
  await expect(root.getByText(/1,2 km/)).toBeVisible();
  await expect(root.getByText("Gangruter er i beta.")).toBeVisible();
  expect(routes).toEqual([{ token: "opaque-token", allow_shared_location: true }]);
  await page.reload();
  await expect(app.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  await expect(app.getByText("Ferskt testresultat [0]")).toHaveCount(0);
  expect(searches).toHaveLength(1);
  expect(errors).toEqual([]);
});

test("a result arriving after changing channels is discarded", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const { app, searches, hold, resolve, errors } = await fixture(page);
  const root = app.locator(`[data-message-id="${id(35)}"]`);
  await root.getByRole("button", { name: "Vis stadssøk" }).click();
  hold(); await root.getByRole("button", { name: "Søk med Google" }).click();
  await expect.poll(() => searches.length).toBe(1);
  await app.getByRole("button", { name: "# Andre", exact: true }).click();
  await expect(app.getByText("Vanleg melding 38")).toBeVisible();
  await resolve();
  await expect(app.getByText("Ferskt testresultat [0]")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("place lookup discovery also covers newly opened thread replies", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const { app, batches, searches } = await fixture(page);
  const root = app.locator(`.sp-timeline [data-message-id="${id(35)}"]`);
  await root.getByRole("button", { name: /^Opne tråd/ }).click();
  const reply = app.locator(`.sp-thread-pane [data-message-id="${id(37)}"]`);
  await expect(reply.getByRole("button", { name: "Vis stadssøk" })).toBeVisible();
  expect(batches.flat()).toContain(id(37));
  expect(searches).toHaveLength(0);
});

test("the requester explicitly authorizes Geoapify and selects an ambiguous origin", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 900 });
  const { app, searches, geoCalls } = await fixture(page);
  const root = app.locator(`[data-message-id="${id(35)}"]`);
  await root.getByRole("button", { name: "La agenten trille via Geoapify" }).click();
  await expect(root.getByText(/Trefflista blir gitt til agenten/)).toBeVisible();
  await expect(root.getByLabel("Utgangspunkt")).toHaveValue("Nørreport");
  expect(geoCalls.map(call => call.method)).toEqual(["GET"]);
  expect(searches).toHaveLength(0);
  await root.getByRole("button", { name: "Tillat oppslag og la agenten trille" }).click();
  await expect(root.getByRole("button", { name: "Nørreport, København" })).toBeVisible();
  expect(geoCalls[1]?.body).toEqual({ preview_binding: "a".repeat(64), allow_shared_location: false, origin_text: "Nørreport" });
  await root.getByRole("button", { name: "Nørreport, København" }).click();
  await expect(root.getByText("Agenten har lagt svaret i samtalen.")).toBeVisible();
  expect(geoCalls[2]).toEqual({ method: "ORIGIN", body: { roll_id: "roll-1", index: 0 } });
});

test("shared location is disclosed before consent; other users cannot authorize", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 900 });
  const { app, geoCalls } = await fixture(page, "shared");
  const root = app.locator(`[data-message-id="${id(35)}"]`);
  await root.getByRole("button", { name: "La agenten trille via Geoapify" }).click();
  await expect(root.getByText(/delte posisjonen din blir send til Geoapify/)).toBeVisible();
  expect(geoCalls.map(call => call.method)).toEqual(["GET"]);
  await root.getByRole("button", { name: "Tillat oppslag og la agenten trille" }).click();
  await expect.poll(() => geoCalls.length).toBe(2);
  expect(geoCalls[1]?.body).toEqual({ preview_binding: "a".repeat(64), allow_shared_location: true });
});

test("Geoapify action is hidden when unavailable or the viewer is not the requester", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 900 });
  const { app, geoCalls } = await fixture(page, "forbidden");
  const root = app.locator(`[data-message-id="${id(35)}"]`);
  await expect(root.getByRole("button", { name: "Vis stadssøk" })).toBeVisible();
  await expect(root.getByRole("button", { name: "La agenten trille via Geoapify" })).toHaveCount(0);
  expect(geoCalls).toHaveLength(0);
});

test("Geoapify-only configuration does not offer an unavailable Google search", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 900 });
  const { app, searches, geoCalls } = await fixture(page, "geo-only");
  const root = app.locator(`[data-message-id="${id(35)}"]`);
  await expect(root.getByRole("button", { name: "La agenten trille via Geoapify" })).toBeVisible();
  await expect(root.getByRole("button", { name: "Vis stadssøk" })).toHaveCount(0);
  expect(searches).toHaveLength(0);
  expect(geoCalls).toHaveLength(0);
});

test("geocoding retry reuses the exact original origin and binding", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 900 });
  const { app, geoCalls } = await fixture(page, "retry");
  const root = app.locator(`[data-message-id="${id(35)}"]`);
  await root.getByRole("button", { name: "La agenten trille via Geoapify" }).click();
  await expect(root.getByText(/Grand Hotel, København/)).toBeVisible();
  await root.getByRole("button", { name: "Prøv stadoppslaget igjen" }).click();
  await expect.poll(() => geoCalls.length).toBe(2);
  expect(geoCalls[1]?.body).toEqual({ preview_binding: "a".repeat(64), allow_shared_location: false, origin_text: "Grand Hotel, København" });
});
