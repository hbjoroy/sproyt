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
    const timestamp = sent.locator("time");
    await expect(timestamp).toHaveAttribute("aria-label", /^Sendt .+ Trykk for å vise eller skjule tidspunktet\.$/);
    await expect(timestamp).toHaveAttribute("title", /\d{4}/);
    const timestampTooltip = sent.getByRole("tooltip");
    await timestamp.tap();
    await expect(timestamp).toHaveAttribute("aria-expanded", "true");
    await expect(timestampTooltip).toBeVisible();
    await composer.tap();
    await expect(timestamp).toHaveAttribute("aria-expanded", "false");
    await expect(timestampTooltip).toBeHidden();
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
    await parent.getByRole("button", { name: "Fleire meldingsval" }).click();
    await expect(parent.getByRole("button", { name: "Rediger", exact: true })).toBeVisible();
    await expect(parent.getByRole("button", { name: "Slett", exact: true })).toBeVisible();
    await parent.getByRole("button", { name: "Lukk meldingsvala" }).click();
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
  const tools = channel.getByRole("toolbar", { name: "Skriveverktøy" });
  await expect(tools).toBeHidden();
  await channel.getByRole("button", { name: "Skriveverktøy", exact: true }).click();
  await expect(tools).toBeVisible();
  await expect(tools.getByRole("button")).toHaveCount(4);
  expect(await tools.evaluate(element => element.scrollWidth)).toBeLessThanOrEqual(await tools.evaluate(element => element.clientWidth));
  await expect(channel.getByRole("button", { name: "Biletegenerering", exact: true })).toBeInViewport({ ratio: 1 });
  await channel.getByRole("button", { name: "Biletegenerering", exact: true }).click();
  await expect(channel.getByRole("region", { name: "Private biletmeldingar" })).toBeVisible();
  await context.close();
});

test("synthetic visual viewport coordinates position the composer", async ({ browser, baseURL }) => {
  const context = await browser.newContext({
    baseURL, serviceWorkers: "block", viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true
  });
  const page = await context.newPage();
  await page.addInitScript(() => {
    const viewport = new EventTarget();
    Object.assign(viewport, {
      height: 844, width: 390, offsetTop: 0, offsetLeft: 0,
      pageTop: 0, pageLeft: 0, scale: 1, onresize: null, onscroll: null
    });
    Object.defineProperty(window, "visualViewport", { configurable: true, value: viewport });
  });
  await page.goto("/?participant=playwright-android-keyboard", { waitUntil: "domcontentloaded" });
  const app = page.locator("#sproyt-react-preview");
  const composer = app.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  await composer.focus();

  const keyboardViewport = { height: 411.4, offsetTop: 37.2 };
  await page.evaluate(({ height, offsetTop }) => {
    const viewport = window.visualViewport!;
    Object.assign(viewport, { height, offsetTop, pageTop: offsetTop });
    viewport.dispatchEvent(new Event("resize"));
    viewport.dispatchEvent(new Event("scroll"));
  }, keyboardViewport);

  await expect.poll(async () => app.evaluate((element, viewport) => {
    const bounds = element.getBoundingClientRect();
    return Math.max(
      Math.abs(bounds.top - viewport.offsetTop),
      Math.abs(bounds.height - viewport.height),
      Math.abs(bounds.bottom - viewport.offsetTop - viewport.height)
    );
  }, keyboardViewport)).toBeLessThan(.05);
  expect(await app.evaluate(element => element.style.bottom)).toBe("auto");
  const composerBounds = await composer.boundingBox();
  expect(composerBounds).not.toBeNull();
  expect(composerBounds!.y + composerBounds!.height)
    .toBeLessThanOrEqual(keyboardViewport.offsetTop + keyboardViewport.height);
  expect(await page.locator("html").evaluate(element => ({
    height: element.style.getPropertyValue("--app-height"),
    offsetTop: element.style.getPropertyValue("--app-offset-top")
  }))).toEqual({ height: "411.4px", offsetTop: "37.2px" });
  await context.close();
});

test("opt-in viewport diagnostics report geometry without changing layout or exposing drafts", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/?participant=viewport-diagnostics");
  const input = page.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled();
  await expect(page.locator("#sproyt-viewport-diagnostics")).toHaveCount(0);
  const original = await input.boundingBox();
  await page.goto("/?participant=viewport-diagnostics&viewport-debug=1");
  await expect(input).toBeEnabled();
  expect(await input.boundingBox()).toEqual(original);
  await input.fill("Private draft not for diagnostics");
  const panel = page.locator("#sproyt-viewport-diagnostics");
  await expect(panel).toContainText("focus=true");
  await expect(panel).toContainText("outline extent=");
  await expect(panel).not.toContainText("Private draft");
  await expect(input).toBeFocused();
  await expect(panel).toHaveCSS("pointer-events", "none");
});

test("channel overflow keeps notification and confirmed leave actions together", async ({ page }) => {
  await page.goto("/?participant=preview-channel-overflow&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  await preview.getByRole("button", { name: "Kanalval" }).click();
  const menu = preview.getByRole("dialog", { name: /Kanalval:/ });
  await expect(menu.getByRole("button", { name: /Varsel (på|av)/ })).toBeVisible();
  await menu.getByRole("button", { name: "Forlat kanalen", exact: true }).click();
  await expect(menu.getByRole("group", { name: "Stadfest at du vil forlate kanalen" })).toBeVisible();
  await menu.getByRole("button", { name: "Avbryt" }).click();
  await expect(menu.getByRole("button", { name: "Forlat kanalen", exact: true })).toBeVisible();
});
