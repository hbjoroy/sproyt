import { expect, test, type Locator, type Page } from "@playwright/test";

async function expectUsableConversation(page: Page, pane: Locator, minimumTimeline: number) {
  const viewport = page.viewportSize()!;
  const timeline = await pane.locator(".sp-thread-replies").count()
    ? pane.locator(".sp-thread-replies") : pane.locator(".sp-timeline");
  await expect.poll(async () => (await timeline.boundingBox())?.height ?? 0).toBeGreaterThan(minimumTimeline);
  const input = pane.locator("textarea");
  const send = pane.getByRole("button", { name: "Send ↑", exact: true });
  for (const control of [input, send]) {
    const rect = await control.boundingBox();
    expect(rect).not.toBeNull();
    expect(rect!.x).toBeGreaterThanOrEqual(0);
    expect(rect!.x + rect!.width).toBeLessThanOrEqual(viewport.width + 1);
    expect(rect!.y + rect!.height).toBeLessThanOrEqual(viewport.height + 1);
  }
  expect(await page.locator("#sproyt-react-preview").evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
}

// The short portrait viewport also covers space remaining above a mobile keyboard.
for (const viewport of [{ width: 320, height: 568 }, { width: 390, height: 844 }, { width: 667, height: 375 }, { width: 320, height: 360 }]) {
  test(`mobile conversation reserves space for messages at ${viewport.width}x${viewport.height}`, async ({ browser, baseURL }) => {
    const context = await browser.newContext({ baseURL, serviceWorkers: "block", viewport, hasTouch: true, isMobile: true });
    const page = await context.newPage();
    await page.goto(`/?participant=playwright-mobile-${viewport.width}`, { waitUntil: "domcontentloaded" });
    const app = page.locator("#sproyt-react-preview");
    const channel = app.locator(".sp-channel-pane");
    const composer = channel.getByRole("textbox", { name: "Skriv melding" });
    await expect(composer).toBeEnabled({ timeout: 15_000 });
    await composer.blur();
    await expectUsableConversation(page, channel, viewport.height * .45);
    const message = `Mobilmelding ${viewport.width} ${Date.now()}`;
    await composer.fill(message);
    await expectUsableConversation(page, channel, viewport.height * .3);
    await channel.getByRole("button", { name: "Send ↑", exact: true }).click();
    const sent = channel.locator("[data-message-id]").filter({ hasText: message });
    await expect(sent).toBeVisible();
    await expect(composer).toHaveValue("");
    await composer.fill("Kanalutkast som skal bli verande");
    await channel.getByRole("button", { name: "← Samtalar" }).click();
    const navigation = app.getByRole("navigation", { name: "Samtalar", exact: true });
    await expect(navigation).toBeVisible();
    await navigation.locator("button[aria-current=page]").click();
    await expect(composer).toHaveValue("Kanalutkast som skal bli verande");
    await sent.getByRole("button", { name: "Svar i tråd" }).click();
    const thread = app.locator(".sp-thread-pane");
    await expect(channel).toBeHidden();
    const parent = thread.locator(".sp-thread-parent");
    await expect(parent.locator("[data-thread-trigger]")).toHaveCount(0);
    await expect(parent.getByRole("button", { name: "Legg til reaksjon", exact: true })).toBeVisible();
    await expect(parent.getByRole("button", { name: "Rediger", exact: true })).toBeVisible();
    await expect(parent.getByRole("button", { name: "Slett", exact: true })).toBeVisible();
    const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
    await reply.fill("Trådutkast");
    await expectUsableConversation(page, thread, viewport.height * .25);
    await thread.getByRole("button", { name: "Lukk tråden" }).click();
    await expect(composer).toHaveValue("Kanalutkast som skal bli verande");
    await context.close();
  });
}

test("compact toolbar and writing tools stay accessible without shrinking the conversation", async ({ browser, baseURL }) => {
  const context = await browser.newContext({ baseURL, serviceWorkers: "block", viewport: { width: 320, height: 568 }, hasTouch: true, isMobile: true });
  const page = await context.newPage();
  await page.goto("/?participant=playwright-mobile-controls", { waitUntil: "domcontentloaded" });
  const app = page.locator("#sproyt-react-preview");
  const channel = app.locator(".sp-channel-pane");
  const composer = channel.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  await composer.fill("Bevar mobilutkastet");
  const height = (await channel.locator(".sp-timeline").boundingBox())!.height;
  const menu = app.getByRole("button", { name: "Meny", exact: true });
  await menu.click();
  await expect(menu).toHaveAttribute("aria-expanded", "true");
  await app.getByRole("button", { name: "Byt tema", exact: true }).click();
  await app.getByRole("button", { name: "Meny og innstillingar", exact: true }).click();
  const dialog = app.getByRole("dialog", { name: "Meny og innstillingar" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Lukk menyen" }).click();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveAttribute("aria-expanded", "false");
  await expect(composer).toHaveValue("Bevar mobilutkastet");
  expect((await channel.locator(".sp-timeline").boundingBox())!.height).toBeGreaterThanOrEqual(height - 1);
  await composer.click();
  const tools = channel.locator(".sp-writing-tools-track");
  const more = channel.getByRole("button", { name: "Vis fleire skriveverktøy", exact: true });
  await expect(more).toBeVisible();
  for (let step = 0; step < 5 && await more.isVisible(); step++) {
    const previous = await tools.evaluate(element => element.scrollLeft);
    await more.click();
    await expect.poll(() => tools.evaluate(element => element.scrollLeft)).toBeGreaterThan(previous);
  }
  await expect(channel.getByRole("button", { name: "Vis første skriveverktøy", exact: true })).toBeVisible();
  await expect(channel.getByRole("button", { name: "Biletegenerering", exact: true })).toBeInViewport({ ratio: 1 });
  await channel.getByRole("button", { name: "Biletegenerering", exact: true }).click();
  await expect(channel.getByRole("region", { name: "Private biletmeldingar" })).toBeVisible();
  await context.close();
});
