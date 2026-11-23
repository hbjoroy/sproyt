import { readFile } from "node:fs/promises";
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
  await composer.press("Control+Enter");
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
      if (touch) await preview.getByRole("button", { name: /^Send / }).click(); else await composer.press("Control+Enter");
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

test("Kretsrom keeps status beside the name and actions compact", async ({ page }) => {
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url));
  await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  await page.addInitScript(() => localStorage.setItem("sproyt.design.kretsrom.v1", "on"));
  await page.goto(`/?participant=kr-status-${Date.now()}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", {name:"Skriv melding"});
  await expect(composer).toBeEnabled();
  await preview.getByRole("button", { name: "Innstillingar", exact: true }).click();
  await preview.getByRole("button", { name: "Profil og status", exact: true }).click();
  const dialog = destinationSurface(preview, "Profil og status", false);
  await dialog.getByLabel("Statusemoji", { exact: true }).fill("☀️");
  await dialog.getByLabel("Statusmelding", { exact: true }).fill("Ute i sola");
  await dialog.getByRole("button", { name: "Lagre status", exact: true }).click();
  await expect(dialog).toContainText("Statusen er lagra.");
  await returnFromProfile(preview);
  const body = `Status i Kretsrom ${Date.now()}`;
  await composer.fill(body);
  await composer.press("Control+Enter");
  const message = preview.locator("[data-message-id]").filter({ hasText: body }).last();
  const status = message.getByRole("button", {name: /^Vis status:/});
  await expect(status.locator(".kr-status-label")).toHaveText("Ute i sola");
  const label = status.locator(".kr-status-label");
  expect(await label.evaluate(el => el.clientWidth >= el.scrollWidth && el.clientWidth > 0)).toBe(true);
  const byline = message.locator(".kr-byline");
  const name = (await byline.locator("strong").boundingBox())!;
  const emoji = (await status.boundingBox())!;
  expect(emoji.x).toBeGreaterThan(name.x);
  expect(emoji.y).toBeLessThan(name.y + name.height);
  const coarse = await page.evaluate(() => matchMedia("(pointer: coarse)").matches);
  expect((await message.locator(".kr-footer").boundingBox())!.height).toBe(await message.locator(".kr-mobile-menu-trigger").isVisible() ? 0 : coarse ? 32 : 26);
  await status.click();
  const detail = message.locator(".kr-status-detail");
  await expect(detail).toBeVisible();
  await expect(detail).toContainText("Ute i sola");
  await detail.getByRole("button", {name:"Endre status"}).click();
  await expect(page.getByRole("dialog", {name:"Profil og status"})).toBeVisible();
});
