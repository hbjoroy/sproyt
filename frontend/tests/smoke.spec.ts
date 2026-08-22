import { expect, test, type WebSocket as PlaywrightWebSocket } from "@playwright/test";

declare global {
  interface Window {
    __sproytRecordCspViolation(violation: string): Promise<void>;
    __sproytE2eSockets: globalThis.WebSocket[];
    __sproytDmCommands: string[];
  }
}

test("development client loads through CSP, connects, and sends a message", async ({ page }) => {
  const appModuleRequests: string[] = [];
  const legacyStoreRequests: string[] = [];
  const consoleErrors: Array<{ text: string; observedAt: number }> = [];
  const pageErrors: string[] = [];
  const cspViolations: string[] = [];
  let currentSocket: PlaywrightWebSocket | undefined;
  let offlineStartedAt = 0;
  let offlineFinishedAt = 0;
  page.on("request", (request) => {
    if (request.url().includes("/assets/app/")) appModuleRequests.push(request.url());
    if (request.url().includes("/assets/client-store/")) legacyStoreRequests.push(request.url());
  });
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push({ text: message.text(), observedAt: Date.now() });
  });
  page.on("pageerror", (error) => pageErrors.push(error.message));
  page.on("websocket", (socket) => { currentSocket = socket; });
  await page.exposeBinding("__sproytRecordCspViolation", (_, violation: string) => cspViolations.push(violation));
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    const sockets: globalThis.WebSocket[] = [];
    class TrackingWebSocket extends NativeWebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        if (protocols === undefined) super(url);
        else super(url, protocols);
        sockets.push(this);
      }
    }
    window.WebSocket = TrackingWebSocket;
    window.__sproytE2eSockets = sockets;
    window.addEventListener("securitypolicyviolation", (event) => {
      void window.__sproytRecordCspViolation(`${event.violatedDirective}: ${event.blockedURI}`);
    });
  });

  let sessionRequests = 0;
  let refreshRequests = 0;
  await page.route(/\/auth\/session(?:\?.*)?$/, async (route) => {
    sessionRequests += 1;
    await route.fulfill({ json: { refresh_after_seconds: refreshRequests === 0 ? 1 : 300 } });
  });
  await page.route(/\/auth\/refresh(?:\?.*)?$/, async (route) => {
    refreshRequests += 1;
    await route.fulfill({ json: { refresh_after_seconds: 300 } });
  });

  const response = await page.goto("/?participant=playwright-smoke", { waitUntil: "domcontentloaded" });
  expect(response).not.toBeNull();
  expect(response?.headers()["content-security-policy"]).toMatch(/script-src 'self' 'nonce-/);

  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  await expect(page.locator("#body")).toBeEnabled();
  await expect.poll(() => appModuleRequests.length).toBe(1);
  expect(appModuleRequests[0]).toMatch(/\/assets\/app\/[a-f0-9]{7,64}\/app\.js$/);
  expect(legacyStoreRequests).toEqual([]);
  await page.locator("#body").fill("draft overlever kontrollert øktfornying");
  const socketBeforeSessionRefresh = currentSocket;
  expect(socketBeforeSessionRefresh).toBeDefined();
  await expect.poll(() => refreshRequests, { timeout: 10_000 }).toBe(1);
  await expect.poll(() => socketBeforeSessionRefresh?.isClosed(), { timeout: 15_000 }).toBe(true);
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  await expect.poll(() => currentSocket !== socketBeforeSessionRefresh && !currentSocket?.isClosed()).toBe(true);
  expect(sessionRequests).toBeGreaterThanOrEqual(2);
  await expect(page.locator("#body")).toHaveValue("draft overlever kontrollert øktfornying");
  await expect(page.locator("#body")).toBeFocused();

  const socketBeforeOffline = currentSocket;
  expect(socketBeforeOffline).toBeDefined();
  offlineStartedAt = Date.now();
  await page.context().setOffline(true);
  await page.evaluate(() => window.__sproytE2eSockets.at(-1)?.close(4002, "e2e offline"));
  await expect.poll(() => socketBeforeOffline?.isClosed(), { timeout: 10_000 }).toBe(true);
  await page.context().setOffline(false);
  await page.evaluate(() => window.dispatchEvent(new Event("online")));
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  await expect.poll(() => currentSocket !== socketBeforeOffline && !currentSocket?.isClosed(), {
    timeout: 15_000
  }).toBe(true);
  offlineFinishedAt = Date.now();
  await expect(page.locator("#body")).toHaveValue("draft overlever kontrollert øktfornying");

  const message = `playwright smoke ${crypto.randomUUID()}`;
  await page.locator("#body").fill(message);
  await page.locator("#send").click();
  await expect(page.locator("#messages")).toContainText(message, { timeout: 15_000 });
  await page.reload();
  await expect(page.locator("#messages")).toContainText(message, { timeout: 15_000 });
  const unexpectedConsoleErrors = consoleErrors.filter(({ text, observedAt }) => {
    const expectedOfflineTransportFailure = observedAt >= offlineStartedAt
      && observedAt <= offlineFinishedAt
      && /WebSocket|ERR_INTERNET_DISCONNECTED|network/i.test(text);
    return !expectedOfflineTransportFailure;
  });
  expect(cspViolations).toEqual([]);
  expect(pageErrors).toEqual([]);
  expect(unexpectedConsoleErrors).toEqual([]);
});

test("conversation-first navigation collapses on desktop and behaves as a modal drawer on mobile", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/?participant=playwright-conversation-layout", { waitUntil: "domcontentloaded" });
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });

  const sidebar = page.locator("#sidebar-panel");
  const drawer = page.locator("#conversation-drawer");
  const drawerSearch = page.locator("#conversation-search");
  const headerToggle = page.locator("#conversation-drawer-header-toggle");
  await expect(sidebar).toHaveClass(/desktop-collapsed/);
  await expect(drawer).toBeVisible();
  await expect(page.locator(".bottom-navigation")).toBeHidden();

  await page.locator("#conversation-drawer-toggle").click();
  await expect(drawer).toBeHidden();
  await expect(headerToggle).toBeFocused();
  await headerToggle.click();
  await expect(drawer).toBeVisible();
  await expect(drawerSearch).toBeFocused();

  await page.setViewportSize({ width: 375, height: 667 });
  await expect(headerToggle).toBeHidden();
  await expect(page.locator(".conversation-header #channel-people")).toBeHidden();
  const mobileShortcut = page.locator("#mobile-conversations-shortcut");
  await expect(mobileShortcut).toBeVisible();
  await expect(page.locator("#mobile-people-shortcut")).toBeVisible();
  await mobileShortcut.click();
  await expect(drawer).toHaveAttribute("role", "dialog");
  await expect(drawer).toHaveAttribute("aria-modal", "true");
  await expect(drawerSearch).toBeFocused();
  await expect(page.locator(".conversation-header")).toHaveAttribute("inert", "");
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
  await expect(mobileShortcut).toBeFocused();
});

test("a dropped send survives a real IndexedDB reload with one request id and clears on receipt", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    class JournalProbeWebSocket extends NativeWebSocket {
      send(data: string): void {
        let command: { type?: unknown; request_id?: unknown } | null = null;
        try { command = JSON.parse(data) as { type?: unknown; request_id?: unknown }; } catch { /* non-json is not our protocol */ }
        if (command?.type === "send_message" && typeof command.request_id === "string") {
          const sent = JSON.parse(sessionStorage.getItem("e2e-durable-sends") ?? "[]") as string[];
          sent.push(command.request_id);
          sessionStorage.setItem("e2e-durable-sends", JSON.stringify(sent));
          if (sessionStorage.getItem("e2e-durable-drop") !== "done") {
            sessionStorage.setItem("e2e-durable-drop", "done");
            return;
          }
        }
        super.send(data);
      }
    }
    window.WebSocket = JournalProbeWebSocket;
  });
  await page.goto("/?participant=playwright-durable-journal", { waitUntil: "domcontentloaded" });
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  const message = `journal reload ${crypto.randomUUID()}`;
  await page.locator("#body").fill(message);
  await page.locator("#send").click();
  await expect.poll(() => page.evaluate(async () => new Promise<number>((resolve, reject) => {
    const request = indexedDB.open("sproyt-durable-outbox", 1);
    request.onerror = () => reject(request.error);
    request.onsuccess = () => {
      const transaction = request.result.transaction("pending-sends", "readonly");
      const rows = transaction.objectStore("pending-sends").getAll();
      rows.onsuccess = () => resolve(rows.result.length);
      rows.onerror = () => reject(rows.error);
    };
  })), { timeout: 10_000 }).toBe(1);
  await page.reload({ waitUntil: "domcontentloaded" });
  await expect(page.locator("#messages")).toContainText(message, { timeout: 15_000 });
  await expect.poll(() => page.evaluate(() => JSON.parse(sessionStorage.getItem("e2e-durable-sends") ?? "[]") as string[])).toHaveLength(2);
  const requestIds = await page.evaluate(() => JSON.parse(sessionStorage.getItem("e2e-durable-sends") ?? "[]") as string[]);
  expect(new Set(requestIds).size).toBe(1);
  await expect.poll(() => page.evaluate(async () => new Promise<number>((resolve, reject) => {
    const request = indexedDB.open("sproyt-durable-outbox", 1);
    request.onerror = () => reject(request.error);
    request.onsuccess = () => {
      const transaction = request.result.transaction("pending-sends", "readonly");
      const rows = transaction.objectStore("pending-sends").getAll();
      rows.onsuccess = () => resolve(rows.result.length);
      rows.onerror = () => reject(rows.error);
    };
  })), { timeout: 10_000 }).toBe(0);
  await expect(page.locator("#body")).toBeEditable();
});

test("the channel member browser opens a direct conversation without showing yourself", async ({ page }) => {
  const peer = await page.context().newPage();
  await peer.goto("/?participant=playwright-dm-peer", { waitUntil: "domcontentloaded" });
  await expect(peer.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });

  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    const commands: string[] = [];
    class RecordingWebSocket extends NativeWebSocket {
      send(data: string): void {
        commands.push(data);
        super.send(data);
      }
    }
    window.WebSocket = RecordingWebSocket;
    window.__sproytDmCommands = commands;
  });
  await page.goto("/?participant=playwright-dm-actor", { waitUntil: "domcontentloaded" });
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  await page.locator("#channel-people").click();
  const memberList = page.locator("#channel-member-list");
  const directAction = memberList.getByRole("button", { name: "Start direktesamtale med playwright-dm-peer" });
  await expect(directAction).toBeVisible({ timeout: 15_000 });
  await expect(memberList).not.toContainText("playwright-dm-actor");
  const peerId = await directAction.locator("xpath=..").getAttribute("data-profile-user-id");
  expect(peerId).not.toBeNull();
  await directAction.press("Enter");
  await expect.poll(() => page.evaluate(() => {
    for (const serialized of window.__sproytDmCommands) {
      const command: unknown = JSON.parse(serialized);
      if (typeof command !== "object" || command === null || !("type" in command) || command.type !== "open_direct_channel") continue;
      if (!("payload" in command) || typeof command.payload !== "object" || command.payload === null || !("user_id" in command.payload)) continue;
      return typeof command.payload.user_id === "string" ? command.payload.user_id : null;
    }
    return null;
  })).toBe(peerId);
  await expect(page.locator("#channel-details-dialog")).not.toBeVisible({ timeout: 15_000 });
  await expect(page.locator("#conversation-circle")).toHaveText("Direktemelding");
  await expect(page.locator("#conversation-title")).toContainText("playwright-dm-peer");
  await peer.close();
});
