import { expect, test } from "@playwright/test";

test("editorial theme is scoped to the application and persists an explicit choice", async ({ page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.goto("/?participant=playwright-editorial-theme", { waitUntil: "domcontentloaded" });
  await expect(page.locator("#status")).toHaveText(/Tilkopla/, { timeout: 15_000 });

  const app = page.locator("#sproyt-app");
  const toggle = page.locator("#theme-mode-toggle");
  await expect(app).toHaveClass(/sp-theme/);
  await expect(app).toHaveAttribute("data-accent", "citron");
  await expect(app).toHaveAttribute("data-theme", "system");
  await expect.poll(() => page.locator("#sproyt-editorial-design-system").evaluate((style) => style.textContent?.includes("--sp-canvas"))).toBe(true);

  const afterLight = await toggle.evaluate((button) => {
    (button as HTMLButtonElement).click();
    return [document.querySelector("#sproyt-app")?.getAttribute("data-theme"), button.textContent, localStorage.getItem("sproyt.theme.v1")];
  });
  expect(pageErrors).toEqual([]);
  expect(afterLight).toEqual(["light", "Tema: light", "light"]);
  await page.reload({ waitUntil: "domcontentloaded" });
  await expect(app).toHaveAttribute("data-theme", "light");

  await toggle.evaluate((button) => (button as HTMLButtonElement).click());
  await expect(app).toHaveAttribute("data-theme", "dark");
});
