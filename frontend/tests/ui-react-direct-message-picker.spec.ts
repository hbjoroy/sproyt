import { destinationSurface } from "./support/destination-surface";
import { expect, test, type Page } from "@playwright/test";

const owner = "dm-picker-owner", ada = "dm-picker-ada", grace = "dm-picker-grace";
const directId = "00000000-0000-7000-8000-000000009501";
const groupId = "00000000-0000-7000-8000-000000009502";
const profile = (id: string, display_name: string, handle: string) => ({ id, display_name, handle, kind: "human",
  external_provider: null, external_subject: null, created_at: "2026-01-01T00:00:00Z", status_text: "", status_emoji: "", status_expires_at: null, early_adopter: false });
const newDirectId = "00000000-0000-7000-8000-000000009503";
const createdCircleId = "00000000-0000-7000-8000-000000009505";
const pratId = "00000000-0000-7000-8000-000000009506";
async function fixture(page: Page, options: { empty?: boolean; holdDirectory?: boolean; message?: boolean } = {}) {
  await page.route(/\/api\/v1\/me\/favorite-circles(?:\?.*)?$/, route => route.fulfill({ json: [] }));
  let identity = owner;
  const createdCircles = new Map<string, { id: string; name: string; slug: string; created_by: string; created_at: string }>();
  const rootMessage = { id: "00000000-0000-7000-8000-000000009504", channel_id: directId, parent_message_id: null,
    sender_id: ada, sender_display_name: "Ada Lovelace", body: "Ei melding med ei eiga tråd", sequence: 1,
    sent_at: "2026-01-01T00:00:00Z", edited_at: null, deleted_at: null };
  const users = () => [profile(identity, "Meg", "my_handle"), profile(ada, "Ada Lovelace", "ada_handle"), profile(grace, "Grace Hopper", "grace_handle")];
  const channels = options.empty ? [] : [
    { id: directId, name: "Legacy Ada handle", direct_user_id: ada },
    { id: groupId, name: "Ada Lovelace, Grace Hopper og meg", direct_user_id: null }
  ].map(channel => ({ ...channel, slug: channel.id, kind: "private", circle_id: null, is_direct: true,
    role: "owner", description: "Skal ikkje visast som redigerbar kanalomtale", last_read_sequence: 0, latest_sequence: options.message && channel.id === directId ? 1 : 0 }));
  const commands: { type: string; request_id?: string; payload?: { user_id?: string; channel_id?: string; token?: string } }[] = [];
  let usersFailure = false, holdUsers = false, holdOpen = false, holdExpansion = false, holdPrat = false;
  let holdChannels = !!options.holdDirectory, holdCircles = !!options.holdDirectory;
  let releaseUsers: (() => string) | undefined, releaseOpen: (() => string) | undefined, releaseExpansion: (() => string) | undefined;
  let releaseChannels: (() => string) | undefined, releaseCircles: (() => string) | undefined, releasePrat: (() => string) | undefined;
  const deliver = async (release: (() => string) | undefined) => {
    expect(release, "the held request must exist before its response is released").toBeDefined();
    expect(release!(), "the held response must retain its original wire request ID").toBeTruthy();
    // routeWebSocket sends through Playwright's injected transport; native
    // page.on("websocket") frame events do not reliably observe that transport.
    // Drain its queued message and React's DOM publication before unchanged-UI checks.
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  };
  let emit = (_type: string, _payload: unknown, _requestId?: string) => {};
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    emit = (type, payload, request_id) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload, ...(request_id ? { request_id } : {}) }));
    route.onMessage(data => {
      const command = JSON.parse(String(data)); commands.push(command);
      const reply = (type: string, payload: unknown = {}) => emit(type, payload, command.request_id);
      const selected = command.payload?.channel_id;
      switch (command.type) {
        case "hello": reply("hello", { participant_id: identity, signup_ordinal: 1 }); break;
        case "ping": reply("pong"); break;
        case "list_users": {
          const people = users();
          if (usersFailure) { usersFailure = false; reply("error", { code: "unavailable", message: "Kunne ikkje hente personane no" }); }
          else if (holdUsers) {
            holdUsers = false;
            releaseUsers = () => { reply("users_listed", { users: people.map(person => person.id === ada ? { ...person, display_name: "Utdatert person frå første A" } : person) }); return command.request_id; };
          } else reply("users_listed", { users: people });
          break;
        }
        case "list_my_circles": {
          const ownCircle = createdCircles.get(identity);
          const circles = ownCircle ? [[ownCircle, "owner"]] : [];
          if (holdCircles) { holdCircles = false; releaseCircles = () => { reply("circles_listed", { circles }); return command.request_id; }; }
          else reply("circles_listed", { circles });
          break;
        }
        case "list_my_channels":
          if (holdChannels) { holdChannels = false; releaseChannels = () => { reply("channels_listed", { channels }); return command.request_id; }; }
          else reply("channels_listed", { channels });
          break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "list_channel_users": reply("channel_users_listed", { channel_id: selected, users: selected === directId ? users().slice(0, 2) : users() }); break;
        case "open_direct_channel":
          if (holdOpen) {
            holdOpen = false;
            const requestedOwner = identity;
            releaseOpen = () => { reply("direct_channel_opened", { channel: { id: newDirectId, slug: "direct-grace", name: "Grace Hopper", kind: "private", circle_id: null, created_by: requestedOwner } }); return command.request_id; };
          } else reply("error", { code: "unavailable", message: "Samtalen kunne ikkje opnast no" });
          break;
        case "create_circle": {
          const circle = { id: createdCircleId, slug: command.payload.slug, name: command.payload.name, created_by: identity, created_at: "2026-01-01T00:00:00Z" };
          createdCircles.set(identity, circle); reply("circle_created", { circle }); break;
        }
        case "create_channel": {
          const requestedOwner = identity;
          const channel = { id: pratId, slug: command.payload.slug, name: command.payload.name, kind: command.payload.kind, circle_id: command.payload.circle_id, created_by: requestedOwner };
          if (holdPrat) {
            holdPrat = false; releasePrat = () => { reply("channel_created", { channel }); return command.request_id; };
          } else reply("channel_created", { channel });
          break;
        }
        case "list_joinable_channels": reply("joinable_channels_listed", { channels: [] }); break;
        case "expand_direct_channel":
          if (holdExpansion) {
            holdExpansion = false;
            const requestedOwner = identity;
            releaseExpansion = () => { reply("direct_channel_expanded", { channel: { id: newDirectId, slug: "expanded-grace", name: "Ny gruppe med Grace", kind: "private", circle_id: null, created_by: requestedOwner } }); return command.request_id; };
          } else reply("error", { code: "unavailable", message: "Utvidinga er ikkje aktiv i denne prøva" });
          break;
        case "load_thread": reply("thread_loaded", { root_message_id: command.payload.root_message_id, messages: [rootMessage] }); break;
        case "accept_invitation": reply("error", { code: "invalid_token", message: "Invitasjonen er ikkje gyldig" }); break;
        case "subscribe_channel": reply("subscription_started", { channel_id: selected, history: options.message && selected === directId ? [rootMessage] : [] }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: selected, summaries: [] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: selected, reactions: [] }); break;
        case "load_recent_messages": reply("messages_loaded", { channel_id: selected, messages: [] }); break;
      }
    });
  });
  return { commands, failNextUsers: () => { usersFailure = true; },
    holdNextUsers: () => { holdUsers = true; }, releaseUsers: async () => { const release = releaseUsers; releaseUsers = undefined; await deliver(release); },
    holdNextPrat: () => { holdPrat = true; },
    releasePrat: async () => { const release = releasePrat; releasePrat = undefined; await deliver(release); },
    holdNextExpansion: () => { holdExpansion = true; },
    releaseExpansion: async () => { const release = releaseExpansion; releaseExpansion = undefined; await deliver(release); },
    holdNextOpen: () => { holdOpen = true; }, releaseOpen: async () => { const release = releaseOpen; releaseOpen = undefined; await deliver(release); },
    releaseChannels: async () => { const release = releaseChannels; releaseChannels = undefined; await deliver(release); },
    releaseCircles: async () => { const release = releaseCircles; releaseCircles = undefined; await deliver(release); },
    switchAccount: (next: string) => { identity = next; emit("hello", { participant_id: identity, signup_ordinal: 1 }); },
    unmatchedDirectory: async () => { await deliver(() => {
      emit("channels_listed", { channels }, "not-an-issued-channel-request");
      emit("circles_listed", { circles: [] }, "not-an-issued-circle-request"); return "not-an-issued-circle-request";
    }); }
  };
}
async function enter(page: Page) {
  await page.goto(`/?participant=${owner}&channel=${directId}`);
  const app = page.locator("#sproyt-react-preview");
  await expect(app.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  return app;
}
async function picker(page: Page) {
  const app = page.locator("#sproyt-react-preview");
  const back = app.getByRole("button", { name: "Samtalar", exact: true });
  if (await back.isVisible()) await back.click();
  await app.getByRole("tab", { name: /^Direktemeldingar/ }).click();
  await app.getByRole("button", { name: "Ny direktemelding", exact: true }).click();
  return destinationSurface(app, "Ny direktemelding", true);
}

test("the compact picker focuses search and opens an existing person conversation without creating another channel", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 320, height: 780 }); await page.emulateMedia({ colorScheme: "dark" });
  const server = await fixture(page); await enter(page);
  expect(server.commands.filter(command => command.type === "list_channel_users").every(command => command.payload?.channel_id === directId)).toBe(true);
  const dialog = await picker(page);
  const search = dialog.getByRole("searchbox", { name: "Finn person", exact: true });
  await expect(search).toBeFocused(); await search.fill("ada_handle");
  const existing = dialog.getByRole("button", { name: "Opne samtale med Ada Lovelace", exact: true });
  await expect(existing).toContainText("@ada_handle"); await expect(existing).toContainText("Opne samtale");
  await expect(dialog.getByRole("button", { name: "Last personlista på nytt", exact: true })).toHaveCount(0);
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath("dm-picker-320-dark.png"), fullPage: true });
  await existing.click(); await expect(dialog).toHaveCount(0);
  expect(new URL(page.url()).searchParams.get("channel")).toBe(directId);
  expect(server.commands.filter(command => command.type === "open_direct_channel")).toHaveLength(0);
  expect(server.commands.filter(command => command.type === "send_message")).toHaveLength(0);
});

test("a failed new conversation preserves the person filter and exposes a local retry on the same row", async ({ page }) => {
  const server = await fixture(page); await enter(page);
  const dialog = await picker(page);
  const search = dialog.getByRole("searchbox", { name: "Finn person", exact: true }); await search.fill("Grace");
  const start = dialog.getByRole("button", { name: "Start samtale med Grace Hopper", exact: true });
  await start.click(); await expect(dialog).toContainText("Samtalen kunne ikkje opnast no");
  await expect(search).toHaveValue("Grace"); await expect(start).toBeEnabled();
  await start.click(); await expect(start).toBeEnabled();
  await expect.poll(() => server.commands.filter(command => command.type === "open_direct_channel").length).toBe(2);
  expect(server.commands.filter(command => command.type === "open_direct_channel").map(command => command.payload?.user_id)).toEqual([grace, grace]);
  expect(server.commands.filter(command => command.type === "send_message")).toHaveLength(0);
});

test("a person read failure uses a small explicit retry without losing the search", async ({ page }) => {
  const server = await fixture(page); await enter(page); server.failNextUsers();
  const dialog = await picker(page);
  const search = dialog.getByRole("searchbox", { name: "Finn person", exact: true }); await search.fill("ada");
  await expect(dialog).toContainText("Kunne ikkje hente personane no");
  await dialog.getByRole("button", { name: "Prøv igjen", exact: true }).click();
  await expect(search).toHaveValue("ada");
  await expect(dialog.getByRole("button", { name: "Opne samtale med Ada Lovelace", exact: true })).toBeVisible();
});

for (const group of [false, true]) test(`${group ? "group" : "one-to-one"} details read only the opened conversation and expose no ordinary channel administration`, async ({ page }) => {
  const server = await fixture(page);
  const selected = group ? groupId : directId;
  await page.goto(`/?participant=${owner}&view=settings&section=community&task=channel&channel=${selected}`);
  const dialog = destinationSurface(page, "Om samtalen", true);
  await expect(dialog.getByRole("heading", { name: group ? "Ada Lovelace, Grace Hopper og meg" : "Ada Lovelace", exact: true })).toBeVisible();
  await expect(dialog).toContainText(group ? "Gruppesamtale" : "Direktemelding");
  await expect(dialog).toContainText("Vanlege kanaltypar og kanalroller gjeld ikkje her");
  await expect(dialog).toContainText("Den opphavlege samtalen og meldingane hennar blir bevarte");
  await expect(dialog.getByRole("region", { name: "Legg til kanalmedlem", exact: true })).toHaveCount(0);
  await expect(dialog.getByLabel("Kanalomtale (Markdown)")).toHaveCount(0);
  await expect(dialog.getByRole("button", { name: "Forlat kanalen", exact: true })).toHaveCount(0);
  const memberReads = server.commands.filter(command => command.type === "list_channel_users");
  expect(memberReads.length).toBeGreaterThanOrEqual(1);
  expect(memberReads.every(command => command.payload?.channel_id === selected)).toBe(true);
  expect(server.commands.filter(command => ["add_channel_member", "expand_direct_channel", "send_message"].includes(command.type))).toHaveLength(0);
});


test("an empty 320px account waits for both correlated directory replies before showing empty actions and the guide", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 320, height: 780 });
  const server = await fixture(page, { empty: true, holdDirectory: true });
  await page.goto(`/?participant=${owner}`);
  const app = page.locator("#sproyt-react-preview");
  await expect.poll(() => server.commands.filter(command => ["list_my_channels", "list_my_circles"].includes(command.type)).length).toBe(2);
  const welcome = app.getByRole("heading", { name: "Velkomen til Sprøyt", exact: true, includeHidden: true });
  await expect(welcome).toHaveCount(0);
  await expect(app.getByText("Du har ingen kretsar enno.", { exact: true })).toHaveCount(0);
  await expect(app.getByText(/Hentar samtalane dine/).first()).toBeAttached();
  await server.unmatchedDirectory(); await expect(welcome).toHaveCount(0);
  await server.releaseChannels(); await expect(welcome).toHaveCount(0);
  await expect(app.getByText("Du har ingen kretsar enno.", { exact: true })).toHaveCount(0);
  await server.releaseCircles(); await expect(welcome).toHaveCount(1);
  const back = app.getByRole("button", { name: "Samtalar", exact: true });
  if (await back.isVisible()) await back.click();
  const nav = app.getByRole("navigation", { name: "Samtalar", exact: true });
  await expect(nav.getByRole("button", { name: "Finn kretsar og kanalar", exact: true })).toBeVisible();
  await expect(nav.getByRole("button", { name: "Bruk invitasjonslenkje eller kode", exact: true })).toBeVisible();
  await expect(nav.getByRole("button", { name: "Kom i gang", exact: true })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath("empty-account-320.png"), fullPage: true });
  expect(server.commands.filter(command => ["create_circle", "create_channel", "open_direct_channel", "send_message"].includes(command.type))).toHaveLength(0);
});

test("the sidebar guide is addressable and a short invitation error never reports acceptance", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1280, height: 900 }); await page.emulateMedia({ colorScheme: "light" });
  const server = await fixture(page); const app = await enter(page);
  const back = app.getByRole("button", { name: "Samtalar", exact: true });
  if (await back.isVisible()) await back.click();
  await app.getByRole("navigation", { name: "Samtalar", exact: true }).getByRole("button", { name: "Kom i gang", exact: true }).click();
  const guide = destinationSurface(app, "Kom i gang", true);
  await expect(guide.getByRole("heading", { name: "Kom i gang med Sprøyt", exact: true })).toBeVisible();
  expect(new URL(page.url()).searchParams.get("section")).toBe("welcome");
  await page.screenshot({ path: testInfo.outputPath("getting-started-1280-light.png"), fullPage: true });
  await guide.getByRole("button", { name: "Bruk invitasjonslenkje eller kode", exact: true }).click();
  const invitation = destinationSurface(app, "Bruk invitasjon", true);
  const token = invitation.getByRole("textbox", { name: "Invitasjonskode eller lenkje", exact: true });
  await token.fill("kort"); await invitation.getByRole("button", { name: "Godta invitasjon", exact: true }).click();
  await expect(invitation).toContainText(/gyldig invitasjonskode|Invitasjonen er ikkje gyldig/);
  await expect(invitation).not.toContainText("Invitasjonen er godteken"); await expect(token).toHaveValue("kort");
  expect(new URL(page.url()).searchParams.get("task")).toBe("accept-invitation");
  expect(server.commands.filter(command => ["create_circle", "create_channel", "send_message"].includes(command.type))).toHaveLength(0);
});

test("a held double-clicked direct open cannot navigate after Back and choosing another conversation", async ({ page }) => {
  const server = await fixture(page); const app = await enter(page);
  const dialog = await picker(page);
  await dialog.getByRole("searchbox", { name: "Finn person", exact: true }).fill("Grace");
  server.holdNextOpen();
  await dialog.getByRole("button", { name: "Start samtale med Grace Hopper", exact: true }).dblclick();
  await expect.poll(() => server.commands.filter(command => command.type === "open_direct_channel").length).toBe(1);
  await page.goBack(); await expect(dialog).toHaveCount(0);
  await app.getByRole("button", { name: /Ada Lovelace, Grace Hopper og meg/ }).click();
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(composer).toBeEnabled(); await composer.fill("Utkast i den valde gruppa");
  expect(new URL(page.url()).searchParams.get("channel")).toBe(groupId);
  await server.releaseOpen();
  await expect(composer).toHaveValue("Utkast i den valde gruppa");
  expect(new URL(page.url()).searchParams.get("channel")).toBe(groupId);
  expect(server.commands.filter(command => command.type === "open_direct_channel")).toHaveLength(1);
  expect(server.commands.filter(command => command.type === "send_message")).toHaveLength(0);
});

test("a retired person read from the first A cannot replace a fresh picker after A-B-A", async ({ page }) => {
  const server = await fixture(page); await enter(page); server.holdNextUsers();
  const old = await picker(page); await expect(old).toContainText("Hentar personar");
  const firstRead = server.commands.filter(command => command.type === "list_users").at(-1)!;
  expect(firstRead.request_id).toBeTruthy();
  server.switchAccount("dm-picker-other-owner"); await expect(old).toHaveCount(0);
  await expect.poll(() => server.commands.filter(command => command.type === "list_my_channels").length).toBeGreaterThanOrEqual(2);
  server.switchAccount(owner);
  await expect.poll(() => server.commands.filter(command => command.type === "list_my_channels").length).toBeGreaterThanOrEqual(3);
  const current = await picker(page);
  await expect(current.getByRole("button", { name: "Opne samtale med Ada Lovelace", exact: true })).toBeVisible();
  await server.releaseUsers();
  await expect(current.getByRole("button", { name: "Opne samtale med Ada Lovelace", exact: true })).toBeVisible();
  await expect(current).not.toContainText("Utdatert person frå første A");
  await page.goBack(); await expect(current).toHaveCount(0);
  await expect(page.getByRole("navigation", { name: "Samtalar", exact: true }).getByRole("button", { name: /Utdatert person frå første A/ })).toHaveCount(0);
});

test("a retired direct-open response from the first A cannot alter the returned A snapshot", async ({ page }) => {
  const server = await fixture(page); const app = await enter(page);
  const old = await picker(page); await old.getByRole("searchbox", { name: "Finn person", exact: true }).fill("Grace");
  server.holdNextOpen(); await old.getByRole("button", { name: "Start samtale med Grace Hopper", exact: true }).click();
  await expect.poll(() => server.commands.filter(command => command.type === "open_direct_channel").length).toBe(1);
  expect(server.commands.find(command => command.type === "open_direct_channel")!.request_id).toBeTruthy();
  server.switchAccount("dm-picker-other-owner"); await expect(old).toHaveCount(0);
  await expect.poll(() => server.commands.filter(command => command.type === "list_my_channels").length).toBeGreaterThanOrEqual(2);
  server.switchAccount(owner);
  await expect.poll(() => server.commands.filter(command => command.type === "list_my_channels").length).toBeGreaterThanOrEqual(3);
  const back = app.getByRole("button", { name: "Samtalar", exact: true });
  if (await back.isVisible()) await back.click();
  await app.getByRole("tab", { name: /^Direktemeldingar/ }).click();
  await app.getByRole("button", { name: /Ada Lovelace, Grace Hopper og meg/ }).click();
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(composer).toBeEnabled(); await composer.fill("Nytt A-utkast");
  await server.releaseOpen();
  await expect(composer).toHaveValue("Nytt A-utkast");
  expect(new URL(page.url()).searchParams.get("channel")).toBe(groupId);
  const navigation = app.getByRole("button", { name: "Samtalar", exact: true });
  if (await navigation.isVisible()) await navigation.click();
  await expect(app.getByRole("navigation", { name: "Samtalar", exact: true }).getByRole("button", { name: /^Grace Hopper$/ })).toHaveCount(0);
});

test("the direct conversation header exposes its scoped member details without loading other DM members", async ({ page }) => {
  const server = await fixture(page); const app = await enter(page);
  await expect.poll(() => server.commands.filter(command => command.type === "list_channel_users").length).toBeGreaterThanOrEqual(1);
  const before = server.commands.filter(command => command.type === "list_channel_users").length;
  expect(server.commands.filter(command => command.type === "list_channel_users").every(command => command.payload?.channel_id === directId)).toBe(true);
  const title = app.locator(".sp-mobile-conversation-title");
  if (await title.isVisible()) await title.click();
  await app.getByRole("button", { name: "Medlemmer i Ada Lovelace", exact: true }).click();
  const details = destinationSurface(app, "Om samtalen", true);
  await expect(details.getByRole("heading", { name: "Ada Lovelace", exact: true })).toBeVisible();
  const reads = server.commands.filter(command => command.type === "list_channel_users");
  expect(reads).toHaveLength(before + 1); expect(reads.every(command => command.payload?.channel_id === directId)).toBe(true);
});


test("a held group-expansion response cannot navigate away after opening a thread in the same channel", async ({ page }) => {
  const server = await fixture(page, { message: true }); const app = await enter(page);
  const input = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await input.fill("Privat utkast @grace"); await input.press("End");
  const suggestion = app.getByRole("option", { name: /Grace Hopper.*@grace_handle.*Ny gruppesamtale/ });
  await expect(suggestion).toBeVisible(); await suggestion.click();
  const confirmation = app.getByRole("dialog", { name: "Ny gruppesamtale", exact: true });
  await expect(confirmation).toContainText("Den gamle direkte samtalen held fram privat");
  server.holdNextExpansion();
  await confirmation.getByRole("button", { name: "Start gruppesamtale", exact: true }).click();
  await expect.poll(() => server.commands.filter(command => command.type === "expand_direct_channel").length).toBe(1);
  const root = app.locator('[data-message-id="00000000-0000-7000-8000-000000009504"]');
  await root.getByRole("button", { name: "Svar i tråd", exact: true }).click();
  const reply = app.getByRole("textbox", { name: "Svar i tråden", exact: true });
  await expect(reply).toBeEnabled(); await reply.fill("Utkast i same tråd");
  const selected = new URL(page.url()); expect(selected.searchParams.get("channel")).toBe(directId);
  expect(selected.searchParams.get("thread")).toBe("00000000-0000-7000-8000-000000009504");
  await server.releaseExpansion();
  await expect(reply).toHaveValue("Utkast i same tråd");
  expect(page.url()).toBe(selected.href);
  expect(server.commands.filter(command => command.type === "send_message")).toHaveLength(0);
});


for (const navigation of ["account-A-B-A", "Back"] as const) test(`a held automatic Prat creation cannot navigate after ${navigation}`, async ({ page }) => {
  const server = await fixture(page); const app = await enter(page);
  const back = app.getByRole("button", { name: "Samtalar", exact: true });
  if (await back.isVisible()) await back.click();
  await app.getByRole("navigation", { name: "Samtalar", exact: true }).getByRole("button", { name: "Kom i gang", exact: true }).click();
  await destinationSurface(app, "Kom i gang", true).getByRole("button", { name: "Finn kretsar og kanalar", exact: true }).click();
  await destinationSurface(app, "Vel krets", true).getByRole("button", { name: "Ny vennekrets", exact: true }).click();
  const creation = destinationSurface(app, "Ny vennekrets", true);
  await creation.getByRole("textbox", { name: "Namn på vennekrets", exact: true }).fill("Krets frå første A");
  server.holdNextPrat(); await creation.getByRole("button", { name: "Opprett vennekrets", exact: true }).click();
  await expect.poll(() => server.commands.filter(command => command.type === "create_channel").length).toBe(1);
  expect(server.commands.find(command => command.type === "create_channel")!.request_id).toBeTruthy();
  if (navigation === "account-A-B-A") {
    let reads = server.commands.filter(command => command.type === "list_my_channels").length;
    server.switchAccount("dm-picker-other-owner");
    await expect.poll(() => server.commands.filter(command => command.type === "list_my_channels").length).toBeGreaterThan(reads);
    reads = server.commands.filter(command => command.type === "list_my_channels").length;
    server.switchAccount(owner);
    await expect.poll(() => server.commands.filter(command => command.type === "list_my_channels").length).toBeGreaterThan(reads);
  } else {
    for (let step = 0; step < 5 && await app.locator("[data-workspace-page], dialog[open]").count(); step++) {
      const previous = page.url(); await page.goBack(); await expect.poll(() => page.url()).not.toBe(previous);
    }
  }
  await expect(app.locator("[data-workspace-page], dialog[open]")).toHaveCount(0);
  if (await back.isVisible()) await back.click();
  await app.getByRole("tab", { name: /^Direktemeldingar/ }).click();
  await app.getByRole("button", { name: /Ada Lovelace, Grace Hopper og meg/ }).click();
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(composer).toBeEnabled(); await composer.fill("Utkastet i den valde samtalen");
  const selected = page.url(); await server.releasePrat();
  await expect(composer).toHaveValue("Utkastet i den valde samtalen"); expect(page.url()).toBe(selected);
  expect(server.commands.filter(command => command.type === "subscribe_channel" && command.payload?.channel_id === pratId)).toHaveLength(0);
  if (navigation === "account-A-B-A") {
    if (await back.isVisible()) await back.click();
    await app.getByRole("tab", { name: /^Kretsar/ }).click();
    await expect(app.locator(`[data-conversation-group="circle:${createdCircleId}"]`)).toHaveCount(1);
    await expect(app.getByRole("button", { name: /^# Prat$/, includeHidden: true })).toHaveCount(0);
  }
  expect(server.commands.filter(command => command.type === "send_message")).toHaveLength(0);
});
