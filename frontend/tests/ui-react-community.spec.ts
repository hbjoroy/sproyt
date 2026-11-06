import { openChannelMenu } from "./support/channel-menu";
import { destinationSurface } from "./support/destination-surface";
import { expect, test, type Page, type Locator } from "@playwright/test";

async function enter(page: Page, participant: string) {
  await page.goto(`/?participant=${participant}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled({ timeout: 15_000 });
  return preview;
}
function circleGroup(preview: Locator, name: string) {
  return preview.locator("[data-conversation-group]").filter({ has: preview.page().getByText(name, { exact: true }) });
}
async function openCircleNavigation(preview: Locator) {
  const back = preview.getByRole("button", { name: "Samtalar", exact: true });
  if (await back.isVisible()) await back.click();
  const tab = preview.getByRole("tab", { name: /^Kretsar/ });
  if (await tab.getAttribute("aria-selected") !== "true") await tab.click();
}
async function closeManagedDialog(page: Page) {
  const preview = page.locator("#sproyt-react-preview");
  const before = page.url();
  const dialogs = preview.locator("[data-workspace-page], dialog[open]");
  const previous = await dialogs.count() ? await dialogs.last().elementHandle() : null;
  // A local popup can sit above a management dialog without changing its URL.
  const routed = !!previous && new URL(before).searchParams.get("view") === "settings"
    && await preview.locator("dialog[open]").count() === 1;
  await page.keyboard.press("Escape");
  if (routed) await expect.poll(() => page.url()).not.toBe(before);
  if (previous) await previous.waitForElementState("hidden");
  if (routed && new URL(page.url()).searchParams.get("view") === "settings") {
    // URL publication precedes React mounting the parent. Wait for both sides
    // of Back before a menu helper decides whether it should open the toolbar.
    await expect(dialogs).toHaveCount(1);
    await expect(dialogs).toBeVisible();
  } else if (routed) await expect(dialogs).toHaveCount(0);
}
async function menu(preview: Locator, label: string) {
  const menu = destinationSurface(preview, "Innstillingar", true);
  // Return through the current destination's real origin before opening a new
  // top-level setting. There must never be multiple modal destinations.
  for (let depth = 0; depth < 4 && !await menu.isVisible(); depth++) {
    const dialog = preview.locator("[data-workspace-page], dialog[open]");
    if (!await dialog.count()) break;
    await expect(dialog).toHaveCount(1);
    await closeManagedDialog(preview.page());
  }
  if (!await menu.isVisible()) {
    const trigger = preview.getByRole("button", { name: "Innstillingar", exact: true });

    await trigger.click();
  }
  const action = menu.getByRole("button", { name: label, exact: true });
  if (!await action.isVisible()) await menu.getByRole("searchbox", { name: "Søk i innstillingar" }).fill(label);
  await action.click();
}
async function circleSettings(preview: Locator, name: string) {
  await menu(preview, "Administrer krets");
  const directory = destinationSurface(preview, "Vel krets", true);
  await directory.getByRole("searchbox", { name: "Finn vennekrets" }).fill(name);
  await directory.getByRole("button", { name, exact: true }).click();
  const circle = destinationSurface(preview, name, true);
  await expect(circle).toBeVisible();
  await expect(preview.locator("[data-workspace-page], dialog[open]")).toHaveCount(1);
  return circle;
}
async function createCircle(preview: Locator, name: string) {
  await menu(preview, "Ny vennekrets");
  const dialog = destinationSurface(preview, "Ny vennekrets", true);
  await dialog.getByLabel("Namn på vennekrets").fill(name);
  await dialog.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(dialog).toHaveCount(0);
  await selectCreatedPrat(preview, name);
}
async function selectCreatedPrat(preview: Locator, name: string) {
  const settings = destinationSurface(preview, "Innstillingar", true);
  if (await settings.isVisible()) {
    await settings.getByRole("button", { name: "Lukk innstillingane", exact: true }).click();
    await expect(settings).toHaveCount(0);
  }
  await openCircleNavigation(preview);
  const group = circleGroup(preview, name);
  await expect(group).toHaveCount(1);
  const disclosure = group.locator(".sp-product-nav-disclosure");
  if (await disclosure.getAttribute("aria-expanded") !== "true") await disclosure.click();
  await group.getByRole("button", { name: "# Prat", exact: true }).click();
}

for (const width of [1280, 390]) test(`navigation exposes scoped channel and invitation actions at ${width}px without the management menu`, async ({ page }) => {
  await page.setViewportSize({ width, height: 844 });
  const commands: Array<{ type: string; payload?: any }> = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => commands.push(JSON.parse(String(payload)))));
  const preview = await enter(page, `scope-navigation-${width}-${Date.now()}`);
  const circleName = `Navigasjonskrets ${width}`;
  await createCircle(preview, circleName);
  await closeManagedDialog(page);
  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("utkast frå navigasjon");
  if (width === 390) await preview.getByRole("button", { name: /Samtalar/ }).click();
  const scope = circleGroup(preview, circleName);
  const scopeMenu = scope.getByRole("button", { name: `Val for ${circleName}`, exact: true });
  await expect(scope.getByRole("button", { name: `Ny kanal i ${circleName}`, exact: true })).toBeHidden();
  await scopeMenu.click();
  const dropdown = scope.getByRole("group", { name: `Val for ${circleName}`, exact: true });
  await expect(dropdown).toBeVisible();
  const bounds = await dropdown.boundingBox();
  expect(bounds?.height).toBeLessThan(280);
  expect(bounds?.x).toBeGreaterThanOrEqual(0);
  expect((bounds?.x ?? 0) + (bounds?.width ?? 0)).toBeLessThanOrEqual(width);
  await preview.getByRole("searchbox", { name: "Finn samtale" }).click();
  await expect(dropdown).toBeHidden();
  await scopeMenu.click();
  await closeManagedDialog(page);
  await expect(scopeMenu).toBeFocused();
  await scopeMenu.click();
  const create = scope.getByRole("button", { name: `Ny kanal i ${circleName}`, exact: true });
  await create.click();
  const channel = destinationSurface(preview, `Ny kanal i ${circleName}`, true);
  await expect(channel.getByLabel("Kanalnamn")).toBeFocused();
  await channel.getByLabel("Kanalnamn").fill("Frå kretslista");
  await channel.getByLabel("Kanaltype").selectOption("public");
  await channel.getByRole("button", { name: "Opprett kanal", exact: true }).click();
  await expect(channel).toHaveCount(0);
  const creation = commands.find(item => item.type === "create_channel" && item.payload?.name === "Frå kretslista");
  expect(creation?.payload.circle_id).toBeTruthy();
  await openCircleNavigation(preview);
  const browse = scope.getByRole("button", { name: `Finn kanalar i ${circleName}`, exact: true });
  await scopeMenu.click();
  await browse.click();
  const directory = destinationSurface(preview, `Kanalar i ${circleName}`, true);
  await directory.getByRole("button", { name: "# Prat Gå inn", exact: true }).click();
  await expect(directory).toHaveCount(0);
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("utkast frå navigasjon");
  if (width === 390) await preview.getByRole("button", { name: /Samtalar/ }).click();
  const invite = scope.getByRole("button", { name: `Inviter til ${circleName}`, exact: true });
  await scopeMenu.click();
  await invite.click();
  await expect(destinationSurface(preview, `Inviter til ${circleName}`, false)).toBeVisible();
  await closeManagedDialog(page);
  await expect(scopeMenu).toBeFocused();
  await openCircleNavigation(preview);
  const shared = preview.locator('[data-conversation-group="scope:shared"]');
  await shared.getByRole("button", { name: "Val for Felles", exact: true }).click();
  await shared.getByRole("button", { name: "Ny kanal i Felles", exact: true }).click();
  await expect(destinationSurface(preview, "Ny kanal i Felles", true)).toBeVisible();
  await closeManagedDialog(page);
  await shared.getByRole("button", { name: "Val for Felles", exact: true }).click();
  await shared.getByRole("button", { name: "Inviter ny brukar til Sprøyt", exact: true }).click();
  await expect(destinationSurface(preview, "Inviter ny brukar til Sprøyt", true)).toBeVisible();
  expect(commands.filter(item => item.type === "create_invitation")).toHaveLength(0);
  const shell = await preview.boundingBox();
  expect(shell?.width).toBeLessThanOrEqual(width);
});

test("real React circle creates Prat once, creates scoped channel, edits Markdown and confirms deletion", async ({ page }) => {
  const commands: Array<{ type: string; payload?: any }> = [];
  let sockets = 0;
  page.on("websocket", socket => { sockets++; socket.on("framesent", ({ payload }) => commands.push(JSON.parse(String(payload)))); });
  const preview = await enter(page, `community-owner-${Date.now()}`);
  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("opphavleg utkast");
  await createCircle(preview, "Reell React krets");
  await expect.poll(() => commands.filter(item => item.type === "create_channel" && item.payload.name === "Prat").length).toBe(1);
  const circle = await circleSettings(preview, "Reell React krets");
  await circle.getByRole("button", { name: "Kanalar og medlemskap" }).click();
  const channels = destinationSurface(preview, "Kanalar i Reell React krets", false);
  await channels.getByRole("button", { name: "Ny kanal i Reell React krets", exact: true }).click();
  const create = destinationSurface(preview, "Ny kanal i Reell React krets", true);
  await create.getByLabel("Kanalnamn").fill("Turplan");
  await create.getByLabel("Kanaltype").selectOption("public");
  await create.getByRole("button", { name: "Opprett kanal" }).click();
  await expect(create).toHaveCount(0);
  await expect(channels).toHaveCount(0);
  await menu(preview, "Kanaldetaljar, medlemmer og integrasjonar");
  const details = destinationSurface(preview, "Kanaldetaljar: Turplan", false);
  await details.getByLabel("Kanalomtale (Markdown)").fill("# Planlegg turen\n\nmed vener");
  await details.getByRole("button", { name: "Lagre omtale" }).click();
  await expect(details).toContainText("Omtalen er lagra.");
  await expect(details.getByRole("heading", { name: "Planlegg turen" })).toBeVisible();
  await closeManagedDialog(page);
  await circleSettings(preview, "Reell React krets");
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
  await page.setViewportSize({ width: 1280, height: 720 });
  const commands: Array<{ type: string; payload?: { name?: string; slug?: string; circle_id?: string | null } }> = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => commands.push(JSON.parse(String(payload)))));
  const participant = `community-global-${Date.now()}`;
  const preview = await enter(page, participant);
  await menu(preview, "Kanalar i Felles");
  const channels = destinationSurface(preview, "Kanalar i Felles", false);
  await channels.getByRole("button", { name: "Ny kanal i Felles", exact: true }).click();
  const create = destinationSurface(preview, "Ny kanal i Felles", true);
  await create.getByLabel("Kanalnamn").fill("Felles prat");
  await create.getByLabel("Kanaltype").selectOption("public");
  await create.getByRole("button", { name: "Opprett kanal" }).click();
  await expect(create).toHaveCount(0);
  await expect(channels).toHaveCount(0);
  await expect.poll(() => commands.find(command => command.type === "create_channel" && command.payload?.name === "Felles prat")?.payload).toEqual({
    circle_id: null, kind: "public", name: "Felles prat", slug: "felles-prat"
  });
  await expect(preview.getByRole("button", { name: "# Felles prat" })).toBeVisible();
  const management = destinationSurface(preview, "Innstillingar", true);
  // Creation selects its canonical conversation, closing settings rather than
  // leaving the overview over the newly created channel.
  await expect(management).toHaveCount(0);
  await preview.getByRole("button", { name: "Medlemmer i Felles prat" }).click();
  const details = destinationSurface(preview, "Kanaldetaljar: Felles prat", false);
  await expect(details.getByRole("searchbox", { name: "Finn kanalmedlem" })).toBeVisible();
  await expect(details.getByRole("listitem")).toContainText(participant);
  await expect(details.getByRole("button", { name: "Last personlista på nytt" })).toBeVisible();
  await closeManagedDialog(page);
  await openChannelMenu(preview);
  await preview.getByRole("group", { name: "Kanalval: Felles prat" }).getByRole("button", { name: "Legg til eller inviter kanalmedlem" }).click();
  await expect(details.getByLabel("Vel person", { exact: true })).toBeFocused();
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
  const invite = destinationSurface(preview, "Inviter ny brukar til Sprøyt", false);
  await invite.getByLabel("E-postadresse").fill("new@example.org");
  await invite.getByLabel("Namn på ny brukar").fill("Ny på Sprøyt");
  await invite.getByRole("button", { name: "Send registreringsinvitasjon" }).click();
  await expect(invite).toContainText("Registreringsinvitasjonen er sendt på e-post.");
  await expect(invite.getByRole("textbox", { name: "Invitasjonslenkje", exact: true })).toHaveValue("https://auth.example.org/enrollment/global");
  expect(body).toEqual({ email: "new@example.org", display_name: "Ny på Sprøyt" });
  expect(globalRequests).toBe(1);
  expect(circleRequests).toBe(0);
  await closeManagedDialog(page);
  const management = destinationSurface(preview, "Innstillingar", false);
  await expect(management).toBeVisible();
  await expect(management.getByRole("button", { name: "Inviter ny brukar til Sprøyt" })).toBeFocused();
});

test("people search filters self, opens real DM and preserves original draft", async ({ page, context }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const peer = await context.newPage();
  await enter(peer, "community-peer-anna");
  const preview = await enter(page, "community-dm-owner");
  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("privat kanalutkast");
  await menu(preview, "Ny direktemelding");
  const people = destinationSurface(preview, "Ny direktemelding", false);
  await people.getByRole("searchbox", { name: "Finn person" }).fill("community-dm-owner");
  await expect(people.getByRole("button", { name: /Start samtale med/ })).toHaveCount(0);
  await people.getByRole("searchbox", { name: "Finn person" }).fill("community-peer-anna");
  const direct = people.getByRole("button", { name: "Start samtale med community-peer-anna" });
  const info = direct.locator(".sp-direct-person-copy");
  await expect(info).toContainText("community-peer-anna");
  await expect(info.locator(".sp-direct-person-detail")).toContainText("Ny samtale");
  const [directBox, infoBox] = await Promise.all([direct.boundingBox(), info.boundingBox()]);
  expect(infoBox?.width).toBeGreaterThan(160);
  expect(infoBox?.height).toBeLessThan(80);
  expect(infoBox!.x).toBeGreaterThanOrEqual(directBox!.x);
  expect(infoBox!.x + infoBox!.width).toBeLessThanOrEqual(directBox!.x + directBox!.width);
  expect(await direct.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await direct.click();
  await page.setViewportSize({ width: 1280, height: 720 });
  await expect(people).toHaveCount(0);
  await closeManagedDialog(page);
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("");
  await openCircleNavigation(preview);
  const shared = preview.locator('[data-conversation-group="scope:shared"]');
  const disclosure = shared.locator(".sp-product-nav-disclosure");
  if (await disclosure.getAttribute("aria-expanded") !== "true") await disclosure.click();
  await preview.getByRole("button", { name: /^# general/i }).click();
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("privat kanalutkast");
});

test("invitation link joins the correct circle, member can leave, enrollment reports configuration failure", async ({ page, context }) => {
  const preview = await enter(page, `community-invite-owner-${Date.now()}`);
  await createCircle(preview, "Invitasjonskrets");
  await (await circleSettings(preview, "Invitasjonskrets")).getByRole("button", { name: "Inviter personar og nye brukarar" }).click();
  const invite = destinationSurface(preview, "Inviter til Invitasjonskrets", false);
  await invite.getByRole("button", { name: "Lag kretslenkje" }).click();
  await expect(invite.getByRole("textbox", { name: "Invitasjonslenkje", exact: true })).toHaveValue(/invite=/);
  const link = await invite.getByRole("textbox", { name: "Invitasjonslenkje", exact: true }).inputValue();
  await invite.getByRole("textbox", { name: "E-postadresse" }).fill("test@example.org");
  await invite.getByRole("button", { name: "Send invitasjon" }).click();
  await expect(invite).toContainText("Registrering av nye brukarar er ikkje tilgjengeleg enno");
  const peer = await context.newPage(); const member = await enter(peer, `community-invite-member-${Date.now()}`);
  await menu(member, "Bruk invitasjonskode");
  const admin = destinationSurface(member, "Vel krets", true);
  await admin.locator("summary").filter({ hasText: "Bruk invitasjonskode" }).click();
  await admin.getByLabel("Invitasjonskode eller lenkje").fill(link);
  await admin.getByRole("button", { name: "Godta invitasjon" }).click();
  await expect(circleGroup(member, "Invitasjonskrets")).toHaveCount(1);
  await expect(member.locator(".sp-context")).toContainText("Invitasjonskrets");
  // Joining can select the invited conversation. Open the joined circle from
  // settings afresh instead of retaining the pre-join directory instance.
  await (await circleSettings(member, "Invitasjonskrets")).getByRole("button", { name: "Kanalar og medlemskap" }).click();
  const channels = destinationSurface(member, "Kanalar i Invitasjonskrets", false);
  await expect(channels.getByRole("button", { name: "Slett vennekrets" })).toHaveCount(0);
  await channels.getByRole("button", { name: "Forlat vennekrets", exact: true }).click();
  await channels.getByRole("button", { name: "Ja, forlat kretsen" }).click();
  await expect(channels).toHaveCount(0);
  await expect(circleGroup(member, "Invitasjonskrets")).toHaveCount(0);
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
  const create = destinationSurface(preview, "Ny vennekrets", true);
  await create.getByLabel("Namn på vennekrets").fill("Krets etter retry");
  await create.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(create).toContainText("Mellombels feil, prøv igjen");
  await expect(create.getByLabel("Namn på vennekrets")).toHaveValue("Krets etter retry");
  await create.getByRole("button", { name: "Opprett vennekrets" }).click();
  await expect(create).toHaveCount(0);
  await selectCreatedPrat(preview, "Krets etter retry");
  await menu(preview, "Kanaldetaljar, medlemmer og integrasjonar");
  const details = destinationSurface(preview, "Kanaldetaljar: Prat", false);
  await details.getByRole("tab", { name: "Medlemmer", exact: true }).click();
  await expect(details).toContainText("Mellombels feil, prøv igjen");
  await details.getByRole("button", { name: "Last personlista på nytt" }).click();
  await expect(details.getByText("Mellombels feil, prøv igjen")).toHaveCount(0);
  await expect(details.getByRole("listitem")).toContainText(participant);
  await closeManagedDialog(page);
  await (await circleSettings(preview, "Krets etter retry")).getByRole("button", { name: "Inviter personar og nye brukarar" }).click();
  let body: unknown;
  let endpoint = "";
  await page.route("**/api/v1/circles/*/enrollment-invitations?*", route => {
    body = route.request().postDataJSON(); endpoint = route.request().url();
    return route.fulfill({ json: { url: "https://auth.example.org/enrollment/test", expires_at: "2027-01-01T00:00:00Z" } });
  });
  const invitation = destinationSurface(preview, "Inviter til Krets etter retry", false);
  await invitation.getByLabel("E-postadresse").fill("invite@example.org");
  await invitation.getByLabel("Namn (valfritt for nye brukarar)").fill("Ny ven");
  await invitation.getByRole("button", { name: "Send invitasjon" }).click();
  await expect(invitation).toContainText("Invitasjonen er sendt på e-post.");
  await expect(invitation.getByRole("textbox", { name: "Invitasjonslenkje", exact: true })).toHaveValue("https://auth.example.org/enrollment/test");
  expect(body).toEqual({ email: "invite@example.org", display_name: "Ny ven" });
  expect(endpoint).toMatch(/\/circles\/[0-9a-f-]{36}\/enrollment-invitations/);
});

test("channel discovery, joining, direct member addition and circle DM invitations use real membership", async ({ page, context }) => {
  test.setTimeout(90_000); // Two users traverse several real dialogs on mobile WebKit.
  const memberPage = await context.newPage();
  const member = await enter(memberPage, "community-join-peer");
  const preview = await enter(page, `community-members-${Date.now()}`);
  const sent: Array<{ type: string; payload?: any }> = [];
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => sent.push(JSON.parse(String(payload)))));
  await createCircle(preview, "Medlemskrets");
  const circle = await circleSettings(preview, "Medlemskrets");
  await circle.getByRole("button", { name: "Inviter personar og nye brukarar" }).click();
  const invite = destinationSurface(preview, "Inviter til Medlemskrets", false);
  await invite.getByRole("searchbox", { name: "Finn person å invitere" }).fill("community-join-peer");
  await invite.getByRole("button", { name: "Inviter community-join-peer", exact: true }).click();
  await expect(invite).toContainText("Invitasjonen til community-join-peer er sendt i direktemelding.");
  await invite.getByRole("button", { name: "Lag kretslenkje" }).click();
  const linkField = invite.getByRole("textbox", { name: "Invitasjonslenkje", exact: true });
  await expect(linkField).toHaveValue(/invite=/);
  const link = await linkField.inputValue();
  await menu(member, "Bruk invitasjonskode");
  const admin = destinationSurface(member, "Vel krets", true);
  await admin.locator("summary").filter({ hasText: "Bruk invitasjonskode" }).click();
  await admin.getByLabel("Invitasjonskode eller lenkje").fill(link);
  await admin.getByRole("button", { name: "Godta invitasjon" }).click();
  await expect(circleGroup(member, "Medlemskrets")).toHaveCount(1);
  await expect(member.locator(".sp-context")).toContainText("Medlemskrets");
  await closeManagedDialog(memberPage);
  await closeManagedDialog(page);
  const createChannel = async (name: string, kind: string) => {
    await circleSettings(preview, "Medlemskrets");
    await circle.getByRole("button", { name: "Kanalar og medlemskap" }).click();
    const dialog = destinationSurface(preview, "Kanalar i Medlemskrets", false);
    await dialog.getByRole("button", { name: "Ny kanal i Medlemskrets", exact: true }).click();
    const create = destinationSurface(preview, "Ny kanal i Medlemskrets", true);
    await create.getByLabel("Kanalnamn").fill(name);
    await create.getByLabel("Kanaltype").selectOption(kind);
    await create.getByRole("button", { name: "Opprett kanal" }).click();
    await expect(create).toHaveCount(0);
    await expect(dialog).toHaveCount(0);
  };
  await createChannel("Open tur", "public");
  await (await circleSettings(member, "Medlemskrets")).getByRole("button", { name: "Kanalar og medlemskap" }).click();
  const discover = destinationSurface(member, "Kanalar i Medlemskrets", false);
  await discover.getByRole("button", { name: "Bli med i Open tur" }).click();
  await expect(discover).toHaveCount(0);
  await menu(member, "Kanaldetaljar, medlemmer og integrasjonar");
  const memberDetails = destinationSurface(member, "Kanaldetaljar: Open tur", false);
  await memberDetails.getByRole("tab", { name: "Medlemmer", exact: true }).click();
  await expect(memberDetails.getByLabel("Kanalomtale (Markdown)")).toHaveCount(0);
  await expect(memberDetails.getByRole("region", { name: "Legg til kanalmedlem" })).toHaveCount(0);
  await memberDetails.getByRole("tab", { name: "Om", exact: true }).click();
  await memberDetails.getByRole("button", { name: "Forlat kanalen", exact: true }).click();
  await memberDetails.getByRole("button", { name: "Ja, forlat kanalen" }).click();
  await expect(memberDetails).toHaveCount(0);
  await createChannel("Privat tur", "private");
  await menu(preview, "Kanaldetaljar, medlemmer og integrasjonar");
  const details = destinationSurface(preview, "Kanaldetaljar: Privat tur", false);
  await details.getByRole("tab", { name: "Medlemmer", exact: true }).click();
  await details.getByLabel("Vel person").selectOption({ label: "community-join-peer" });
  await details.getByRole("button", { name: "Legg til", exact: true }).click();
  await expect(details).toContainText("Personen er lagd til.");
  await expect(details.getByRole("button", { name: "Opne samtale med community-join-peer" })).toBeVisible();
});
