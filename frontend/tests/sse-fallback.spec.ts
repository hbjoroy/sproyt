import { expect, test } from "@playwright/test";

test("forced SSE loads, sends and receives live messages", async ({ page, context }) => {
  const participant = `sse-${crypto.randomUUID()}`;
  const streamRequests: string[] = [];
  const commandRequests: string[] = [];
  page.on("request", request => {
    if (request.url().includes("/api/v1/events")) streamRequests.push(request.url());
    if (request.url().includes("/api/v1/commands")) commandRequests.push(request.url());
  });
  await page.goto(`/?participant=${participant}&transport=sse&ui=legacy`);
  await expect(page.locator("#status")).toHaveText(/Tilkopla via reserve/, { timeout: 20_000 });
  await expect(page.locator("#body")).toBeEnabled();
  const message = `SSE ${crypto.randomUUID()}`;
  await page.locator("#body").fill(message);
  await page.locator("#send").click();
  await expect(page.locator("#messages")).toContainText(message, { timeout: 15_000 });
  expect(streamRequests.length).toBeGreaterThanOrEqual(2);
  expect(commandRequests.length).toBeGreaterThanOrEqual(6);

  const sender = await context.newPage();
  await sender.goto(`/?participant=sender-${crypto.randomUUID()}&ui=legacy`);
  await expect(sender.locator("#body")).toBeEnabled();
  const liveMessage = `Live to SSE ${crypto.randomUUID()}`;
  await sender.locator("#body").fill(liveMessage);
  await sender.locator("#send").click();
  await expect(page.locator("#messages")).toContainText(liveMessage, { timeout: 20_000 });
  await sender.close();
});

test("forced SSE switches channels without leaving the previous stream active", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 800 });
  const streamRequests: string[] = [];
  page.on("request", request => {
    if (request.url().includes("/api/v1/events")) streamRequests.push(request.url());
  });
  await page.goto(`/?participant=sse-switch-${crypto.randomUUID()}&transport=sse&ui=legacy`);
  await expect(page.locator("#status")).toHaveText(/Tilkopla via reserve/, { timeout: 20_000 });

  const circleName = `SSE ${crypto.randomUUID().slice(0, 8)}`;
  await page.locator("#create-circle-from-drawer").click();
  await page.locator("#create-circle-name").fill(circleName);
  await page.locator("#create-circle-from-dialog").click();
  await expect(page.locator("#onboarding-notice")).toContainText("klar", { timeout: 15_000 });
  await expect(page.locator("#create-circle-dialog")).toBeHidden({ timeout: 15_000 });
  await page.getByLabel(`Val for ${circleName}`).click();
  await page.getByRole("button", { name: "Ny kanal" }).click();
  const channelName = `Plan ${crypto.randomUUID().slice(0, 6)}`;
  await page.locator("#managed-channel-name").fill(channelName);
  await page.locator("#circle-channel-create").getByRole("button", { name: "Lag kanal" }).click();
  await expect(page.locator("#onboarding-notice")).toContainText(channelName, { timeout: 15_000 });

  const channel = page.locator(".conversation-select").filter({ hasText: channelName });
  await channel.click();
  await expect(page.locator("#body")).toBeEnabled();
  const channelMessage = `Circle only ${crypto.randomUUID()}`;
  await page.locator("#body").fill(channelMessage);
  await page.locator("#send").click();
  await expect(page.locator("#messages")).toContainText(channelMessage, { timeout: 15_000 });

  await page.locator(".conversation-select").filter({ hasText: "general" }).click();
  await expect(page.locator("#body")).toBeEnabled();
  await expect(page.locator("#messages")).not.toContainText(channelMessage);
  expect(new Set(streamRequests.map(url => new URL(url).searchParams.get("channel_id")).filter(Boolean)).size).toBeGreaterThanOrEqual(2);
});

test("blocked WebSocket changes to SSE and restores the composer", async ({ page }) => {
  await page.addInitScript(() => {
    class BlockedWebSocket extends EventTarget {
      static readonly CONNECTING = 0;
      static readonly OPEN = 1;
      static readonly CLOSING = 2;
      static readonly CLOSED = 3;
      readyState = 0;
      constructor() {
        super();
        window.setTimeout(() => { this.dispatchEvent(new Event("error")); this.close(); }, 0);
      }
      send(): void { throw new Error("blocked"); }
      close(): void {
        if (this.readyState === 3) return;
        this.readyState = 3;
        this.dispatchEvent(new CloseEvent("close", { code: 1006, reason: "blocked" }));
      }
    }
    window.WebSocket = BlockedWebSocket as unknown as typeof WebSocket;
  });
  await page.goto(`/?participant=blocked-${crypto.randomUUID()}&ui=legacy`);
  await expect(page.locator("#status")).toHaveText(/Tilkopla via reserve/, { timeout: 20_000 });
  await expect(page.locator("#body")).toBeEnabled();
});

test("a silent probe returns from SSE to WebSocket when it becomes available", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    let attempts = 0;
    class BlockedWebSocket extends EventTarget {
      readyState = 0;
      constructor() {
        super();
        window.setTimeout(() => { this.dispatchEvent(new Event("error")); this.close(); }, 0);
      }
      send(): void { throw new Error("blocked"); }
      close(): void {
        if (this.readyState === 3) return;
        this.readyState = 3;
        this.dispatchEvent(new CloseEvent("close", { code: 1006, reason: "blocked" }));
      }
    }
    const RecoveringWebSocket = function (url: string | URL, protocols?: string | string[]) {
      if (attempts++ < 2) return new BlockedWebSocket();
      return protocols === undefined ? new NativeWebSocket(url) : new NativeWebSocket(url, protocols);
    };
    Object.assign(RecoveringWebSocket, { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 });
    window.WebSocket = RecoveringWebSocket as unknown as typeof WebSocket;
  });
  await page.goto(`/?participant=probe-${crypto.randomUUID()}&ui=legacy`);
  await expect(page.locator("#status")).toHaveText(/Tilkopla via reserve/, { timeout: 20_000 });
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(page.locator("#status")).toHaveText(/^Tilkopla(?! via reserve)/, { timeout: 20_000 });
  await expect(page.locator("#body")).toBeEnabled();
});

test("rapid channel changes do not block a later WebSocket handoff", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 800 });
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    let attempts = 0;
    class BlockedWebSocket extends EventTarget {
      readyState = 0;
      constructor() {
        super();
        window.setTimeout(() => { this.dispatchEvent(new Event("error")); this.close(); }, 0);
      }
      send(): void { throw new Error("blocked"); }
      close(): void {
        if (this.readyState === 3) return;
        this.readyState = 3;
        this.dispatchEvent(new CloseEvent("close", { code: 1006, reason: "blocked" }));
      }
    }
    const RecoveringWebSocket = function (url: string | URL, protocols?: string | string[]) {
      if (attempts++ < 2) return new BlockedWebSocket();
      return protocols === undefined ? new NativeWebSocket(url) : new NativeWebSocket(url, protocols);
    };
    Object.assign(RecoveringWebSocket, { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 });
    window.WebSocket = RecoveringWebSocket as unknown as typeof WebSocket;
  });
  await page.goto(`/?participant=rapid-switch-${crypto.randomUUID()}&ui=legacy`);
  await expect(page.locator("#status")).toHaveText(/Tilkopla via reserve/, { timeout: 20_000 });
  const circleName = `Rapid ${crypto.randomUUID().slice(0, 8)}`;
  await page.locator("#create-circle-from-drawer").click();
  await page.locator("#create-circle-name").fill(circleName);
  await page.locator("#create-circle-from-dialog").click();
  await expect(page.locator("#onboarding-notice")).toContainText("klar", { timeout: 15_000 });
  await expect(page.locator("#create-circle-dialog")).toBeHidden({ timeout: 15_000 });
  await page.getByLabel(`Val for ${circleName}`).click();
  await page.getByRole("button", { name: "Ny kanal" }).click();
  const channelName = `Rapid ${crypto.randomUUID().slice(0, 6)}`;
  await page.locator("#managed-channel-name").fill(channelName);
  await page.locator("#circle-channel-create").getByRole("button", { name: "Lag kanal" }).click();
  await expect(page.locator("#onboarding-notice")).toContainText(channelName, { timeout: 15_000 });
  await page.locator(".conversation-select").filter({ hasText: channelName }).click();
  await page.locator(".conversation-select").filter({ hasText: "general" }).click();
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(page.locator("#status")).toHaveText(/^Tilkopla(?! via reserve)/, { timeout: 20_000 });
  await expect(page.locator("#body")).toBeEnabled();
});
