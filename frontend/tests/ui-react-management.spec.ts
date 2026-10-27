import { destinationSurface } from "./support/destination-surface";
import { expect, test, type Locator, type Page } from "@playwright/test";

async function openPreview(page: Page, participant: string) {
  await page.goto(`/?participant=${participant}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  return preview;
}

async function openManagement(preview: Locator) {
  const trigger = preview.getByRole("button", { name: "Innstillingar" });

  await trigger.click();
  return trigger;
}

test("management directory contains focus, cancels with Escape and does not mutate", async ({ page }) => {
  const commands: string[] = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => commands.push(JSON.parse(String(payload)).type)));
  const preview = await openPreview(page, "preview-management-cancel");
  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("utkast medan menyen er open");
  await openManagement(preview);
  const dialog = destinationSurface(preview, "Innstillingar", false);
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Samtalen og utkasta dine blir tekne vare på");
  await dialog.getByRole("searchbox", { name: "Søk i innstillingar" }).fill("bjelle");
  await expect(dialog.getByRole("button", { name: "Varslingsinnstillingar", exact: true })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Profil og status", exact: true })).toHaveCount(0);
  await page.keyboard.press("Tab");
  expect(await dialog.evaluate(element => element.contains(document.activeElement))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  // Navigation closes the toolbar, so focus returns to its visible entry point.
  await expect(preview.getByRole("button", { name: "Innstillingar", exact: true })).toBeFocused();
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("utkast medan menyen er open");
  expect(commands.filter(command => /^(create_|accept_|enable_|start_)/.test(command))).toEqual([]);
});

for (const [label, title] of [
  ["Ny direktemelding", /Ny direktemelding/],
  ["Kanaldetaljar, medlemmer og integrasjonar", /Kanaldetaljar:/],
  ["Administrer krets", /Vel krets/],
] as const) {
  test(`management opens ${label} in React without reconnecting or losing draft`, async ({ page }) => {
    let sockets = 0;
    page.on("websocket", () => sockets++);
    const preview = await openPreview(page, `preview-management-${label}`);
    await preview.getByRole("textbox", { name: "Skriv melding" }).fill("behald administrasjonsutkast");
    await openManagement(preview);
    if (label !== "Administrer krets") await destinationSurface(preview, "Innstillingar", true).getByRole("searchbox", { name: "Søk i innstillingar" }).fill(label);
    await preview.getByRole("button", { name: label, exact: true }).click();
    const dialog = destinationSurface(preview, title, false);
    await expect(dialog).toBeVisible();
    await expect(preview.locator("[data-workspace-page], dialog[open]")).toHaveCount(1);
    await expect(destinationSurface(preview, "Innstillingar", true)).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(preview.getByRole("button", { name: label, exact: true })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("behald administrasjonsutkast");
    expect(new URL(page.url()).searchParams.has("ui")).toBe(true);
    expect(await page.locator("#sproyt-app").evaluate((element: HTMLElement) => element.inert)).toBe(true);
    expect(sockets).toBe(1);
  });
}

test("creation, channel membership and enrollment stay in React and retain real circle scope", async ({ page }) => {
  const participant = `preview-management-circle-${Date.now()}`;
  let preview = await openPreview(page, participant);
  await openManagement(preview);
  await destinationSurface(preview, "Innstillingar", true).getByRole("searchbox", { name: "Søk i innstillingar" }).fill("Ny vennekrets");
  await preview.getByRole("button", { name: "Ny vennekrets", exact: true }).click();
  const creation = destinationSurface(preview, "Ny vennekrets", true);
  await creation.getByRole("textbox", { name: "Namn på vennekrets" }).fill("Krets frå React");
  await creation.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(creation).toHaveCount(0);
  preview = await openPreview(page, participant);
  await openManagement(preview);
  await preview.getByRole("button", { name: "Administrer krets", exact: true }).click();
  await destinationSurface(preview, "Vel krets", true).getByRole("button", { name: "Krets frå React", exact: true }).click();
  const circle = destinationSurface(preview, "Krets frå React", true);
  await circle.getByRole("button", { name: "Kanalar og medlemskap" }).click();
  await expect(destinationSurface(preview, "Kanalar i Krets frå React", false)).toBeVisible();
  preview = await openPreview(page, participant);
  await openManagement(preview);
  await preview.getByRole("button", { name: "Administrer krets", exact: true }).click();
  await preview.getByRole("searchbox", { name: "Finn vennekrets" }).fill("Krets frå React");
  await destinationSurface(preview, "Vel krets", true).getByRole("button", { name: "Krets frå React", exact: true }).click();
  await destinationSurface(preview, "Krets frå React", true)
    .getByRole("button", { name: "Inviter personar og nye brukarar" }).click();
  const invitation = destinationSurface(preview, "Inviter til Krets frå React", false);
  await expect(invitation).toBeVisible();
  await expect(invitation.getByRole("textbox", { name: "E-postadresse" })).toHaveAttribute("type", "email");
});

for (const [label, field] of [
  ["Profil og status", "Visningsnamn"],
  ["Varslingsinnstillingar", "Varslingsmodus"],
] as const) {
  test(`compact management opens ${label} directly in React`, async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    const preview = await openPreview(page, `preview-management-compact-${label}`);
    await preview.getByRole("textbox", { name: "Skriv melding" }).fill("mobilt utkast");

    await preview.getByRole("button", { name: "Innstillingar" }).click();
    await preview.getByRole("button", { name: label, exact: true }).click();
    const dialog = destinationSurface(preview, label, true);
    await expect(dialog.getByLabel(field, { exact: true })).toBeVisible();
    await expect(preview.locator("[data-workspace-page], dialog[open]")).toHaveCount(1);
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
    await expect(preview.getByRole("button", { name: label, exact: true })).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("mobilt utkast");
  });
}

test("task inbox stays in React and preserves the active channel draft", async ({ page }) => {
  const preview = await openPreview(page, "preview-management-tasks");
  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("oppgåveutkast");
  await preview.getByRole("button", { name: /^Innboks,/ }).click();
  await preview.getByRole("button", { name: "Personlege oppgåver", exact: true }).click();
  await expect(destinationSurface(preview, "Innboks", false)).toBeVisible();
  await preview.getByRole("button", { name: "Tilbake til samtalen" }).click();
  await expect(destinationSurface(preview, "Innboks", false)).toBeVisible();
  await preview.getByRole("button", { name: "Tilbake til samtalen" }).click();
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("oppgåveutkast");
});

test("React management preserves both channel and thread drafts", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1000 });
  const preview = await openPreview(page, "preview-management-thread");
  const channelInput = preview.getByRole("textbox", { name: "Skriv melding" });
  const root = `management-tråd ${Date.now()}`;
  await channelInput.fill(root);
  await preview.locator(".sp-channel-pane").getByRole("button", { name: "Send ↑", exact: true }).click();
  await expect(channelInput).toHaveValue("");
  await channelInput.fill("kanalutkast gjennom meny");
  await preview.locator("[data-message-id]").filter({ hasText: root }).getByRole("button", { name: "Svar i tråd" }).click();
  await preview.getByRole("textbox", { name: "Svar i tråden" }).fill("trådutkast gjennom meny");
  await openManagement(preview);
  await destinationSurface(preview, "Innstillingar", true).getByRole("searchbox", { name: "Søk i innstillingar" }).fill("Ny direktemelding");
  await preview.getByRole("button", { name: "Ny direktemelding" }).click();
  await expect(destinationSurface(preview, "Ny direktemelding", false)).toBeVisible();
  await expect(page.locator("#thread-panel")).not.toHaveAttribute("open", "");
  await page.keyboard.press("Escape");
  await expect(destinationSurface(preview, "Innstillingar", true)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(preview.locator("[data-workspace-page], dialog[open]")).toHaveCount(0);
  await expect(preview.getByRole("textbox", { name: "Svar i tråden" })).toHaveValue("trådutkast gjennom meny");
  await expect(channelInput).toHaveValue("kanalutkast gjennom meny");
});
