import { expect, test } from "@playwright/test";

for (const width of [320, 390, 1500]) {
  const compact = width < 500;
  test.describe(`${width}px`, () => {
  test.use({ hasTouch: compact, isMobile: compact });
  test("thread actions and replies are discoverable", async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`/?participant=thread-discovery-${width}&ui=react`);
    const app = page.locator("#sproyt-react-preview");
    const channel = app.locator(".sp-channel-pane");
    const composer = channel.getByRole("textbox", { name: "Skriv melding" });
    await expect(composer).toBeEnabled({ timeout: 15_000 });
    if (compact) {
      const name = app.locator(".sp-mobile-conversation-short-name");
      await expect(name).toBeVisible();
      expect((await name.boundingBox())!.width).toBeGreaterThanOrEqual(25);
      const directory = app.getByRole("button", { name: /Trådar i samtalen/ });
      const bounds = await directory.boundingBox();
      expect(bounds!.width).toBeGreaterThanOrEqual(44);
      expect(bounds!.height).toBeGreaterThanOrEqual(44);
    }
    const text = `Ei melding som får tråd ${Date.now()}`;
    await composer.fill(text); await channel.getByRole("button", { name: "Send ↑", exact: true }).click();
    const root = channel.locator("[data-message-id]").filter({ hasText: text });
    await expect(root).toBeVisible();
    const icon = root.getByRole("button", { name: "Svar i tråd", exact: true });
    await expect(icon.locator("svg")).toBeVisible();
    await expect(root.getByRole("button", { name: "Opne tråd", exact: true })).toHaveCount(0);
    await composer.fill("Utkast i kanalen");
    await root.getByRole("button", { name: "Fleire meldingsval", exact: true }).click();
    await app.getByRole("dialog", { name: "Meldingsval" }).getByRole("button", { name: "Start tråd", exact: true }).click();
    const thread = app.locator(".sp-thread-pane");
    const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
    await expect(reply).toBeEnabled();
    const answer = `Eit svar ${Date.now()}`;
    await reply.fill(answer); await thread.getByRole("button", { name: "Send ↑", exact: true }).click();
    await expect(thread.getByText(answer, { exact: true })).toBeVisible();
    await thread.getByRole("button", { name: "Lukk tråden" }).click();
    await expect(composer).toHaveValue("Utkast i kanalen");
    const threadLink = root.locator(".sp-message-body").getByRole("button", { name: /^Opne tråd/ });
    await expect(threadLink).toBeVisible();
    await expect(threadLink).toContainText("1 svar");
    if (compact) {
      expect(await page.evaluate(() => matchMedia("(pointer: coarse)").matches)).toBe(true);
      const bounds = await threadLink.boundingBox();
      expect(bounds!.height).toBeGreaterThanOrEqual(44);
    }
    await threadLink.focus(); await threadLink.press("Enter");
    await expect(thread.getByText(answer, { exact: true })).toBeVisible();
    await thread.getByRole("button", { name: "Lukk tråden" }).click();
    await expect(threadLink).toBeFocused();
    await expect(composer).toHaveValue("Utkast i kanalen");
    if (width === 320) {
      await app.getByRole("button", { name: "Samtalar", exact: true }).click();
      const history = app.locator(".sp-product-nav-footer .sp-view-history-in-menu");
      const back = history.getByRole("button", { name: "Tilbake", exact: true });
      await expect(back).toBeVisible();
      await expect(back).toBeEnabled();
      const before = page.url();
      await back.click();
      await expect.poll(() => page.url()).not.toBe(before);
    }
  });
  });
}
