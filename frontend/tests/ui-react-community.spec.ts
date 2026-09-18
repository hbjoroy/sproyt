import { expect, test, type Page, type Locator } from "@playwright/test";

async function enter(page: Page, participant: string) {
  await page.goto(`/?participant=${participant}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  return preview;
}
async function menu(preview: Locator, label: string) {
  const menu = preview.getByRole("dialog", { name: "Meny og innstillingar" });
  if (!await menu.isVisible()) {
    const trigger = preview.getByRole("button", { name: "Meny og innstillingar", exact: true });
    if (!await trigger.isVisible()) await preview.getByRole("button", { name: "Meny", exact: true }).click();
    await trigger.click();
  }
  await menu.getByRole("button", { name: label, exact: true }).click();
}
async function createCircle(preview: Locator, name: string) {
  await menu(preview, "Ny vennekrets");
  const dialog = preview.getByRole("dialog", { name: "Ny vennekrets", exact: true });
  await dialog.getByLabel("Namn på vennekrets").fill(name);
  await dialog.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(preview.getByRole("region", { name, exact: true })).toBeVisible();
}

test("real React circle creates Prat once, creates scoped channel, edits Markdown and confirms deletion", async ({ page }) => {
  const commands: Array<{ type: string; payload?: any }> = [];
  let sockets = 0;
  page.on("websocket", socket => { sockets++; socket.on("framesent", ({ payload }) => commands.push(JSON.parse(String(payload)))); });
  const preview = await enter(page, `community-owner-${Date.now()}`);
  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("opphavleg utkast");
  await createCircle(preview, "Reell React krets");
  await expect.poll(() => commands.filter(item => item.type === "create_channel" && item.payload.name === "Prat").length).toBe(1);
  const circle = preview.getByRole("region", { name: "Reell React krets", exact: true });
  await circle.getByRole("button", { name: "Kanalar og medlemskap" }).click();
  const channels = preview.getByRole("dialog", { name: "Kanalar i Reell React krets" });
  await channels.getByLabel("Kanalnamn").fill("Turplan");
  await channels.getByLabel("Kanaltype").selectOption("public");
  await channels.getByRole("button", { name: "Opprett kanal" }).click();
  await expect(channels).toHaveCount(0);
  await menu(preview, "Kanaldetaljar, medlemmer og integrasjonar");
  const details = preview.getByRole("dialog", { name: "Kanaldetaljar: Turplan" });
  await details.getByLabel("Kanalomtale (Markdown)").fill("# Planlegg turen\n\nmed vener");
  await details.getByRole("button", { name: "Lagre omtale" }).click();
  await expect(details).toContainText("Omtalen er lagra.");
  await expect(details.getByRole("heading", { name: "Planlegg turen" })).toBeVisible();
  await page.keyboard.press("Escape");
  await circle.getByRole("button", { name: "Kanalar og medlemskap" }).click();
  await channels.getByRole("button", { name: "Slett vennekrets", exact: true }).click();
  await channels.getByRole("button", { name: "Avbryt", exact: true }).click();
  expect(commands.filter(item => item.type === "delete_circle")).toHaveLength(0);
  await channels.getByRole("button", { name: "Slett vennekrets", exact: true }).click();
  await channels.getByRole("button", { name: "Ja, slett kretsen" }).click();
  await expect(channels).toHaveCount(0);
  await expect(circle).toHaveCount(0);
  expect(sockets).toBe(1);
  expect(commands.find(item => item.type === "create_channel" && item.payload.name === "Turplan")?.payload.circle_id).toBeTruthy();
});

test("Felles creates a global channel with a global slug and exposes its members directly", async ({ page }) => {
  const commands: Array<{ type: string; payload?: { name?: string; slug?: string; circle_id?: string | null } }> = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => commands.push(JSON.parse(String(payload)))));
  const participant = `community-global-${Date.now()}`;
  const preview = await enter(page, participant);
  await menu(preview, "Kanalar i Felles");
  const channels = preview.getByRole("dialog", { name: "Kanalar i Felles" });
  await channels.getByLabel("Kanalnamn").fill("Felles prat");
  await channels.getByLabel("Kanaltype").selectOption("public");
  await channels.getByRole("button", { name: "Opprett kanal" }).click();
  await expect(channels).toHaveCount(0);
  await expect.poll(() => commands.find(command => command.type === "create_channel" && command.payload?.name === "Felles prat")?.payload).toEqual({
    circle_id: null, kind: "public", name: "Felles prat", slug: "felles-prat"
  });
  await expect(preview.getByRole("button", { name: "# Felles prat" })).toBeVisible();
  await page.keyboard.press("Escape");
  await preview.getByRole("button", { name: "Medlemmer i Felles prat" }).click();
  const details = preview.getByRole("dialog", { name: "Kanaldetaljar: Felles prat" });
  await expect(details.getByRole("searchbox", { name: "Finn kanalmedlem" })).toBeVisible();
  await expect(details.getByRole("listitem")).toContainText(participant);
  await expect(details.getByRole("button", { name: "Last personlista på nytt" })).toBeVisible();
});

test("Felles sends a global registration invitation without using a circle endpoint", async ({ page }) => {
  let globalRequests = 0; let circleRequests = 0; let body: unknown;
  await page.route("**/api/v1/enrollment-invitations?*", route => {
    globalRequests += 1; body = route.request().postDataJSON();
    return route.fulfill({ json: { url: "https://auth.example.org/enrollment/global", expires_at: "2027-01-01T00:00:00Z" } });
  });
  await page.route("**/api/v1/circles/*/enrollment-invitations?*", route => {
    circleRequests += 1;
    return route.fulfill({ status: 500, body: "kretsendepunktet skal ikkje brukast" });
  });
  const preview = await enter(page, `community-global-enrollment-${Date.now()}`);
  await menu(preview, "Inviter ny brukar til Sprøyt");
  const invite = preview.getByRole("dialog", { name: "Inviter ny brukar til Sprøyt" });
  await invite.getByLabel("E-postadresse").fill("new@example.org");
  await invite.getByLabel("Namn på ny brukar").fill("Ny på Sprøyt");
  await invite.getByRole("button", { name: "Send registreringsinvitasjon" }).click();
  await expect(invite).toContainText("Registreringsinvitasjonen er sendt på e-post.");
  await expect(invite.getByRole("textbox", { name: "Invitasjonslenkje", exact: true })).toHaveValue("https://auth.example.org/enrollment/global");
  expect(body).toEqual({ email: "new@example.org", display_name: "Ny på Sprøyt" });
  expect(globalRequests).toBe(1);
  expect(circleRequests).toBe(0);
  await page.keyboard.press("Escape");
  const management = preview.getByRole("dialog", { name: "Meny og innstillingar" });
  await expect(management).toBeVisible();
  await expect(management.getByRole("button", { name: "Inviter ny brukar til Sprøyt" })).toBeFocused();
});

test("people search filters self, opens real DM and preserves original draft", async ({ page, context }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const peer = await context.newPage();
  await enter(peer, "community-peer-anna");
  const preview = await enter(page, "community-dm-owner");
  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("privat kanalutkast");
  await menu(preview, "Personar og ny direktemelding");
  const people = preview.getByRole("dialog", { name: "Personar og ny direktemelding" });
  await people.getByRole("searchbox", { name: "Finn person" }).fill("community-dm-owner");
  await expect(people.getByRole("button", { name: /Start samtale med/ })).toHaveCount(0);
  await people.getByRole("searchbox", { name: "Finn person" }).fill("community-peer-anna");
  const direct = people.getByRole("button", { name: "Start samtale med community-peer-anna" });
  const info = people.locator(".sp-person-info");
  const [directBox, infoBox] = await Promise.all([direct.boundingBox(), info.boundingBox()]);
  expect(directBox?.width).toBeLessThanOrEqual(48);
  expect(infoBox?.width).toBeGreaterThan(160);
  expect(infoBox?.height).toBeLessThan(80);
  await direct.click();
  await page.setViewportSize({ width: 1280, height: 720 });
  await expect(people).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("");
  await preview.getByRole("button", { name: /^# general/i }).click();
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("privat kanalutkast");
});

test("invitation link joins the correct circle, member can leave, enrollment reports configuration failure", async ({ page, context }) => {
  const preview = await enter(page, `community-invite-owner-${Date.now()}`);
  await createCircle(preview, "Invitasjonskrets");
  await preview.getByRole("region", { name: "Invitasjonskrets", exact: true }).getByRole("button", { name: "Inviter personar og nye brukarar" }).click();
  const invite = preview.getByRole("dialog", { name: "Inviter til Invitasjonskrets" });
  await invite.getByRole("button", { name: "Lag kretslenkje" }).click();
  await expect(invite.getByRole("textbox", { name: "Invitasjonslenkje", exact: true })).toHaveValue(/invite=/);
  const link = await invite.getByRole("textbox", { name: "Invitasjonslenkje", exact: true }).inputValue();
  await invite.getByRole("textbox", { name: "E-postadresse" }).fill("test@example.org");
  await invite.getByRole("button", { name: "Send registreringsinvitasjon" }).click();
  await expect(invite).toContainText("Registrering av nye brukarar er ikkje tilgjengeleg enno");
  const peer = await context.newPage(); const member = await enter(peer, `community-invite-member-${Date.now()}`);
  await menu(member, "Kretsadministrasjon og invitasjonskode");
  const admin = member.getByRole("dialog", { name: "Kretsadministrasjon og invitasjonskode" });
  await admin.getByLabel("Invitasjonskode eller lenkje").fill(link);
  await admin.getByRole("button", { name: "Godta invitasjon" }).click();
  await expect(admin).toContainText("Invitasjonen er godteken");
  await admin.getByRole("button", { name: "Administrer Invitasjonskrets" }).click();
  const channels = member.getByRole("dialog", { name: "Kanalar i Invitasjonskrets" });
  await expect(channels.getByRole("button", { name: "Slett vennekrets" })).toHaveCount(0);
  await channels.getByRole("button", { name: "Forlat vennekrets", exact: true }).click();
  await channels.getByRole("button", { name: "Ja, forlat kretsen" }).click();
  await expect(channels).toHaveCount(0);
  await expect(member.getByRole("region", { name: "Invitasjonskrets", exact: true })).toHaveCount(0);
});

test("community rejects keep entered values, member loads retry and enrollment keeps scoped API contract", async ({ page }) => {
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    const rejected = new Set<string>();
    window.WebSocket = class extends NativeWebSocket {
      send(data: string) {
        const command = JSON.parse(data);
        if (["create_circle", "list_channel_users"].includes(command.type) && !rejected.has(command.type)) {
          rejected.add(command.type);
          setTimeout(() => this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({ protocol: "sproyt.chat.v1", request_id: command.request_id, type: "error", payload: { code: "validation_error", message: "Mellombels feil, prøv igjen" } }) })), 30);
        } else super.send(data);
      }
    };
  });
  const participant = `community-retry-${Date.now()}`;
  const preview = await enter(page, participant);
  await menu(preview, "Ny vennekrets");
  const create = preview.getByRole("dialog", { name: "Ny vennekrets", exact: true });
  await create.getByLabel("Namn på vennekrets").fill("Krets etter retry");
  await create.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(create).toContainText("Mellombels feil, prøv igjen");
  await expect(create.getByLabel("Namn på vennekrets")).toHaveValue("Krets etter retry");
  await create.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(create).toHaveCount(0);
  await expect(preview.getByRole("button", { name: "# Prat", exact: true })).toBeVisible();
  await menu(preview, "Kanaldetaljar, medlemmer og integrasjonar");
  const details = preview.getByRole("dialog", { name: "Kanaldetaljar: Prat" });
  await expect(details).toContainText("Mellombels feil, prøv igjen");
  await details.getByRole("button", { name: "Last personlista på nytt" }).click();
  await expect(details.getByText("Mellombels feil, prøv igjen")).toHaveCount(0);
  await expect(details.getByRole("listitem")).toContainText(participant);
  await page.keyboard.press("Escape");
  await preview.getByRole("region", { name: "Krets etter retry", exact: true }).getByRole("button", { name: "Inviter personar og nye brukarar" }).click();
  let body: unknown;
  let endpoint = "";
  await page.route("**/api/v1/circles/*/enrollment-invitations?*", route => {
    body = route.request().postDataJSON(); endpoint = route.request().url();
    return route.fulfill({ json: { url: "https://auth.example.org/enrollment/test", expires_at: "2027-01-01T00:00:00Z" } });
  });
  const invitation = preview.getByRole("dialog", { name: "Inviter til Krets etter retry" });
  await invitation.getByLabel("E-postadresse").fill("invite@example.org");
  await invitation.getByLabel("Namn på ny brukar").fill("Ny ven");
  await invitation.getByRole("button", { name: "Send registreringsinvitasjon" }).click();
  await expect(invitation).toContainText("Registreringsinvitasjonen er sendt på e-post.");
  await expect(invitation.getByRole("textbox", { name: "Invitasjonslenkje", exact: true })).toHaveValue("https://auth.example.org/enrollment/test");
  expect(body).toEqual({ email: "invite@example.org", display_name: "Ny ven" });
  expect(endpoint).toMatch(/\/circles\/[0-9a-f-]{36}\/enrollment-invitations/);
});

test("channel discovery, joining, direct member addition and circle DM invitations use real membership", async ({ page, context }) => {
  const memberPage = await context.newPage();
  const member = await enter(memberPage, "community-join-peer");
  const preview = await enter(page, `community-members-${Date.now()}`);
  const sent: Array<{ type: string; payload?: any }> = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => sent.push(JSON.parse(String(payload)))));
  await createCircle(preview, "Medlemskrets");
  const circle = preview.getByRole("region", { name: "Medlemskrets", exact: true });
  await circle.getByRole("button", { name: "Inviter personar og nye brukarar" }).click();
  const invite = preview.getByRole("dialog", { name: "Inviter til Medlemskrets" });
  await invite.getByRole("searchbox", { name: "Finn person å invitere" }).fill("community-join-peer");
  await invite.getByRole("button", { name: "Inviter community-join-peer", exact: true }).click();
  await expect(invite).toContainText("Invitasjonen til community-join-peer er sendt i direktemelding.");
  await invite.getByRole("button", { name: "Lag kretslenkje" }).click();
  const linkField = invite.getByRole("textbox", { name: "Invitasjonslenkje", exact: true });
  await expect(linkField).toHaveValue(/invite=/);
  const link = await linkField.inputValue();
  await menu(member, "Kretsadministrasjon og invitasjonskode");
  const admin = member.getByRole("dialog", { name: "Kretsadministrasjon og invitasjonskode" });
  await admin.getByLabel("Invitasjonskode eller lenkje").fill(link);
  await admin.getByRole("button", { name: "Godta invitasjon" }).click();
  await expect(admin).toContainText("Invitasjonen er godteken");
  await memberPage.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  const createChannel = async (name: string, kind: string) => {
    await circle.getByRole("button", { name: "Kanalar og medlemskap" }).click();
    const dialog = preview.getByRole("dialog", { name: "Kanalar i Medlemskrets" });
    await dialog.getByLabel("Kanalnamn").fill(name);
    await dialog.getByLabel("Kanaltype").selectOption(kind);
    await dialog.getByRole("button", { name: "Opprett kanal" }).click();
    await expect(dialog).toHaveCount(0);
  };
  await createChannel("Open tur", "public");
  await member.getByRole("region", { name: "Medlemskrets", exact: true }).getByRole("button", { name: "Kanalar og medlemskap" }).click();
  const discover = member.getByRole("dialog", { name: "Kanalar i Medlemskrets" });
  await discover.getByRole("button", { name: "Bli med i Open tur" }).click();
  await expect(discover).toHaveCount(0);
  await menu(member, "Kanaldetaljar, medlemmer og integrasjonar");
  const memberDetails = member.getByRole("dialog", { name: "Kanaldetaljar: Open tur" });
  await expect(memberDetails.getByLabel("Kanalomtale (Markdown)")).toHaveCount(0);
  await expect(memberDetails.getByRole("region", { name: "Legg til kanalmedlem" })).toHaveCount(0);
  await memberDetails.getByRole("button", { name: "Forlat kanalen", exact: true }).click();
  await memberDetails.getByRole("button", { name: "Ja, forlat kanalen" }).click();
  await expect(memberDetails).toHaveCount(0);
  await createChannel("Privat tur", "private");
  await menu(preview, "Kanaldetaljar, medlemmer og integrasjonar");
  const details = preview.getByRole("dialog", { name: "Kanaldetaljar: Privat tur" });
  await details.getByLabel("Vel person").selectOption({ label: "community-join-peer" });
  await details.getByRole("button", { name: "Legg til", exact: true }).click();
  await expect(details).toContainText("Personen er lagd til.");
  await expect(details.getByRole("button", { name: "Start samtale med community-join-peer" })).toBeVisible();
});
