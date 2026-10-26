import { expect, test } from "@playwright/test";

test("saved statuses survive reload, prefill without publishing, and can be forgotten independently", async ({ page }) => {
  const participant = `saved-status-${Date.now()}`;
  let saves = 0;
  page.on("websocket", socket => socket.on("framesent", frame => {
    try { if (JSON.parse(String(frame.payload)).type === "set_status") saves++; } catch {}
  }));
  const open = async () => {
    const preview = page.locator("#sproyt-react-preview");
    const profile = preview.getByRole("dialog", { name: "Profil og status", exact: true });
    if (new URL(page.url()).searchParams.get("section") === "profile") {
      await expect(profile).toBeVisible();
      await expect(preview.getByRole("dialog")).toHaveCount(1);
      return profile;
    }
    await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15000 });
    const settings = preview.getByRole("button", { name: "Innstillingar", exact: true });
    if (!await settings.isVisible()) await preview.getByRole("button", { name: "Meny", exact: true }).click();
    await settings.click();
    await preview.getByRole("button", { name: "Profil og status", exact: true }).click();
    return profile;
  };
  await page.goto(`/?participant=${participant}&ui=react`);
  let dialog = await open();
  const save = async (text: string) => {
    await dialog.getByLabel("Statusemoji", { exact: true }).fill("☀️");
    await dialog.getByLabel("Statusmelding", { exact: true }).fill(text);
    await dialog.getByRole("button", { name: "Lagre status", exact: true }).click();
    await expect(dialog).toContainText("Statusen er lagra.");
    await expect(dialog.getByRole("button", { name: `Bruk status: ☀️ ${text}`, exact: true, includeHidden: true })).toBeAttached();
  };
  await save("På tur"); await save("Heime");
  await page.reload(); dialog = await open();
  await dialog.locator("summary").filter({ hasText: "Tidlegare statusar" }).click();
  const before = saves;
  await dialog.getByRole("button", { name: "Bruk status: ☀️ På tur", exact: true }).click();
  await expect(dialog.getByLabel("Statusmelding", { exact: true })).toHaveValue("På tur");
  expect(saves).toBe(before);
  await save("På tur");
  await expect(dialog.locator(".sp-saved-statuses li").first()).toContainText("På tur");
  await dialog.getByRole("button", { name: "Gløym status: ☀️ På tur", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Bruk status: ☀️ På tur", exact: true })).toHaveCount(0);
  await page.reload(); dialog = await open();
  await expect(dialog.getByLabel("Statusmelding", { exact: true })).toHaveValue("På tur");
  await dialog.locator("summary").filter({ hasText: "Tidlegare statusar" }).click();
  await expect(dialog.getByRole("button", { name: "Bruk status: ☀️ Heime", exact: true })).toBeVisible();
  await dialog.getByRole("button", { name: "Tøm status", exact: true }).click();
  await expect(dialog).toContainText("Statusen er tømd.");
  await expect(dialog.getByRole("button", { name: "Bruk status: ☀️ Heime", exact: true })).toBeVisible();
});
