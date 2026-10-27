import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";

const channelA = "00000000-0000-7000-8000-000000009101";
const channelB = "00000000-0000-7000-8000-000000009102";
const circle = "00000000-0000-7000-8000-000000009001";
const owner = "place-lookup-reader";
const id = (n: number) => `00000000-0000-7000-8000-${String(n).padStart(12, "0")}`;
const marker = "Opne stadssøket for ferske barforslag.";
type Command = { type: string; request_id?: string; payload?: { channel_id?: string; root_message_id?: string } };

async function fixture(page: Page) {
  const batches: string[][] = [];
  const searches: { id: string; body: unknown; account: string | undefined }[] = [];
  const routes: unknown[] = [];
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
    return route.fulfill({ json: { message_ids: ids.filter(value => value === id(35) || value === id(37)) } });
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
  return { app, batches, searches, routes, errors, hold: () => { hold = true; }, resolve: async () => { await pending?.(); } };
}

for (const width of [320, 1280]) test(`place discovery is batched and requires deliberate consent at ${width}px`, async ({ page }) => {
  await page.setViewportSize({ width, height: 900 });
  const { app, batches, searches, routes, errors } = await fixture(page);
  await expect.poll(() => batches.reduce((sum, batch) => sum + batch.length, 0)).toBe(35);
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
