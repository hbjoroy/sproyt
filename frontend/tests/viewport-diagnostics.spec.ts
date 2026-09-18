import { expect, test } from "@playwright/test";

test("viewport diagnostics are compact, geometry-only, and do not take the composer hit target", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 378 });
  await page.goto("/?participant=viewport-diagnostic-probe", { waitUntil: "domcontentloaded" });
  const input = page.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled();
  const before = await input.boundingBox();
  const originalScrollGeometry = await page.evaluate(() => ({
    document: [document.documentElement.clientHeight, document.documentElement.scrollHeight],
    body: [document.body.clientHeight, document.body.scrollHeight]
  }));
  await page.goto("/?participant=viewport-diagnostic-probe&viewport-debug=1", { waitUntil: "domcontentloaded" });
  await expect(input).toBeEnabled();
  await input.fill("Private draft not for diagnostics");
  const panel = page.locator("#sproyt-viewport-diagnostics");
  await expect(panel).toContainText("Viewport diagnostic v2 (CSS px)");
  await expect(panel).toContainText("dvh=");
  await expect(panel).toContainText("safe T/B=");
  await expect(panel).toContainText("doc C/S=");
  await expect(panel).toContainText("disabled=false inert=false dlg=0 hit=textarea");
  await expect(panel).not.toContainText("Private draft");
  await expect(panel).toHaveCSS("pointer-events", "none");
  await input.click();
  await expect(panel).toContainText("pointer=1:textarea");
  const after = await input.boundingBox();
  expect(after).toEqual(before);
  expect(await page.evaluate(() => ({
    document: [document.documentElement.clientHeight, document.documentElement.scrollHeight],
    body: [document.body.clientHeight, document.body.scrollHeight]
  }))).toEqual(originalScrollGeometry);
  expect(await input.evaluate(element => {
    const rect = element.getBoundingClientRect();
    return document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2) === element;
  })).toBe(true);
  const panelBox = await panel.boundingBox();
  expect(panelBox).not.toBeNull();
  expect(panelBox!.height).toBeLessThan(280);
  expect(panelBox!.y + panelBox!.height).toBeLessThanOrEqual(after!.y);
  expect(await panel.evaluate(element => element.scrollHeight <= element.clientHeight)).toBe(true);
});
