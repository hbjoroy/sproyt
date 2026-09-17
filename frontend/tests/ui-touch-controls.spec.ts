import { expect, test } from "@playwright/test";

test("touch controls remain usable after closing dialogs and reloading", async ({ browser, baseURL }) => {
  const context = await browser.newContext({ baseURL, serviceWorkers: "block", viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  const page = await context.newPage();
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/?participant=touch-controls");
  const app = page.locator("#sproyt-react-preview");
  const input = app.getByRole("textbox", { name: "Skriv melding" });
  for (let pass = 0; pass < 2; pass++) {
    await expect(input).toBeEnabled();
    await input.tap();
    await expect(input).toBeFocused();
    await input.fill("Draft survives dialog");
    await app.getByRole("button", { name: "Meny", exact: true }).tap();
    await app.getByRole("button", { name: "Meny og innstillingar", exact: true }).tap();
    const dialog = app.getByRole("dialog", { name: "Meny og innstillingar" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Lukk menyen" }).tap();
    await expect(dialog).toBeHidden();
    await input.tap();
    await expect(input).toBeFocused();
    await expect(input).toHaveValue("Draft survives dialog");
    expect(await input.evaluate(element => {
      const rect = element.getBoundingClientRect();
      return document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2) === element;
    })).toBe(true);
    await input.fill("");
    if (pass === 0) await page.reload();
  }
  expect(errors).toEqual([]);
  await context.close();
});
