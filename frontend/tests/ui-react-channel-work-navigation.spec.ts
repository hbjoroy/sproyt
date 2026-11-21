import { expect, test } from "@playwright/test";

test("channel and personal work use the right scope and preserve the channel draft", async ({ page }, { project }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/?participant=work-navigation-" + project.name);
  const app = page.locator("#sproyt-react-preview");
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(composer).toBeEnabled();
  await composer.fill("Eit utkast som skal liggje her etter fanebytet");
  const tasks = () => page.waitForResponse(response =>
    new URL(response.url()).pathname === "/api/v1/me/process-tasks" && response.ok());
  const channelRead = tasks();
  const work = app.getByRole("tab", { name: "Arbeid", exact: true });
  await work.click();
  expect(new URL((await channelRead).url()).searchParams.get("channel_id")).toBeTruthy();
  await expect(app.getByText("Du har ingen opne oppgåver i denne kanalen.")).toBeVisible();
  await expect(app.getByRole("heading", { name: "Prosessar", exact: true })).toBeVisible();
  await expect(composer).toBeHidden();
  await work.focus();
  await page.keyboard.press("Home");
  await expect(composer).toHaveValue("Eit utkast som skal liggje her etter fanebytet");

  await app.getByRole("button", { name: "Området mitt", exact: true }).click();
  const personalRead = tasks();
  await app.getByRole("tab", { name: "Mine oppgåver", exact: true }).click();
  expect(new URL((await personalRead).url()).searchParams.has("channel_id")).toBe(false);
  await expect(app.getByText("Du har ingen opne oppgåver.", { exact: true })).toBeVisible();
  await expect(app.getByRole("heading", { name: "Prosessar", exact: true })).toBeHidden();
  await page.setViewportSize({ width: 390, height: 844 });
  const selected = app.getByRole("tab", { name: "Mine oppgåver", exact: true });
  await expect(selected).toHaveAttribute("aria-selected", "true");
  await selected.focus();
  await page.keyboard.press("Home");
  await expect(app.getByRole("tab", { name: "Chatlogg", exact: true })).toBeFocused();
  await expect(composer).toBeVisible();
});
