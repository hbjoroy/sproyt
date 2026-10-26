import { expect, test, type Locator, type Page, type WebSocketRoute } from "@playwright/test";

test.setTimeout(45_000);
const circleId = (index: number) => `00000000-0000-7000-8000-${String(7100 + index).padStart(12, "0")}`;
const channelId = (index: number) => `00000000-0000-7000-8000-${String(7200 + index).padStart(12, "0")}`;
const directId = "00000000-0000-7000-8000-000000007301";
const otherDirectId = "00000000-0000-7000-8000-000000007302";
type Command = { type: string; request_id?: string; payload?: { channel_id?: string; circle_id?: string; sequence?: number } };

async function fixture(page: Page, start = `channel=${channelId(29)}`) {
  let socket: WebSocketRoute;
  let identity = "navigation-owner";
  const commands: Command[] = [];
  const profilePins = new Map<string, Set<string>>();
  await page.route(/\/api\/v1\/me\/favorite-circles(?:\/[^?]+)?(?:\?.*)?$/, async route => {
    const owner = route.request().headers()["x-sproyt-account"]!;
    const pins = profilePins.get(owner) ?? new Set<string>();
    if (route.request().method() === "GET") return route.fulfill({ json: [...pins] });
    const id = new URL(route.request().url()).pathname.split("/").at(-1)!;
    if (route.request().method() === "PUT") pins.add(id); else pins.delete(id);
    profilePins.set(owner, pins); await route.fulfill({ status: 204 });
  });
  const circles = Array.from({ length: 30 }, (_, index) => ({ id: circleId(index), slug: `circle-${index}`,
    name: `Krets ${String(index + 1).padStart(2, "0")}${index === 5 ? ` ${"Langt kretsnamn ".repeat(8)}` : ""}`,
    created_by: "owner", created_at: "2026-01-01T00:00:00Z" }));
  const channels = circles.map((circle, index) => ({ id: channelId(index), slug: `channel-${index}`,
    name: `Kanal ${String(index + 1).padStart(2, "0")}`, kind: "private", circle_id: circle.id,
    direct_user_id: null, is_direct: false, description: "", role: "member", last_read_sequence: 0,
    latest_sequence: index === 0 ? 5 : 0 }));
  const directs = [
    { id: directId, slug: "direct-ada", name: "Ada", direct_user_id: "ada" },
    { id: otherDirectId, slug: "direct-bert", name: "Bert", direct_user_id: "bert" }
  ].map(channel => ({ ...channel, kind: "private", circle_id: null, is_direct: true, description: "",
    role: "member", last_read_sequence: 0, latest_sequence: 0 }));
  const emit = (type: string, payload: unknown = {}, request_id?: string) => socket.send(JSON.stringify({
    protocol: "sproyt.chat.v1", type, payload, ...(request_id ? { request_id } : {})
  }));
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    socket = route;
    route.onMessage(data => {
      const command = JSON.parse(String(data)) as Command;
      commands.push(command);
      const reply = (type: string, payload: unknown = {}) => emit(type, payload, command.request_id);
      const selected = command.payload?.channel_id;
      switch (command.type) {
        case "hello": reply("hello", { participant_id: identity }); break;
        case "ping": reply("pong"); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: circles.map(circle => [circle, "member"]) }); break;
        case "list_my_channels": reply("channels_listed", { channels: [...channels, ...directs] }); break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "list_joinable_channels": reply("joinable_channels_listed", { circle_id: command.payload?.circle_id, channels: [] }); break;
        case "subscribe_channel": reply("subscription_started", { channel_id: selected, history: [] }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: selected, summaries: [] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: selected, reactions: [] }); break;
        case "load_recent_messages": reply("messages_loaded", { channel_id: selected, messages: [] }); break;
      }
    });
  });
  await page.goto(`/?participant=product-navigation&${start}`);
  const app = page.locator("#sproyt-react-preview");
  await expect(app.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  return { app, commands, switchAccount: (next: string) => { identity = next; emit("hello", { participant_id: identity }); } };
}

async function navigation(app: Locator) {
  const back = app.getByRole("button", { name: "Samtalar", exact: true });
  if (await back.isVisible()) await back.click();
  const result = app.getByRole("navigation", { name: "Samtalar", exact: true });
  await expect(result).toBeVisible();
  return result;
}

test("many circles start compact, retain pins and expansion, and search opens matches temporarily", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 844 });
  const { app, commands } = await fixture(page);
  const nav = await navigation(app);
  await expect(nav.getByRole("tab", { name: /^Kretsar/ })).toHaveAttribute("aria-selected", "true");
  expect(await nav.locator(".sp-product-nav-group:visible").count()).toBeLessThanOrEqual(5);
  const selected = nav.locator(`[data-conversation-group="circle:${circleId(29)}"]`);
  await expect(selected.locator(".sp-product-nav-disclosure")).toHaveAttribute("aria-expanded", "true");
  await expect(selected.getByRole("button", { name: "# Kanal 30", exact: true })).toHaveAttribute("aria-current", "page");
  const more = nav.getByRole("button", { name: /Andre kretsar/ });
  await expect(more).toHaveAttribute("aria-expanded", "false");
  await expect(more.getByLabel("5 uleste")).toBeVisible();
  const subscriptions = commands.filter(command => command.type === "subscribe_channel").length;
  await more.click();
  const first = nav.locator(`[data-conversation-group="circle:${circleId(0)}"]`);
  await expect(first.locator(".sp-product-nav-disclosure")).toHaveAttribute("aria-expanded", "false");
  await first.getByRole("button", { name: "Fest Krets 01 som favoritt", exact: true }).click();
  await first.locator(".sp-product-nav-disclosure").click();
  await expect(first.getByRole("button", { name: /^# Kanal 01/ })).toBeVisible();
  expect(commands.filter(command => command.type === "subscribe_channel")).toHaveLength(subscriptions);
  await page.reload();
  await expect(app.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  await navigation(app);
  await expect(first.getByRole("button", { name: "Løys Krets 01 frå favorittar", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(first.locator(".sp-product-nav-disclosure")).toHaveAttribute("aria-expanded", "true");
  await nav.getByRole("searchbox", { name: "Finn samtale" }).fill("Kanal 02");
  const second = nav.locator(`[data-conversation-group="circle:${circleId(1)}"]`);
  await expect(second.locator(".sp-product-nav-disclosure")).toHaveAttribute("aria-expanded", "true");
  await expect(second.getByRole("button", { name: "# Kanal 02", exact: true })).toBeVisible();
  await nav.getByRole("searchbox", { name: "Finn samtale" }).fill("");
  await expect(second.locator(".sp-product-nav-disclosure")).toHaveAttribute("aria-expanded", "false");
});

test("favorites remain isolated when the active account changes", async ({ page }) => {
  const { app, switchAccount } = await fixture(page);
  const nav = await navigation(app);
  const selected = nav.locator(`[data-conversation-group="circle:${circleId(29)}"]`);
  await selected.getByRole("button", { name: "Fest Krets 30 som favoritt", exact: true }).click();
  await expect(selected.getByRole("button", { name: "Løys Krets 30 frå favorittar", exact: true })).toHaveAttribute("aria-pressed", "true");
  switchAccount("navigation-other-owner");
  await expect(selected.getByRole("button", { name: "Fest Krets 30 som favoritt", exact: true })).toHaveAttribute("aria-pressed", "false");
  switchAccount("navigation-owner");
  await expect(selected.getByRole("button", { name: "Løys Krets 30 frå favorittar", exact: true })).toHaveAttribute("aria-pressed", "true");
  const keys = await page.evaluate(() => Object.keys(localStorage).filter(key => key.startsWith("sproyt:conversation-navigation:")));
  expect(keys).toEqual([]); // Profile pins are not browser-local preferences.
});

for (const width of [1280, 320]) test(`direct directory has its own URL, new-message action and bounded layout at ${width}px`, async ({ page }, testInfo) => {
  await page.setViewportSize({ width, height: 844 });
  const { app } = await fixture(page);
  const nav = await navigation(app);
  await expect(nav.getByRole("button", { name: "Ny direktemelding", exact: true })).toBeHidden();
  const directTab = nav.getByRole("tab", { name: /^Direktemeldingar/ });
  await directTab.click();
  await expect(directTab).toHaveAttribute("aria-selected", "true");
  await expect.poll(() => new URL(page.url()).searchParams.get("view")).toBe("direct");
  await expect(nav.getByRole("button", { name: "Ada", exact: true })).toBeVisible();
  await expect(nav.getByRole("button", { name: "Bert", exact: true })).toBeVisible();
  const create = nav.getByRole("button", { name: "Ny direktemelding", exact: true });
  await expect(create).toBeVisible();
  await nav.getByRole("searchbox", { name: "Finn samtale" }).fill("ingen samtalar passar");
  await expect(create).toBeVisible();
  await create.click();
  const people = app.getByRole("dialog", { name: "Personar og ny direktemelding", exact: true });
  await expect(people.getByRole("searchbox", { name: "Finn person" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(create).toBeFocused();
  await nav.getByRole("searchbox", { name: "Finn samtale" }).fill("");
  await directTab.focus();
  await page.keyboard.press("ArrowLeft");
  await expect(nav.getByRole("tab", { name: /^Kretsar/ })).toBeFocused();
  await expect.poll(() => new URL(page.url()).searchParams.get("view")).toBe("circles");
  await nav.getByRole("button", { name: /Andre kretsar/ }).click();
  const longName = nav.locator(`[data-conversation-group="circle:${circleId(5)}"]`);
  await expect(longName.getByRole("button", { name: /^Fest Krets 06/ })).toBeVisible();
  expect(await nav.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  expect(await app.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  const bounds = await longName.locator(".sp-product-nav-pin").boundingBox();
  expect(bounds?.width).toBeGreaterThanOrEqual(44);
  expect(bounds?.height).toBeGreaterThanOrEqual(44);
  expect((bounds?.x ?? 0) + (bounds?.width ?? 0)).toBeLessThanOrEqual(width);
  await page.screenshot({ path: testInfo.outputPath(`product-navigation-${width}.png`) });
});

test("a direct-channel deep link selects the direct tab and retains both direct conversations", async ({ page }) => {
  const { app } = await fixture(page, `channel=${directId}`);
  const nav = await navigation(app);
  await expect(nav.getByRole("tab", { name: /^Direktemeldingar/ })).toHaveAttribute("aria-selected", "true");
  await expect(nav.getByRole("button", { name: "Ada", exact: true })).toHaveAttribute("aria-current", "page");
  await expect(nav.getByRole("button", { name: "Bert", exact: true })).toBeVisible();
});
