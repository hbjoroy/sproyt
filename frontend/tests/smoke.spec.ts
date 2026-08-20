import { expect, test, type WebSocket as PlaywrightWebSocket } from "@playwright/test";

declare global {
  interface Window {
    __sproytRecordCspViolation(violation: string): Promise<void>;
    __sproytE2eSockets: globalThis.WebSocket[];
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
    function TrackingWebSocket(url: string | URL, protocols?: string | string[]): globalThis.WebSocket {
      const socket = protocols === undefined
        ? new NativeWebSocket(url)
        : new NativeWebSocket(url, protocols);
      sockets.push(socket);
      return socket;
    }
    TrackingWebSocket.prototype = NativeWebSocket.prototype;
    Object.setPrototypeOf(TrackingWebSocket, NativeWebSocket);
    window.WebSocket = TrackingWebSocket as unknown as typeof WebSocket;
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
