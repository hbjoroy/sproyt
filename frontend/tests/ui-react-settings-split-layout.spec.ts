import { expect, test } from "@playwright/test";

for (const compact of [false, true]) {
  test(`settings categories and content use ${compact ? "sequential" : "split"} navigation`, async ({ page }) => {
    await page.setViewportSize({ width: compact ? 390 : 1800, height: 1000 });
    await page.goto(`/?participant=settings-split-${compact}&ui=react`);
    const preview = page.locator("#sproyt-react-preview");
    const composer = preview.getByRole("textbox", { name: "Skriv melding" });
    await expect(composer).toBeEnabled({ timeout: 15_000 });
    await composer.fill("behald samtaleutkastet");
    await preview.getByRole("button", { name: "Innstillingar", exact: true }).click();
    const navigation = preview.getByRole("navigation", { name: "Innstillingskategoriar" });
    await expect(navigation).toBeVisible();
    await navigation.getByRole("button", { name: "Utsjånad og bruk", exact: true }).click();
    const content = preview.locator(".sp-workspace-page-body");
    await expect(content).toBeVisible();
    if (compact) {
      await expect(navigation).toBeHidden();
      await preview.locator(".sp-workspace-page-header").getByRole("button").click();
      await expect(navigation).toBeVisible();
    } else {
      await expect(navigation).toBeVisible();
      await expect(navigation.getByRole("button", { name: "Utsjånad og bruk", exact: true })).toHaveAttribute("aria-current", "page");
      const categoryBounds = await navigation.boundingBox();
      const contentBounds = await content.boundingBox();
      expect(categoryBounds!.x + categoryBounds!.width).toBeLessThan(contentBounds!.x);
      await navigation.getByRole("button", { name: "Varslingsinnstillingar", exact: true }).click();
      await expect(content.getByLabel("Varslingsmodus", { exact: true })).toBeVisible();
      await page.goBack();
      await expect(preview.locator(".sp-workspace-page-header").getByRole("heading", { name: "Innstillingar", exact: true })).toBeVisible();
    }
    await page.keyboard.press("Escape");
    await expect(composer).toHaveValue("behald samtaleutkastet");
  });
}
test("circle settings retain their category while channel details use their own workspace", async ({ page }) => {
  await page.setViewportSize({ width: 1800, height: 1000 });
  const participant = `settings-category-${Date.now()}`;
  await page.goto(`/?participant=${participant}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  await preview.getByRole("button", { name: "Innstillingar", exact: true }).click();
  let navigation = preview.getByRole("navigation", { name: "Innstillingskategoriar" });
  await navigation.getByRole("searchbox", { name: "Søk i innstillingar" }).fill("Ny vennekrets");
  await navigation.getByRole("button", { name: "Ny vennekrets", exact: true }).click();
  await preview.getByRole("textbox", { name: "Namn på vennekrets" }).fill("Kategorikrets");
  await preview.getByRole("button", { name: "Opprett vennekrets", exact: true }).click();
  await expect(preview.getByRole("textbox", { name: "Namn på vennekrets" })).toHaveCount(0);
  await page.goto(`/?participant=${participant}&ui=react`);
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  await preview.getByRole("button", { name: "Innstillingar", exact: true }).click();
  navigation = preview.getByRole("navigation", { name: "Innstillingskategoriar" });
  const administration = navigation.getByRole("button", { name: "Administrer krets", exact: true });
  await administration.click();
  await expect(administration).toHaveAttribute("aria-current", "page");
  await preview.locator(".sp-workspace-page-body").getByRole("button", { name: "Kategorikrets", exact: true }).click();
  await expect(administration).toHaveAttribute("aria-current", "page");
  await preview.locator(".sp-workspace-page-body").getByRole("button", { name: "Medlemmer og roller", exact: true }).click();
  await expect(administration).toHaveAttribute("aria-current", "page");
  await navigation.getByRole("searchbox", { name: "Søk i innstillingar" }).fill("Kanaldetaljar");
  await navigation.getByRole("button", { name: "Kanaldetaljar, medlemmer og integrasjonar", exact: true }).click();
  await expect(preview.locator("[data-workspace-page]")).toBeVisible();
  await expect(preview.getByRole("navigation", { name: "Innstillingskategoriar" })).toHaveCount(0);
});