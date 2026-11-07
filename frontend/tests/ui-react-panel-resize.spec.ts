import { expect, test, type Locator, type Page } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { createChannelViaUi } from "./support/channel-setup";

test.use({ viewport: { width: 1500, height: 900 }, isMobile: false });

async function openConversation(page: Page) {
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url), "utf8");
  await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  await page.goto(`/?participant=panel-resize-${Date.now()}&ui=react`);
  const app = page.locator("#sproyt-react-preview");
  const composer = app.locator(".sp-channel-pane").getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  return { app, composer, sidebar: app.getByRole("separator", { name: "Breidd på sidepanelet" }),
    threadDivider: app.getByRole("separator", { name: "Breidd på trådpanelet" }) };
}

async function openThread(page: Page, view: Awaited<ReturnType<typeof openConversation>>) {
  const text = `Melding for justerbar tråd ${Date.now()}`;
  await view.composer.fill(text); await view.composer.press("Control+Enter");
  const message = view.app.locator(".sp-channel-pane [data-message-id]").filter({ hasText: text });
  await message.getByRole("button", { name: "Svar i tråd", exact: true }).click();
  const reply = view.app.locator(".sp-thread-pane").getByRole("textbox", { name: "Svar i tråden" });
  await expect(reply).toBeEnabled();
  return { reply, message };
}

async function drag(page: Page, divider: Locator, dx: number, offset = 0) {
  const box = (await divider.boundingBox())!;
  const x = box.x + box.width / 2 + offset;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y); await page.mouse.down();
  await page.mouse.move(x + dx, y, { steps: 8 }); await page.mouse.up();
}

test("thin sidebar divider has a generous hit area, bounded dragging, keyboard control and remembered width", async ({ page }) => {
  const { app, composer, sidebar } = await openConversation(page);
  const nav = app.locator(".sp-sidebar");
  await composer.fill("Utkastet skal bli ståande");
  await expect(sidebar).toBeVisible();
  await expect(sidebar).toHaveCSS("cursor", "col-resize");
  expect(await sidebar.evaluate(element => getComputedStyle(element, "::after").width)).toBe("1px");
  expect((await sidebar.boundingBox())!.width).toBeGreaterThanOrEqual(14);
  const before = (await nav.boundingBox())!.width;
  await drag(page, sidebar, 80, 5);
  await expect.poll(async () => (await nav.boundingBox())!.width).toBeCloseTo(before + 80, 0);
  await expect(composer).toHaveValue("Utkastet skal bli ståande");
  await expect(composer).toBeFocused();
  await drag(page, sidebar, 300);
  await expect(sidebar).toHaveAttribute("aria-valuenow", "420");
  await drag(page, sidebar, -300);
  await expect(sidebar).toHaveAttribute("aria-valuenow", "240");
  await sidebar.focus(); await sidebar.press("End");
  await expect(sidebar).toHaveAttribute("aria-valuenow", "420");
  await sidebar.press("ArrowRight");
  await expect(sidebar).toHaveAttribute("aria-valuenow", "420");
  await sidebar.press("Home");
  await expect(sidebar).toHaveAttribute("aria-valuenow", "240");
  await sidebar.press("ArrowLeft");
  await expect(sidebar).toHaveAttribute("aria-valuenow", "240");
  await sidebar.press("Shift+ArrowRight");
  await expect(sidebar).toHaveAttribute("aria-valuenow", "280");
  await page.reload();
  await expect(sidebar).toHaveAttribute("aria-valuenow", "280");
  await expect(composer).toHaveValue("Utkastet skal bli ståande");
  await sidebar.dblclick();
  await expect(sidebar).toHaveAttribute("aria-valuenow", "260");
  expect(await app.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
});

test("thread and sidebar widths share available space without losing drafts or closing the thread", async ({ page }) => {
  const view = await openConversation(page);
  const { reply, message } = await openThread(page, view);
  await view.composer.fill("Kanalutkast"); await reply.fill("Trådutkast");
  await expect(view.threadDivider).toBeVisible();
  const thread = view.app.locator(".sp-thread-pane");
  const before = (await thread.boundingBox())!.width;
  await drag(page, view.threadDivider, -100, 5);
  await expect.poll(async () => (await thread.boundingBox())!.width).toBeCloseTo(before + 100, 0);
  await expect(reply).toHaveValue("Trådutkast");
  await expect(view.composer).toHaveValue("Kanalutkast");
  await view.threadDivider.focus(); await view.threadDivider.press("End");
  expect((await view.app.locator(".sp-channel-pane").boundingBox())!.width).toBeGreaterThanOrEqual(440);
  await view.sidebar.focus(); await view.sidebar.press("End");
  await expect(view.composer).toBeVisible();
  await expect(reply).toBeVisible();
  expect((await thread.boundingBox())!.width).toBeGreaterThanOrEqual(340);
  expect((await view.app.locator(".sp-channel-pane").boundingBox())!.width).toBeGreaterThanOrEqual(440);
  const desired = await view.threadDivider.getAttribute("aria-valuenow");
  await thread.getByRole("button", { name: "Lukk tråden", exact: true }).click();
  await expect(view.threadDivider).toHaveCount(0);
  await message.getByRole("button", { name: "Svar i tråd", exact: true }).click();
  await expect(view.threadDivider).toHaveAttribute("aria-valuenow", desired!);
  await expect(reply).toHaveValue("Trådutkast");
  await expect(view.composer).toHaveValue("Kanalutkast");
});

test("thread resizing follows the current discussion after a channel boundary replaces its DOM", async ({ page }) => {
  const view = await openConversation(page);
  await openThread(page, view);
  await expect(view.threadDivider).toBeVisible();
  const previous = await view.app.locator(".sp-discussion").elementHandle();
  await view.sidebar.focus(); await view.sidebar.press("Shift+ArrowRight");
  const sidebarWidth = await view.sidebar.getAttribute("aria-valuenow");
  await createChannelViaUi(page, `Ny justerbar kanal ${Date.now()}`);
  expect(await previous!.evaluate(element => element.isConnected)).toBe(false);
  await openThread(page, view);
  await expect(view.threadDivider).toBeVisible();
  await expect(view.sidebar).toHaveAttribute("aria-valuenow", sidebarWidth!);
  expect(await view.threadDivider.evaluate(element => element.isConnected && element.parentElement?.classList.contains("sp-discussion"))).toBe(true);
  const thread = view.app.locator(".sp-thread-pane");
  const before = (await thread.boundingBox())!.width;
  await drag(page, view.threadDivider, -40);
  await expect.poll(async () => (await thread.boundingBox())!.width).toBeCloseTo(before + 40, 0);
});

test("responsive transitions clamp panel sizes and restore the desktop preference", async ({ page }) => {
  const view = await openConversation(page);
  const { reply } = await openThread(page, view);
  await reply.fill("Bevar trådutkastet ved smal vising");
  await view.sidebar.focus(); await view.sidebar.press("End");
  await view.threadDivider.focus(); await view.threadDivider.press("End");
  await page.setViewportSize({ width: 1100, height: 900 });
  await expect(view.sidebar).toHaveAttribute("aria-valuenow", "320");
  await expect(view.threadDivider).toHaveAttribute("aria-disabled", "true");
  await expect(view.threadDivider).toHaveAttribute("tabindex", "-1");
  await expect(view.composer).toBeVisible();
  await expect(reply).toBeVisible();
  await page.setViewportSize({ width: 900, height: 900 });
  await expect(view.sidebar).toHaveCount(0);
  await expect(view.threadDivider).toHaveCount(0);
  await expect(view.app.locator(".sp-sidebar")).toBeHidden();
  await expect(reply).toHaveValue("Bevar trådutkastet ved smal vising");
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(reply).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.setViewportSize({ width: 1500, height: 900 });
  await expect(view.sidebar).toHaveAttribute("aria-valuenow", "420");
  await expect(view.threadDivider).toBeVisible();
  await expect(reply).toHaveValue("Bevar trådutkastet ved smal vising");
});

test("cancelled drag and missing local storage keep resize usable", async ({ page }) => {
  await page.addInitScript(() => {
    const get = Storage.prototype.getItem;
    const set = Storage.prototype.setItem;
    Storage.prototype.getItem = function (key) { if (key === "sproyt.panel-widths.v1") throw new Error("Storage blocked"); return get.call(this, key); };
    Storage.prototype.setItem = function (key, value) { if (key === "sproyt.panel-widths.v1") throw new Error("Storage blocked"); return set.call(this, key, value); };
  });
  const { app, sidebar, composer } = await openConversation(page);
  await composer.fill("Bevar dette utkastet");
  await sidebar.focus(); await sidebar.press("ArrowRight");
  await expect(sidebar).toHaveAttribute("aria-valuenow", "276");
  const box = (await sidebar.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + 200); await page.mouse.down();
  await page.mouse.move(box.x + 100, box.y + 200);
  await expect(app.locator(".sp-workspace")).toHaveAttribute("data-panel-dragging", "sidebar");
  await sidebar.press("Escape"); await page.mouse.up();
  await expect(sidebar).toHaveAttribute("aria-valuenow", "276");
  await expect(app.locator(".sp-workspace")).not.toHaveAttribute("data-panel-dragging");
  await expect(composer).toHaveValue("Bevar dette utkastet");
  await sidebar.press("End");
  await expect(sidebar).toHaveAttribute("aria-valuenow", "420");
  await sidebar.evaluate(element => element.addEventListener("pointerdown", event => {
    element.setAttribute("data-test-pointer", String((event as PointerEvent).pointerId));
  }));
  const capture = (await sidebar.boundingBox())!;
  await page.mouse.move(capture.x + capture.width / 2, capture.y + 200); await page.mouse.down();
  await page.mouse.move(capture.x - 60, capture.y + 200);
  await sidebar.evaluate(element => element.releasePointerCapture(Number(element.getAttribute("data-test-pointer"))));
  await page.mouse.move(capture.x - 80, capture.y + 200);
  await expect(app.locator(".sp-workspace")).not.toHaveAttribute("data-panel-dragging");
  const stopped = await sidebar.getAttribute("aria-valuenow");
  await page.mouse.move(capture.x - 160, capture.y + 200); await page.mouse.up();
  await expect(sidebar).toHaveAttribute("aria-valuenow", stopped!);
});

for (const panel of ["sidebar", "thread"] as const) {
  test(`Escape cancels ${panel} dragging from a focused composer without closing its thread`, async ({ page }) => {
    const view = await openConversation(page);
    const { reply } = await openThread(page, view);
    const field = panel === "sidebar" ? view.composer : reply;
    const divider = panel === "sidebar" ? view.sidebar : view.threadDivider;
    await field.fill("Fokus og utkast skal bli bevarte");
    const before = await divider.getAttribute("aria-valuenow");
    const box = (await divider.boundingBox())!;
    const x = box.x + box.width / 2;
    const y = box.y + box.height / 2;
    await page.mouse.move(x, y); await page.mouse.down();
    await page.mouse.move(x + (panel === "sidebar" ? 60 : -60), y, { steps: 4 });
    await expect(field).toBeFocused();
    await expect(view.app.locator(".sp-workspace")).toHaveAttribute("data-panel-dragging", panel);
    await page.keyboard.press("Escape"); await page.mouse.up();
    await expect(view.app.locator(".sp-workspace")).not.toHaveAttribute("data-panel-dragging");
    await expect(divider).toHaveAttribute("aria-valuenow", before!);
    await expect(reply).toBeVisible();
    await expect(field).toHaveValue("Fokus og utkast skal bli bevarte");
    await expect(field).toBeFocused();
  });
}

test("light and dark layout keep a one-pixel divider and readable content", async ({ page }, testInfo) => {
  const view = await openConversation(page);
  await openThread(page, view);
  await view.sidebar.focus(); await view.sidebar.press("Shift+ArrowRight");
  await view.threadDivider.focus(); await view.threadDivider.press("Shift+ArrowLeft");
  for (const colorScheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme });
    expect(await view.threadDivider.evaluate(element => getComputedStyle(element, "::after").width)).toBe("1px");
    await view.composer.focus();
    await page.screenshot({ path: testInfo.outputPath(`resized-panels-${colorScheme}.png`) });
    expect(await view.app.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  }
});
