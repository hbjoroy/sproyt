import { expect, test, type Page } from "@playwright/test";
import { destinationSurface } from "./support/destination-surface";

const first = "00000000-0000-7000-8000-000000003420", second = "00000000-0000-7000-8000-000000003421";
const channel = "00000000-0000-7000-8000-000000003422";
const otherChannel = "00000000-0000-7000-8000-000000003423";
function process(id = "pilot:one", title = "Planlegg sommartreffet i den lange vennekretsen") {
  return { id, kind: "pilot", title, channel_id: channel, channel_name: "Planlegging og praktiske avklaringar",
    status: "failed", definition_name: "event-planning", runtime: "v2", projection: true,
    steps: [{ name: "Stadfest tidspunkt", status: "completed" }, { name: "Hent svar frå deltakarane", status: "failed" }],
    history: [{ name: "Prosessen vart starta", status: "completed", occurred_at: "2026-11-07T10:00:00Z" }],
    remaining_steps: null, steps_truncated: true, history_truncated: false };
}
async function fixture(page: Page, role: "owner" | "moderator" | "member" = "moderator") {
  let identity = "process-moderator", circleRole = role, channelPresent = true;
  const channels = () => (channelPresent ? [channel, otherChannel] : [otherChannel]).map(id => ({ id, slug: id,
    name: id === channel ? "Planlegging" : "Arbeidsprosessar", circle_id: first, direct_user_id: null, is_direct: false,
    kind: "public", role: "member", description: "", last_read_sequence: 0, latest_sequence: 0 }));
  let emit = (_type: string, _payload?: unknown) => {};
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    emit = (type, payload) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload }));
    route.onMessage(data => {
      const command = JSON.parse(String(data));
      const reply = (type: string, payload: unknown = {}) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
      switch (command.type) {
        case "hello": reply("hello", { participant_id: identity }); break;
        case "ping": reply("pong"); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [first, second].map((id, index) => [{ id, slug: id, name: index ? "Andre krets" : "Prosesskrets", created_by: identity, created_at: "2026-01-01T00:00:00Z" }, circleRole]) }); break;
        case "list_my_channels": reply("channels_listed", { channels: channels() }); break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "subscribe_channel": reply("subscription_started", { channel_id: command.payload.channel_id, history: [] }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: command.payload.channel_id, summaries: [] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: command.payload.channel_id, reactions: [] }); break;
        case "load_recent_messages": reply("messages_loaded", { channel_id: command.payload.channel_id, messages: [] }); break;
      }
    });
  });
  return { revoke: () => { circleRole = "member"; emit("circles_changed"); },
    revokeChannel: () => { channelPresent = false; emit("membership_left", { channel_id: channel }); },
    switchAccount: () => { identity = "another-account"; emit("hello", { participant_id: identity }); } };
}
async function openCircle(page: Page) {
  await page.goto(`/?participant=process-moderator&view=settings&section=circle&circle=${first}&channel=${channel}`);
  return destinationSurface(page, "Prosesskrets", true);
}

for (const theme of ["light", "dark"] as const) test(`moderators find readable processes on mobile in ${theme} theme`, async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ colorScheme: theme });
  await fixture(page);
  const requests: URL[] = [];
  await page.route(/\/api\/v1\/circles\/[^/]+\/processes\?/, async route => {
    expect(route.request().method()).toBe("GET");
    expect(await route.request().headerValue("x-sproyt-account")).toBe("process-moderator");
    const url = new URL(route.request().url()); requests.push(url);
    await route.fulfill({ json: { processes: [process()], next_cursor: null } });
  });
  const circle = await openCircle(page);
  const overview = circle.getByRole("region", { name: "Prosessar", exact: true });
  await expect(overview.getByText(process().title, { exact: true })).toBeVisible();
  await overview.locator("summary").focus();
  await page.keyboard.press("Enter");
  await expect(overview.locator("details")).toHaveAttribute("open", "");
  await expect(overview).toContainText("# Planlegging og praktiske avklaringar");
  await expect(overview).toContainText("Sist registrerte status: Treng oppfølging");
  await expect(overview).toContainText("Heart opplyser ikkje om attståande steg.");
  await expect(overview).toContainText("Prosessen vart starta");
  await expect(overview).toContainText("Dette er eit utdrag av dei registrerte stega.");
  await overview.getByLabel("Vis òg avslutta prosessar").check();
  await expect.poll(() => requests.at(-1)?.searchParams.get("state")).toBe("all");
  expect(requests[0]!.searchParams.get("state")).toBe("open");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("overview recovers from errors, pages, and removes rows after access failure", async ({ page }) => {
  await fixture(page);
  let failure = true, malformed = false, pageFailure = false, revoked = false;
  await page.route(/\/api\/v1\/circles\/[^/]+\/processes\?/, async route => {
    if (failure || revoked) return route.fulfill({ status: revoked ? 403 : 503, body: "Prosessane er ikkje tilgjengelege no." });
    const after = new URL(route.request().url()).searchParams.get("after");
    if (malformed) return route.fulfill({ json: { processes: [{ title: "Malformed row must not render" }], next_cursor: null } });
    if (after && pageFailure) return route.fulfill({ status: 503, body: "Neste side kunne ikkje hentast." });
    return route.fulfill({ json: { processes: [process(after ? "work_review:two" : "pilot:one", after ? "Andre prosess" : "Første prosess")], next_cursor: after ? null : "opaque-page-two" } });
  });
  const circle = await openCircle(page), overview = circle.getByRole("region", { name: "Prosessar", exact: true });
  await expect(overview).toContainText("Prosessane er ikkje tilgjengelege no.");
  failure = false;
  malformed = true;
  await overview.getByRole("button", { name: "Prøv igjen", exact: true }).click();
  await expect(overview).toContainText("Kunne ikkje lese prosessoversikta.");
  await expect(overview.getByText("Malformed row must not render")).toHaveCount(0);
  malformed = false;
  await overview.getByRole("button", { name: "Prøv igjen", exact: true }).click();
  await expect(overview.getByText("Første prosess", { exact: true })).toBeVisible();
  pageFailure = true;
  await overview.getByRole("button", { name: "Vis fleire prosessar" }).click();
  await expect(overview).toContainText("Neste side kunne ikkje hentast.");
  await expect(overview.locator("details")).toHaveCount(0);
  pageFailure = false;
  await overview.getByRole("button", { name: "Prøv igjen", exact: true }).click();
  await expect(overview.getByText("Første prosess", { exact: true })).toBeVisible();
  await overview.getByRole("button", { name: "Vis fleire prosessar" }).click();
  await expect(overview.getByText("Andre prosess", { exact: true })).toBeVisible();
  revoked = true;
  await overview.getByRole("button", { name: "Oppdater prosessar" }).click();
  await expect(overview).toContainText("Prosessane er ikkje tilgjengelege no.");
  await expect(overview.locator("details")).toHaveCount(0);
});

for (const boundary of ["account", "role"] as const) test(`pending overview reply is discarded after ${boundary} changes`, async ({ page }) => {
  const server = await fixture(page);
  let release: (() => Promise<void>) | undefined;
  await page.route(/\/api\/v1\/circles\/[^/]+\/processes\?/, async route => {
    await new Promise<void>(resolve => { release = async () => { await route.fulfill({ json: { processes: [process("pilot:secret", "Fortruleg prosess")], next_cursor: null } }).catch(() => {}); resolve(); }; });
  });
  await openCircle(page);
  await expect.poll(() => Boolean(release)).toBe(true);
  if (boundary === "account") server.switchAccount(); else server.revoke();
  await expect(page.getByRole("region", { name: "Prosessar", exact: true })).toHaveCount(0);
  await release!();
  await expect(page.getByText("Fortruleg prosess", { exact: true })).toHaveCount(0);
});

test("late results cannot cross circle, account or moderator-role boundaries", async ({ page }) => {
  const server = await fixture(page);
  let release: (() => Promise<void>) | undefined;
  await page.route(/\/api\/v1\/circles\/[^/]+\/processes\?/, async route => {
    if (route.request().url().includes(second)) return route.fulfill({ json: { processes: [], next_cursor: null } });
    await new Promise<void>(resolve => { release = async () => { await route.fulfill({ json: { processes: [process("pilot:secret", "Privat gammal prosess")], next_cursor: null } }).catch(() => {}); resolve(); }; });
  });
  let circle = await openCircle(page);
  await expect(circle.getByText("Hentar prosessar …", { exact: true })).toBeVisible();
  await expect.poll(() => Boolean(release)).toBe(true);
  await circle.getByRole("button", { name: "Tilbake til kretsane" }).click();
  await destinationSurface(page, "Vel krets", true).getByRole("button", { name: "Andre krets", exact: true }).click();
  await release!();
  circle = destinationSurface(page, "Andre krets", true);
  await expect(circle).toContainText("Ingen opne prosessar i kanalane du har tilgang til.");
  await expect(page.getByText("Privat gammal prosess", { exact: true })).toHaveCount(0);
  server.revoke();
  await expect(circle.getByRole("region", { name: "Prosessar", exact: true })).toHaveCount(0);
  server.switchAccount();
  await expect(page.getByRole("region", { name: "Prosessar", exact: true })).toHaveCount(0);
});

test("ordinary members have no process overview or lookup", async ({ page }) => {
  await fixture(page, "member");
  let requests = 0;
  await page.route(/\/api\/v1\/circles\/[^/]+\/processes\?/, async route => { requests++; await route.fulfill({ json: { processes: [], next_cursor: null } }); });
  const circle = await openCircle(page);
  await expect(circle.getByText("Krets / Prosesskrets · Medlem")).toBeVisible();
  await expect(circle.getByRole("region", { name: "Prosessar", exact: true })).toHaveCount(0);
  expect(requests).toBe(0);
});

for (const pending of [false, true]) test(`channel revocation clears ${pending ? "pending" : "rendered"} rows while circle moderator remains`, async ({ page }) => {
  const server = await fixture(page);
  let revoked = false, release: (() => Promise<void>) | undefined;
  await page.route(/\/api\/v1\/circles\/[^/]+\/processes\?/, async route => {
    if (revoked) return route.fulfill({ json: { processes: [], next_cursor: null } });
    // Deliberately display a different channel: a hidden task-channel membership
    // can govern a work process whose returned source channel remains unchanged.
    const snapshot = { processes: [{ ...process("work_review:secret", "Fortruleg arbeidsprosess"), channel_id: otherChannel }], next_cursor: null };
    if (!pending) return route.fulfill({ json: snapshot });
    await new Promise<void>(resolve => { release = async () => { await route.fulfill({ json: snapshot }).catch(() => {}); resolve(); }; });
  });
  const circle = await openCircle(page), overview = circle.getByRole("region", { name: "Prosessar", exact: true });
  if (pending) await expect.poll(() => Boolean(release)).toBe(true);
  else await expect(overview.getByText("Fortruleg arbeidsprosess", { exact: true })).toBeVisible();
  revoked = true; server.revokeChannel();
  await expect(circle).toContainText("Krets / Prosesskrets · Moderator");
  await expect(circle.getByRole("region", { name: "Prosessar", exact: true })).toContainText("Ingen opne prosessar i kanalane du har tilgang til.");
  if (release) await release();
  await expect(page.getByText("Fortruleg arbeidsprosess", { exact: true })).toHaveCount(0);
});
