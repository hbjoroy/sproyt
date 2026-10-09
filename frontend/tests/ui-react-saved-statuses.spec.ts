import { expect, test } from "@playwright/test";

test("saved statuses survive reload, prefill without publishing, and can be forgotten independently", async ({ page }) => {
  const participant = `saved-status-${Date.now()}`;
  let saves = 0;
  page.on("websocket", socket => socket.on("framesent", frame => {
    try { if (JSON.parse(String(frame.payload)).type === "set_status") saves++; } catch {}
  }));
  const open = async () => {
    const preview = page.locator("#sproyt-react-preview");
    await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15000 });
    await preview.getByRole("button", { name: "Meny", exact: true }).click();
    await preview.getByRole("button", { name: "Meny og innstillingar", exact: true }).click();
    await preview.getByRole("button", { name: "Profil og status", exact: true }).click();
    return preview.getByRole("dialog", { name: "Profil og status" });
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
