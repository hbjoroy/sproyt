import { expect, test } from "@playwright/test";

test("React preview shows delivery status for an accepted own message", async ({ page }) => {
  await page.goto("/?participant=playwright-react-delivery&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  const body = `leveringsstatus ${Date.now()}`;
  await composer.fill(body);
  await composer.press("Enter");
  const message = preview.locator("[data-message-id]").filter({ hasText: body });
  await expect(message).toBeVisible();
  await expect(message).toContainText("Sendt");
});

test("React reauthentication action persists channel and thread drafts before login", async ({ page }) => {
  let sockets = 0;
  page.on("websocket", () => { sockets += 1; });
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    window.WebSocket = class extends NativeWebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols);
        (window as typeof window & { __sproytTestSocket?: WebSocket }).__sproytTestSocket = this;
      }
    };
  });
  await page.goto("/?participant=playwright-react-reauth&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const channelComposer = preview.locator(".sp-channel-pane").getByRole("textbox", { name: "Skriv melding" });
  await expect(channelComposer).toBeEnabled({ timeout: 15_000 });

  const rootBody = `reauth rot ${Date.now()}`;
  await channelComposer.fill(rootBody);
  await channelComposer.press("Enter");
  const root = preview.locator(".sp-channel-pane [data-message-id]").filter({ hasText: rootBody });
  await expect(root).toBeVisible();
  const rootId = await root.getAttribute("data-message-id");
  expect(rootId).toBeTruthy();
  await root.getByRole("button", { name: "Svar i tråd" }).click();

  const channelDraft = "kanalutkast før ny innlogging";
  const threadDraft = "trådutkast før ny innlogging";
  await channelComposer.fill(channelDraft);
  const threadComposer = preview.locator(".sp-thread-pane").getByRole("textbox", { name: "Svar i tråden" });
  await threadComposer.fill(threadDraft);

  let loginRequests = 0;
  await page.route(/\/auth\/session(?:\?.*)?$/, route => route.fulfill({ status: 401 }));
  await page.route(/\/auth\/refresh(?:\?.*)?$/, route => route.fulfill({ status: 401 }));
  await page.route(/\/auth\/login(?:\?.*)?$/, async route => {
    loginRequests += 1;
    await route.fulfill({ contentType: "text/html", body: "reauth" });
  });
  await page.evaluate(() => {
    const socket = (window as typeof window & { __sproytTestSocket?: WebSocket }).__sproytTestSocket;
    if (!socket) throw new Error("test socket was not captured");
    socket.dispatchEvent(new CloseEvent("close", { code: 1008, reason: "authentication required", wasClean: false }));
  });

  const reauthenticate = preview.getByRole("button", { name: "Logg inn på nytt" });
  await expect(reauthenticate).toBeVisible({ timeout: 15_000 });
  await expect(preview).toContainText("Utkasta dine blir lagra først");
  expect(sockets).toBe(1);
  await reauthenticate.click();
  await expect.poll(() => loginRequests).toBe(1);

  const drafts = await page.evaluate((expectedRootId) => {
    const channelId = localStorage.getItem("sproyt.active-channel.v1");
    return {
      channel: channelId ? localStorage.getItem(`sproyt.channel-draft.v1.${channelId}`) : null,
      thread: channelId && expectedRootId ? localStorage.getItem(`sproyt.thread-draft.v1.${channelId}.${expectedRootId}`) : null
    };
  }, rootId);
  expect(drafts).toEqual({ channel: channelDraft, thread: threadDraft });
});
