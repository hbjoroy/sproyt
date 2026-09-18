import { expect, test, type Locator, type Page } from "@playwright/test";

async function openPreview(page: Page, participant: string) {
  await page.goto(`/?participant=${participant}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  return preview;
}

async function openManagement(preview: Locator) {
  const trigger = preview.getByRole("button", { name: "Meny og innstillingar" });
  if (!await trigger.isVisible()) await preview.getByRole("button", { name: "Meny", exact: true }).click();
  await trigger.click();
  return trigger;
}

test("management directory contains focus, cancels with Escape and does not mutate", async ({ page }) => {
  const commands: string[] = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => commands.push(JSON.parse(String(payload)).type)));
  const preview = await openPreview(page, "preview-management-cancel");
  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("utkast medan menyen er open");
  const trigger = await openManagement(preview);
  const dialog = preview.getByRole("dialog", { name: "Meny og innstillingar" });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Samtalen og utkasta dine blir tekne vare på");
  await page.keyboard.press("Tab");
  expect(await dialog.evaluate(element => element.contains(document.activeElement))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  await expect(trigger).toBeFocused();
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("utkast medan menyen er open");
  expect(commands.filter(command => /^(create_|accept_|enable_|start_)/.test(command))).toEqual([]);
});

for (const [label, title] of [
  ["Personar og ny direktemelding", /Personar og ny direktemelding/],
  ["Kanaldetaljar, medlemmer og integrasjonar", /Kanaldetaljar:/],
  ["Kretsadministrasjon og invitasjonskode", /Kretsadministrasjon og invitasjonskode/],
] as const) {
  test(`management opens ${label} in React without reconnecting or losing draft`, async ({ page }) => {
    let sockets = 0;
    page.on("websocket", () => sockets++);
    const preview = await openPreview(page, `preview-management-${label}`);
    await preview.getByRole("textbox", { name: "Skriv melding" }).fill("behald administrasjonsutkast");
    await openManagement(preview);
    await preview.getByRole("button", { name: label, exact: true }).click();
    const dialog = preview.getByRole("dialog", { name: title });
    await expect(dialog).toBeVisible();
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
  await preview.getByRole("button", { name: "Ny vennekrets", exact: true }).click();
  const creation = preview.getByRole("dialog", { name: "Ny vennekrets", exact: true });
  await creation.getByRole("textbox", { name: "Namn på vennekrets" }).fill("Krets frå React");
  await creation.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(creation).toHaveCount(0);
  preview = await openPreview(page, participant);
  await openManagement(preview);
  const circle = preview.getByRole("region", { name: "Krets frå React", exact: true });
  await circle.getByRole("button", { name: "Kanalar og medlemskap" }).click();
  await expect(preview.getByRole("dialog", { name: "Kanalar i Krets frå React" })).toBeVisible();
  preview = await openPreview(page, participant);
  await openManagement(preview);
  await preview.getByRole("searchbox", { name: "Finn vennekrets" }).fill("Krets frå React");
  await preview.getByRole("region", { name: "Krets frå React", exact: true })
    .getByRole("button", { name: "Inviter personar og nye brukarar" }).click();
  const invitation = preview.getByRole("dialog", { name: "Inviter til Krets frå React" });
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
    await preview.getByRole("button", { name: "Meny", exact: true }).click();
    await preview.getByRole("button", { name: "Meny og innstillingar" }).click();
    await preview.getByRole("button", { name: label, exact: true }).click();
    const dialog = preview.getByRole("dialog", { name: label, exact: true });
    await expect(dialog.getByLabel(field, { exact: true })).toBeVisible();
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
  await preview.getByRole("button", { name: /Innboks og oppgåver/ }).click();
  await preview.getByRole("button", { name: "Oppgåver", exact: true }).click();
  await expect(preview.getByRole("dialog", { name: "Innboks og oppgåver" })).toBeVisible();
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
  await preview.getByRole("button", { name: "Personar og ny direktemelding" }).click();
  await expect(preview.getByRole("dialog", { name: "Personar og ny direktemelding" })).toBeVisible();
  await expect(page.locator("#thread-panel")).not.toHaveAttribute("open", "");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await expect(preview.getByRole("textbox", { name: "Svar i tråden" })).toHaveValue("trådutkast gjennom meny");
  await expect(channelInput).toHaveValue("kanalutkast gjennom meny");
});
