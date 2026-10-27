import { expect, test, type Page } from "@playwright/test";

const channelId = "00000000-0000-7000-8000-000000005001";
type FixtureOptions = {
  permission?: NotificationPermission;
  install?: boolean;
  subscription?: boolean;
  checkStatus?: number;
  settingsStatus?: number;
};

async function setup(page: Page, options: FixtureOptions = {}) {
  let owner = "device-owner-a";
  let emit = (_type: string, _payload?: unknown) => {};
  const checkedOwners: string[] = [];
  let settingsReads = 0;
  let checkReads = 0;
  const subscriptionEndpoint = "https://push.example.test/device";

  await page.addInitScript(({ permission, install, subscription, subscriptionEndpoint }) => {
    const increment = (key: string) => sessionStorage.setItem(key, String(Number(sessionStorage.getItem(key) ?? "0") + 1));
    class MockNotification {
      static permission = permission;
      static async requestPermission() { increment("test:notification-request"); return permission; }
    }
    Object.defineProperty(window, "Notification", { configurable: true, value: MockNotification });
    Object.defineProperty(window, "PushManager", { configurable: true, value: class {} });
    const registration = {
      pushManager: {
        getSubscription: async () => subscription ? { endpoint: subscriptionEndpoint } : null,
        subscribe: async () => { increment("test:push-subscribe"); return { endpoint: subscriptionEndpoint }; }
      },
      active: null, installing: null, waiting: null,
      update: async () => {}, addEventListener: () => {}, removeEventListener: () => {}
    };
    const serviceWorker = Object.assign(new EventTarget(), {
      register: async () => registration,
      ready: Promise.resolve(registration),
      controller: null
    });
    Object.defineProperty(navigator, "serviceWorker", { configurable: true, value: serviceWorker });
    if (install) {
      // Dispatch synchronously as the startup module attaches its listener,
      // before the React UI mounts, to exercise early event capture.
      const add = window.addEventListener.bind(window);
      let sent = false;
      window.addEventListener = ((type: string, listener: EventListenerOrEventListenerObject, opts?: boolean | AddEventListenerOptions) => {
        add(type, listener, opts);
        if (type === "beforeinstallprompt" && !sent) {
          sent = true;
          const event = new Event(type, { cancelable: true }) as Event & { prompt(): Promise<void> };
          event.prompt = async () => { increment("test:install-prompt"); };
          window.dispatchEvent(event);
        }
      }) as typeof window.addEventListener;
    }
  }, { permission: options.permission ?? "default", install: options.install ?? false,
    subscription: options.subscription ?? false, subscriptionEndpoint });

  await page.route("**/api/v1/me/notifications**", async route => {
    settingsReads++;
    if (options.settingsStatus) return route.fulfill({ status: options.settingsStatus, body: "Temporarily unavailable" });
    return route.fulfill({ json: { enabled: true, public_key: "test-public-key", subscriptions: 1, channel_ids: [],
      preferences: { mode: "instant", direct_messages: true, mentions: true } } });
  });
  await page.route("**/api/v1/me/push-subscriptions/check**", async route => {
    checkReads++;
    if (options.checkStatus) return route.fulfill({ status: options.checkStatus, body: "Temporarily unavailable" });
    return route.fulfill({ json: { registered: true } });
  });
  await page.routeWebSocket(/\/ws(?:\?|$)/, socket => {
    emit = (type, payload) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload }));
    socket.onMessage(data => {
      const command = JSON.parse(String(data));
      const reply = (type: string, payload: unknown = {}) => socket.send(JSON.stringify({
        protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload
      }));
      switch (command.type) {
        case "hello": reply("hello", { participant_id: owner }); break;
        case "ping": reply("pong"); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [] }); break;
        case "list_my_channels":
          checkedOwners.push(owner);
          reply("channels_listed", { channels: [{ id: channelId, slug: "general", name: "General", kind: "public",
            circle_id: null, direct_user_id: null, is_direct: false, description: "", role: "member", last_read_sequence: 0, latest_sequence: 0 }] });
          break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "subscribe_channel": reply("subscription_started", { channel_id: channelId, history: [] }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channelId, summaries: [] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channelId, reactions: [] }); break;
        case "load_recent_messages": reply("messages_loaded", { channel_id: channelId, messages: [] }); break;
      }
    });
  });
  return {
    switchAccount(next: string) { owner = next; emit("hello", { participant_id: owner }); },
    get checkedOwners() { return checkedOwners; },
    get settingsReads() { return settingsReads; },
    get checkReads() { return checkReads; }
  };
}

async function openApp(page: Page) {
  await page.goto(`/?participant=device-owner-a&channel=${channelId}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  return preview;
}

test("device reminders preserve draft, require a gesture and stay independently dismissed across reload and accounts", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const server = await setup(page, { install: true });
  const preview = await openApp(page);
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await composer.fill("Eit utkast som skal bli verande");
  const notifications = preview.getByRole("region", { name: "Påminning om varsel" });
  const install = preview.getByRole("region", { name: "Påminning om Sprøyt som app" });
  await expect(notifications).toBeVisible();
  await expect(install).toHaveCount(0);
  const counters = () => page.evaluate(() => [sessionStorage.getItem("test:notification-request"), sessionStorage.getItem("test:push-subscribe"), sessionStorage.getItem("test:install-prompt")]);
  expect(await counters()).toEqual([null, null, null]);

  await notifications.getByRole("button", { name: "Opne varslingsinnstillingar" }).click();
  await expect(preview.getByRole("heading", { name: "Varslingsinnstillingar", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(composer).toHaveValue("Eit utkast som skal bli verande");
  expect(await counters()).toEqual([null, null, null]);

  await notifications.getByRole("button", { name: "Lukk påminning om varsel" }).click();
  await expect(install).toBeVisible();
  await page.reload();
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  await expect(notifications).toHaveCount(0);
  await expect(install).toBeVisible();

  server.switchAccount("device-owner-b");
  await expect.poll(() => server.checkedOwners).toContain("device-owner-b");
  await expect(notifications).toHaveCount(0);
  await expect(install).toBeVisible();
  await install.getByRole("button", { name: "Lukk påminning om Sprøyt som app" }).click();
  await expect(install).toHaveCount(0);
  const previousAccountLoads = server.checkedOwners.filter(owner => owner === "device-owner-b").length;
  await page.reload();
  await expect.poll(() => server.checkedOwners.filter(owner => owner === "device-owner-b").length).toBeGreaterThan(previousAccountLoads);
  // A new account must select its own authorised conversation: the previous
  // account's active view and draft are intentionally not restored for it.
  const nav = preview.getByRole("navigation", { name: "Samtalar", exact: true });
  if (!await nav.isVisible()) {
    const show = preview.getByRole("button", { name: "Vis samtalar", exact: true });
    if (await show.isVisible()) await show.click();
    else await preview.getByRole("button", { name: "Samtalar", exact: true }).click();
  }
  const shared = nav.locator('[data-conversation-group="scope:shared"]');
  const disclosure = shared.locator(".sp-product-nav-disclosure");
  if (await disclosure.getAttribute("aria-expanded") === "false") await disclosure.click();
  await shared.getByRole("button", { name: "# General", exact: true }).click();
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  await expect(composer).toHaveValue("");
  await expect(notifications).toHaveCount(0);
  await expect(install).toHaveCount(0);
  expect(await counters()).toEqual([null, null, null]);
});

for (const scenario of ["registered", "check-failed", "settings-failed"] as const) {
  test(`${scenario} push state never claims notifications are disabled`, async ({ page }) => {
    const server = await setup(page, { permission: "granted", subscription: true,
      ...(scenario === "check-failed" ? { checkStatus: 503 } : {}),
      ...(scenario === "settings-failed" ? { settingsStatus: 503 } : {}) });
    const preview = await openApp(page);
    if (scenario === "settings-failed") await expect.poll(() => server.settingsReads).toBeGreaterThan(0);
    else await expect.poll(() => server.checkReads).toBeGreaterThan(0);
    // Trigger one check after the UI is mounted and wait for its response, so
    // the absence assertion also covers the settled failure/success path.
    const response = page.waitForResponse(value => value.url().includes(scenario === "settings-failed"
      ? "/api/v1/me/notifications" : "/api/v1/me/push-subscriptions/check"));
    await page.evaluate(() => window.dispatchEvent(new Event("sproyt:push-settings-changed")));
    await (await response).finished();
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    await expect(preview.getByRole("region", { name: "Påminning om varsel" })).toHaveCount(0);
    expect(await page.evaluate(() => sessionStorage.getItem("test:notification-request"))).toBeNull();
  });
}

test("the browser installation prompt is opened only by the install action", async ({ page }) => {
  await setup(page, { install: true });
  const preview = await openApp(page);
  await preview.getByRole("region", { name: "Påminning om varsel" }).getByRole("button", { name: "Lukk påminning om varsel" }).click();
  const install = preview.getByRole("region", { name: "Påminning om Sprøyt som app" });
  await expect(install).toBeVisible();
  expect(await page.evaluate(() => sessionStorage.getItem("test:install-prompt"))).toBeNull();
  await install.getByRole("button", { name: "Installer appen" }).click();
  await expect.poll(() => page.evaluate(() => sessionStorage.getItem("test:install-prompt"))).toBe("1");
  // iPhone/WebKit may fall back to its manual home-screen instructions after
  // the synthetic native event has been consumed.
  await expect(install.getByRole("button", { name: "Installer appen" })).toHaveCount(0);
});
