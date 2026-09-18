import { expect, test } from "@playwright/test";

test("first-50 badge is visible on the profile and compact beside chat authors", async ({ page }) => {
  await page.goto("/?participant=preview-early-adopter&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15000 });
  await expect(preview.getByRole("img", { name: "Blant dei første 50 på Sprøyt" })).toBeVisible();

  await preview.getByRole("button", { name: "Meny", exact: true }).click();
  await preview.getByRole("button", { name: "Meny og innstillingar", exact: true }).click();
  await preview.getByRole("button", { name: "Profil og status", exact: true }).click();
  await expect(preview.getByRole("dialog", { name: "Profil og status" })).toContainText("Første 50 på Sprøyt");
  await page.keyboard.press("Escape"); await page.keyboard.press("Escape");

  await composer.fill("Merket skal følgje meldinga");
  await composer.press("Enter");
  const message = preview.locator("[data-message-id]").filter({ hasText: "Merket skal følgje meldinga" }).last();
  const author = message.locator(".sp-message-meta strong");
  await expect(author).toContainText("✨");
  await author.hover();
  await expect(message.getByRole("tooltip", { name: "Blant dei første 50 på Sprøyt" })).toBeVisible();
});
