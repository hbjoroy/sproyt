import { expect, type Locator } from "@playwright/test";

/** Compact channel actions live in the existing conversation disclosure. */
export async function openChannelMenu(app: Locator): Promise<Locator> {
  const title = app.locator(".sp-mobile-conversation-title");
  if (await title.isVisible()) {
    if (await title.getAttribute("aria-expanded") !== "true") await title.click();
    const menu = app.locator(".sp-mobile-conversation-name .sp-channel-actions-inline");
    await expect(menu).toBeVisible();
    return menu;
  }
  const trigger = app.getByRole("button", { name: "Kanalval", exact: true });
  await trigger.click();
  const menu = app.getByRole("group", { name: /^Kanalval:/ });
  await expect(menu).toBeVisible();
  return menu;
}
