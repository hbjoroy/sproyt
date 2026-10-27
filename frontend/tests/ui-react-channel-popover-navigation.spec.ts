import { expect, test } from "@playwright/test";

for (const compact of [false, true]) {
  test(`channel popover dismisses and opens channel settings ${compact ? "on mobile" : "on desktop"}`, async ({ page }) => {
    await page.setViewportSize({ width: compact ? 390 : 1600, height: 1000 });
    await page.goto(`/?participant=channel-popover-${compact}&ui=react`);
    const preview = page.locator("#sproyt-react-preview");
    const composer = preview.getByRole("textbox", { name: "Skriv melding" });
    await expect(composer).toBeEnabled({ timeout: 15_000 });
    await composer.fill("kanalmenyutkast");
    const trigger = preview.getByRole("button", { name: "Kanalval", exact: true });
    const menu = preview.getByRole("group", { name: /^Kanalval:/ });
    await trigger.click();
    await expect(menu).toBeVisible();
    await expect(menu).toHaveAttribute("popover", "auto");
    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();
    await expect(trigger).toBeFocused();
    await expect(composer).toHaveValue("kanalmenyutkast");
    await trigger.click();
    await page.setViewportSize({ width: compact ? 400 : 1580, height: 980 });
    await expect(menu).toBeHidden();
    await trigger.click();
    await menu.getByRole("button", { name: "Kanaldetaljar", exact: true }).click();
    await expect(menu).toBeHidden();
    await expect(preview.locator("[data-workspace-page]")).toBeVisible();
    await expect(preview.getByRole("navigation", { name: "Innstillingskategoriar" })).toHaveCount(0);
  });
}