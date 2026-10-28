import { destinationSurface } from "./support/destination-surface";
import { expect, test, type Locator } from "@playwright/test";

async function returnFromProfile(preview: Locator) {
  await destinationSurface(preview, "Profil og status", true)
    .getByRole("button", { name: "Tilbake til menyen", exact: true }).click();
  const settings = destinationSurface(preview, "Innstillingar", true);
  await expect(settings).toBeVisible();
  await settings.getByRole("button", { name: "Lukk innstillingane", exact: true }).click();
  await expect(settings).toHaveCount(0);
  await expect(preview.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeVisible();
}

test("first-50 badge is visible on the profile and compact beside chat authors", async ({ page }) => {
  await page.goto("/?participant=preview-early-adopter&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15000 });
  await expect(preview.locator(".sp-header-early-adopter")).toHaveCount(0);

  await preview.getByRole("button", { name: "Innstillingar", exact: true }).click();
  await preview.getByRole("button", { name: "Profil og status", exact: true }).click();
  await expect(destinationSurface(preview, "Profil og status", false)).toContainText("Første 50 på Sprøyt");
  await returnFromProfile(preview);

  await composer.fill("Merket skal følgje meldinga");
  await composer.press("Enter");
  const message = preview.locator("[data-message-id]").filter({ hasText: "Merket skal følgje meldinga" }).last();
  const badge = message.getByRole("button", { name: "Blant dei første 50 på Sprøyt", exact: true });
  await expect(message.locator(".sp-message-meta strong")).toHaveText("Du");
  await badge.hover();
  await expect(message.getByRole("tooltip", { name: "Blant dei første 50 på Sprøyt" })).toBeVisible();
});

for (const touch of [false, true]) {
  test.describe(touch ? "touch byline" : "desktop byline", () => {
    test.use(touch ? { hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } } : {});
    test("own status edits independently of the first-50 badge and preserves the draft", async ({ page }) => {
      const errors: string[] = [];
      page.on("pageerror", error => errors.push(error.message));
      await page.goto(`/?participant=status-click-${touch ? "touch" : "desktop"}&ui=react`);
      const preview = page.locator("#sproyt-react-preview");
      const composer = preview.getByRole("textbox", { name: "Skriv melding" });
      await expect(composer).toBeEnabled({ timeout: 15000 });

      await preview.getByRole("button", { name: "Innstillingar", exact: true }).click();
      await preview.getByRole("button", { name: "Profil og status", exact: true }).click();
      const dialog = destinationSurface(preview, "Profil og status", false);
      await dialog.getByLabel("Statusemoji", { exact: true }).fill("☀️");
      await dialog.getByLabel("Statusmelding", { exact: true }).fill("Ute i sola");
      await dialog.getByRole("button", { name: "Lagre status", exact: true }).click();
      await expect(dialog).toContainText("Statusen er lagra.");
      await returnFromProfile(preview);
      const body = `uavhengige statusmål ${touch ? "touch" : "desktop"}`;
      await composer.fill(body);
      if (touch) await preview.getByRole("button", { name: /^Send / }).click(); else await composer.press("Enter");
      const message = preview.locator("[data-message-id]").filter({ hasText: body }).last();
      const badge = message.getByRole("button", { name: "Blant dei første 50 på Sprøyt", exact: true });
      const status = message.getByRole("button", { name: "Endre status: ☀️ Ute i sola", exact: true });
      await expect(status).toBeVisible();
      await composer.fill("utkast før statusendring");
      if (touch) {
        for (const target of [badge, status]) {
          const box = await target.boundingBox();
          expect(box?.width).toBeGreaterThanOrEqual(24); expect(box?.height).toBeGreaterThanOrEqual(24);
        }
        await badge.tap();
      } else await badge.click();
      await expect(message).toHaveAttribute("data-early-adopter-open", "true");
      await expect(dialog).not.toBeVisible();
      if (touch) await status.tap(); else await status.click();
      await expect(dialog).toBeVisible();
      await expect(dialog.getByLabel("Statusmelding", { exact: true })).toBeFocused();
      await expect(dialog.getByLabel("Statusmelding", { exact: true })).toHaveValue("Ute i sola");
      await expect(message).not.toHaveAttribute("data-early-adopter-open", "true");
      await dialog.getByLabel("Statusmelding", { exact: true }).fill("Tilbake inne");
      await dialog.getByRole("button", { name: "Lagre status", exact: true }).click();
      await expect(dialog).toContainText("Statusen er lagra.");
      await page.keyboard.press("Escape");
      const updated = message.getByRole("button", { name: "Endre status: ☀️ Tilbake inne", exact: true });
      await expect(updated).toBeFocused();
      await expect(composer).toHaveValue("utkast før statusendring");
      if (!touch) {
        await updated.press("Enter"); await expect(dialog).toBeVisible(); await page.keyboard.press("Escape");
      }
      expect(errors).toEqual([]);
    });
  });
}
