import { expect, test, type Page } from "@playwright/test";

declare global {
  interface Window {
    __sproytBoundarySockets: WebSocket[];
  }
}

async function trackSockets(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    const sockets: WebSocket[] = [];
    class TrackingWebSocket extends NativeWebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        if (protocols === undefined) super(url);
        else super(url, protocols);
        sockets.push(this);
      }
    }
    window.WebSocket = TrackingWebSocket;
    window.__sproytBoundarySockets = sockets;
  });
}

test("malformed WebSocket envelopes are ignored without a page error or visible state change", async ({ page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await trackSockets(page);
  await page.route(/\/auth\/session(?:\?.*)?$/, (route) => route.fulfill({ json: { refresh_after_seconds: 300 } }));

  await page.goto("/?participant=boundary-envelope", { waitUntil: "domcontentloaded" });
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  await page.waitForTimeout(250);
  const messagesBefore = await page.locator("#messages").innerText();
  await page.evaluate(() => {
    const socket = window.__sproytBoundarySockets.at(-1);
    if (!socket) throw new Error("expected connected socket");
    const malformed = [
      '{"protocol":"sproyt.chat.v1","type":"not_an_event","payload":{}}',
      '{"protocol":"sproyt.chat.v1","type":"chat","request_id":7,"payload":{}}',
      '{"protocol":"sproyt.chat.v1","type":"chat","payload":[]}',
      '{"protocol":"sproyt.chat.v1","type":"chat"}'
    ];
    for (const data of malformed) socket.dispatchEvent(new MessageEvent("message", { data }));
    socket.dispatchEvent(new MessageEvent("message", { data: new Blob(["{}"], { type: "application/json" }) }));
  });
  await page.waitForTimeout(100);
  expect(await page.locator("#messages").innerText()).toBe(messagesBefore);
  expect(pageErrors).toEqual([]);
});

test("unsupported WebSocket protocol is reported visibly", async ({ page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await trackSockets(page);
  await page.route(/\/auth\/session(?:\?.*)?$/, (route) => route.fulfill({ json: { refresh_after_seconds: 300 } }));

  await page.goto("/?participant=boundary-protocol", { waitUntil: "domcontentloaded" });
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  await page.evaluate(() => {
    const socket = window.__sproytBoundarySockets.at(-1);
    if (!socket) throw new Error("expected connected socket");
    socket.dispatchEvent(new MessageEvent("message", { data: '{"protocol":"sproyt.chat.v0","type":"chat","payload":{}}' }));
  });
  await expect(page.locator("#messages")).toContainText("Serveren svarte med ein ukjend protokoll.");
  expect(pageErrors).toEqual([]);
});

test("malformed navigation storage does not crash while session refresh proceeds", async ({ page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.addInitScript(() => {
    localStorage.setItem("sproyt.active-channel.v1", "stored-channel");
    localStorage.setItem("sproyt.active-circle.v1", "stored-circle");
    localStorage.setItem("sproyt.active-channel-by-circle.v1", "{not-json");
    localStorage.setItem("sproyt.session-refresh-lease.v1", "{not-json");
    Object.defineProperty(Navigator.prototype, "locks", { configurable: true, get: () => undefined });
  });
  let refreshRequests = 0;
  await page.route(/\/auth\/session(?:\?.*)?$/, async (route) => {
    await route.fulfill({ json: { refresh_after_seconds: 1 } });
  });
  await page.route(/\/auth\/refresh(?:\?.*)?$/, async (route) => {
    refreshRequests += 1;
    await route.fulfill({ json: { refresh_after_seconds: 300 } });
  });

  await page.goto("/?participant=boundary-storage", { waitUntil: "domcontentloaded" });
  await expect.poll(() => page.evaluate(() => navigator.locks)).toBeUndefined();
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  // A short refresh proves that concurrent session checks cannot suppress the timer.
  await expect.poll(() => refreshRequests, { timeout: 10_000 }).toBeGreaterThan(0);
  await expect.poll(() => page.evaluate(() => window.localStorage.getItem("sproyt.session-refresh-lease.v1"))).toBeNull();
  expect(pageErrors).toEqual([]);
});

test("malformed initial session JSON does not crash the client", async ({ page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.route(/\/auth\/session(?:\?.*)?$/, (route) => route.fulfill({
    status: 200,
    contentType: "application/json",
    body: "{not-json"
  }));

  await page.goto("/?participant=boundary-api", { waitUntil: "domcontentloaded" });
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });
  await page.waitForTimeout(100);
  expect(pageErrors).toEqual([]);
});

test("explicit reauthentication preserves the active draft and supports keyboard activation", async ({ page }) => {
  let loginRequests = 0;
  await page.route(/\/auth\/login(?:\?.*)?$/, async (route) => { loginRequests += 1; await route.fulfill({ contentType: "text/html", body: "reauth" }); });
  await page.goto("/?participant=boundary-reauth", { waitUntil: "domcontentloaded" });
  const composer = page.locator("#body");
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  await composer.fill("utkast som må bevarast");
  await page.locator("#reauthenticate-now").evaluate((button) => { if (button instanceof HTMLButtonElement) button.hidden = false; });
  const statusToggle = page.locator("#connection-status-toggle");
  await statusToggle.focus();
  await statusToggle.press("Enter");
  await expect(page.locator("#reauthenticate-now")).toBeVisible();
  await page.locator("#reauthenticate-now").click();
  await expect.poll(() => loginRequests).toBe(1);
  const storedDraft = await page.evaluate(() => { const channelId = localStorage.getItem("sproyt.active-channel.v1"); return channelId ? localStorage.getItem(`sproyt.channel-draft.v1.${channelId}`) : null; });
  expect(storedDraft).toBe("utkast som må bevarast");
});

test("malformed successful media response stays inside the upload error boundary", async ({ page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.route(/\/api\/v1\/channels\/[^/]+\/media(?:\?.*)?$/, (route) => route.fulfill({ status: 200, contentType: "application/json", body: "{not-json" }));
  await page.goto("/?participant=boundary-upload", { waitUntil: "domcontentloaded" });
  await expect(page.locator("#body")).toBeEnabled({ timeout: 15_000 });
  await page.locator("#media-input").setInputFiles({ name: "test.png", mimeType: "image/png", buffer: Buffer.from([0x89, 0x50, 0x4e, 0x47]) });
  await expect(page.locator("#upload-status")).toContainText("ugyldige mediedata");
  expect(pageErrors).toEqual([]);
});
