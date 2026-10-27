import { destinationSurface } from "./support/destination-surface";
import { expect, test, type Locator, type Page } from "@playwright/test";

const circle = "00000000-0000-7000-8000-000000009001";
const privateChannel = "00000000-0000-7000-8000-000000009002";
const publicChannel = "00000000-0000-7000-8000-000000009003";
const hiddenPrivateChannel = "00000000-0000-7000-8000-000000009004";
const unknownChannel = "00000000-0000-7000-8000-000000009005";
const agentId = "00000000-0000-7000-8000-000000009011";
const secondAgentId = "00000000-0000-7000-8000-000000009012";
const dialogFor = (page: Page) => destinationSurface(page, "Agentar i Testkrets", true);
const destination = (channel = privateChannel) => `/?participant=agent-settings-owner&view=settings&section=chat-agents&circle=${circle}&agent=${agentId}&panel=overview&channel=${channel}`;

type FixtureOptions = { enabled?: boolean; channelEnabled?: boolean; modelConfigured?: boolean; optionalOutage?: boolean; circleRole?: "owner" | "member" };
async function fixture(page: Page, options: FixtureOptions = {}) {
  let agent = { agent_id: agentId, circle_id: circle, display_name: "Maria", trigger_words: ["hei"], response_phrases: ["Svar venleg"],
    enabled: options.enabled ?? true, revision: 3, worker_available: !options.optionalOutage, memory_enabled: true,
    vision_enabled: !!options.optionalOutage, vision_available: !options.optionalOutage,
    image_generation: options.optionalOutage ? { identity_id: "maria-v1", enabled: true, occasional: false } : null,
    image_generation_available: !options.optionalOutage };
  const second = { ...agent, agent_id: secondAgentId, display_name: "Kanalven", enabled: false };
  let identity = "agent-settings-owner", channelRole = "moderator";
  let circleRole: "owner" | "moderator" | "member" = options.circleRole ?? "owner";
  let circlePresent = true;
  let holdList = false, holdSave = false;
  let releaseList: (() => Promise<void>) | undefined, releaseSave: (() => Promise<void>) | undefined;
  let emit = (_type: string, _payload?: unknown) => {};
  let holdRead = false, releaseRead: (() => Promise<void>) | undefined;
  const circleViews = () => circlePresent ? [[{ id: circle, slug: "agent-settings", name: "Testkrets", created_by: "agent-settings-owner", created_at: "2026-01-01T00:00:00Z" }, circleRole]] : [];
  const channelViews = () => [
    { id: privateChannel, name: "Privat arbeid", kind: "private", role: channelRole },
    { id: publicChannel, name: "Felles prat", kind: "public", role: "member" },
    { id: hiddenPrivateChannel, name: "Privat utan administrasjon", kind: "private", role: "member" }
  ].map(item => ({ ...item, slug: item.id, circle_id: circle, direct_user_id: null, is_direct: false, description: "", last_read_sequence: 0, latest_sequence: 0 }));
  const channelGets: string[] = [], channelOwners: (string | null)[] = [], listGets: number[] = [];
  const mutations: { path: string; method: string; body: Record<string, unknown> }[] = [];
  let nextFailure: 409 | 503 | null = null;
  await page.route(/\/api\/v1\/circles\/[^/]+\/chat-agents(?:\/[^?]+)?(?:\?|$)/, async route => {
    const request = route.request(), path = new URL(request.url()).pathname;
    if (request.method() === "GET") {
      listGets.push(agent.revision);
      const snapshot = structuredClone({ agents: [agent, second], worker_available: options.modelConfigured ?? true,
        vision_available: !options.optionalOutage, image_generation_available: !options.optionalOutage,
        image_identities: [{ id: "maria-v1", label: "Maria" }] });
      if (holdList) {
        holdList = false;
        await new Promise<void>(resolve => { releaseList = async () => { await route.fulfill({ json: snapshot }); resolve(); }; });
        return;
      }
      return route.fulfill({ json: snapshot });
    }
    const body = request.postDataJSON() as Record<string, unknown>;
    mutations.push({ path, method: request.method(), body });
    if (nextFailure) { const status = nextFailure; nextFailure = null; return route.fulfill({ status, body: `Lagringa kunne ikkje stadfestast (${status}).` }); }
    if (body.revision !== agent.revision) return route.fulfill({ status: 409, body: "Utdatert revisjon" });
    const complete = async () => {
      agent = { ...agent, ...body, revision: agent.revision + 1 };
      await route.fulfill({ json: agent });
    };
    if (holdSave) {
      holdSave = false;
      await new Promise<void>(resolve => { releaseSave = async () => { await complete(); resolve(); }; });
      return;
    }
    return complete();
  });
  await page.route(/\/api\/v1\/channels\/([^/]+)\/chat-agents(?:\/[^?]+)?(?:\?|$)/, async route => {
    const request = route.request(), path = new URL(request.url()).pathname;
    const selected = path.split("/")[4]!;
    if (request.method() !== "GET") {
      mutations.push({ path, method: request.method(), body: request.postDataJSON() as Record<string, unknown> });
      return route.fulfill({ status: 403, body: "Denne testen har ikkje gitt tilgang." });
    }
    channelGets.push(selected); channelOwners.push(await request.headerValue("x-sproyt-account"));
    if (selected !== privateChannel && selected !== publicChannel) return route.fulfill({ status: 403, body: "Ingen tilgang" });
    const reply = () => route.fulfill({ json: { access_revision: 1, selection_available: true, agents: [agent, second].map(item => ({
      agent_id: item.agent_id, display_name: item.display_name, agent_enabled: item.enabled, enabled: options.channelEnabled ?? false
    })) } });
    if (holdRead) {
      holdRead = false;
      await new Promise<void>(resolve => { releaseRead = async () => { await reply(); resolve(); }; });
      return;
    }
    return reply();
  });
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    emit = (type, payload) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload }));
    route.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: identity }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: circleViews() }); break;
      case "list_my_channels": reply("channels_listed", { channels: channelViews() }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: command.payload.channel_id, history: [] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: command.payload.channel_id, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: command.payload.channel_id, reactions: [] }); break;
      case "load_recent_messages": reply("messages_loaded", { channel_id: command.payload.channel_id, messages: [] }); break;
    }
    });
  });
  return { channelGets, channelOwners, listGets, mutations,
    holdNextChannelRead: () => { holdRead = true; },
    holdNextListRead: () => { holdList = true; },
    releaseListRead: async () => { const release = releaseList; releaseList = undefined; await release?.(); },
    holdNextSave: () => { holdSave = true; },
    releaseSave: async () => { const release = releaseSave; releaseSave = undefined; await release?.(); },
    setCircleRole: (role: "owner" | "moderator" | "member") => { circleRole = role; emit("circles_changed"); },
    setCirclePresent: (present: boolean) => { circlePresent = present; emit("circles_changed"); },
    releaseChannelRead: async () => { const release = releaseRead; releaseRead = undefined; await release?.(); },
    switchAccount: (next: string) => { identity = next; emit("hello", { participant_id: identity }); },
    setChannelRole: (role: "moderator" | "member") => {
      channelRole = role;
      emit("membership_joined", { membership: { channel_id: privateChannel, user_id: identity, role, last_read_sequence: 0 } });
    },
    failNext: (status: 409 | 503) => { nextFailure = status; },
    externalName: (name: string) => { agent = { ...agent, display_name: name, revision: agent.revision + 1 }; } };
}
async function openList(page: Page) {
  await page.goto(`/?participant=agent-settings-owner&channel=${privateChannel}`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  const conversations = preview.getByRole("button", { name: "Samtalar", exact: true });
  if (await conversations.isVisible()) await conversations.click();
  await preview.getByRole("button", { name: "Val for Testkrets", exact: true }).click();
  await preview.getByRole("button", { name: "Agentar i Testkrets", exact: true }).click();
  const dialog = dialogFor(page);
  await expect(dialog.getByRole("button", { name: "Maria", exact: true })).toBeVisible();
  return dialog;
}
async function part(dialog: Locator, name: string) {
  await dialog.getByRole("button", { name, exact: true }).click();
}
function expectOnlyOptional(body: Record<string, unknown>, optional?: string) {
  for (const field of ["memory_enabled", "vision_enabled", "image_generation", "weather", "sea_temperature", "ferry_port"])
    if (field !== optional) expect(body).not.toHaveProperty(field);
}

test("a quiet agent list opens without channel probes or writes, then addresses one agent and its scope", async ({ page }) => {
  const server = await fixture(page);
  const dialog = await openList(page);
  await expect(dialog.getByRole("button", { name: "Kanalven", exact: true })).toBeVisible();
  expect(server.channelGets).toEqual([]); expect(server.mutations).toEqual([]);
  await dialog.getByRole("button", { name: "Maria", exact: true }).click();
  await expect(dialog).toContainText("Testkrets / Agentar / Maria");
  await expect(dialog).toContainText("Eigar");
  await expect(dialog.getByRole("button", { name: "Slå av Maria", exact: true })).toBeEnabled();
  await expect(dialog.getByLabel("Kanal", { exact: true })).toHaveValue(privateChannel);
  await expect(dialog).toContainText("Ingen kanaltilgang");
  const url = new URL(page.url());
  expect(url.searchParams.get("circle")).toBe(circle); expect(url.searchParams.get("agent")).toBe(agentId);
  expect(url.searchParams.get("panel")).toBe("overview"); expect(url.searchParams.get("channel")).toBe(privateChannel);
  expect(server.channelGets).toEqual([privateChannel]); expect(server.mutations).toEqual([]);
});

test("private channel checks fetch one chosen channel at a time and refuse inaccessible URL contexts", async ({ page }) => {
  const server = await fixture(page, { enabled: false });
  await page.goto(destination());
  const dialog = dialogFor(page);
  await expect(dialog).toContainText("Avslått i kretsen");
  expect(server.channelGets).toEqual([privateChannel]);
  await dialog.getByRole("button", { name: "Endre kanaltilgang", exact: true }).click();
  await expect(dialog).toContainText("Private kanalar har ingen agentar før du vel dei her");
  await expect(dialog.getByRole("checkbox", { name: /Maria/ })).not.toBeChecked();
  await expect(dialog.getByRole("checkbox")).toHaveCount(1);
  expect(server.channelGets).toEqual([privateChannel, privateChannel]); expect(server.mutations).toEqual([]);
  for (const denied of [hiddenPrivateChannel, unknownChannel]) {
    const before = server.channelGets.length;
    await page.goto(destination(denied));
    await expect(dialog).toContainText("Du kan ikkje vise eller endre agenttilgang til den valde kanalen");
    await expect(dialog.getByRole("button", { name: "Endre kanaltilgang", exact: true })).toHaveCount(0);
    expect(server.channelGets).toHaveLength(before);
    expect(server.mutations).toEqual([]);
  }
});

test("behaviour and skill drafts survive Escape and browser Back while each save sends only its own fields", async ({ page }) => {
  const server = await fixture(page);
  await page.goto(destination());
  const dialog = dialogFor(page);
  await dialog.getByRole("button", { name: "Oppførsel", exact: true }).click();
  const name = dialog.getByRole("textbox", { name: "Namn", exact: true });
  await name.fill("Maria lokalt");
  await page.keyboard.press("Escape");
  await expect(dialog).toContainText("uferdig utkast");
  await page.goForward();
  await expect(name).toHaveValue("Maria lokalt");
  await page.goBack();
  await expect(dialog.getByRole("button", { name: "Oppførsel", exact: true })).toBeVisible();
  await part(dialog, "Bilettolking");
  await dialog.getByRole("checkbox", { name: "Tolk bilete i meldingar", exact: true }).check();
  await page.keyboard.press("Escape");
  await part(dialog, "Bilettolking");
  await expect(dialog.getByRole("checkbox", { name: "Tolk bilete i meldingar", exact: true })).toBeChecked();
  await dialog.getByRole("button", { name: "Lagre endringar", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Oppførsel", exact: true })).toBeVisible();
  expect(server.mutations).toHaveLength(1);
  expect(server.mutations[0]!.body).toMatchObject({ display_name: "Maria", trigger_words: ["hei"], response_phrases: ["Svar venleg"], enabled: true, revision: 3, vision_enabled: true });
  expectOnlyOptional(server.mutations[0]!.body, "vision_enabled");
  await dialog.getByRole("button", { name: "Oppførsel", exact: true }).click();
  await expect(name).toHaveValue("Maria lokalt");
  await dialog.getByRole("button", { name: "Lagre endringar", exact: true }).click();
  await expect(dialog.getByRole("heading", { name: "Maria lokalt", exact: true })).toBeVisible();
  expect(server.mutations[1]!.body).toMatchObject({ display_name: "Maria lokalt", revision: 4, enabled: true });
  expectOnlyOptional(server.mutations[1]!.body);
});

for (const status of [409, 503] as const) test(`${status} preserves edits and requires read-back plus an explicit conflict decision`, async ({ page }) => {
  const server = await fixture(page);
  await page.goto(destination());
  const dialog = dialogFor(page);
  await dialog.getByRole("button", { name: "Oppførsel", exact: true }).click();
  const name = dialog.getByRole("textbox", { name: "Namn", exact: true });
  const save = dialog.getByRole("button", { name: "Lagre endringar", exact: true });
  await name.fill("Mitt lokale namn"); server.failNext(status); await save.click();
  await expect(dialog).toContainText(`Lagringa kunne ikkje stadfestast (${status})`);
  await expect(name).toHaveValue("Mitt lokale namn"); await expect(save).toBeDisabled();
  expect(server.mutations).toHaveLength(1);
  server.externalName("Namnet på tenaren");
  await dialog.getByRole("button", { name: "Hent lagra oppsett", exact: true }).click();
  await expect(dialog.getByRole("group", { name: "Endra av andre", exact: true })).toBeVisible();
  await expect(name).toHaveValue("Mitt lokale namn"); await expect(save).toBeDisabled();
  expect(server.listGets).toContain(4); expect(server.mutations).toHaveLength(1);
  if (status === 409) {
    await dialog.getByRole("button", { name: "Behald utkastet mitt", exact: true }).click();
    await expect(name).toHaveValue("Mitt lokale namn");
  } else {
    await dialog.getByRole("button", { name: "Bruk lagra oppsett", exact: true }).click();
    await expect(name).toHaveValue("Namnet på tenaren"); await expect(save).toBeDisabled();
    await dialog.getByRole("textbox", { name: "Stikkord og svarføringar, eitt per linje", exact: true }).fill("Nye korte føringar");
  }
  await save.click();
  await expect(dialog.getByRole("button", { name: "Oppførsel", exact: true })).toBeVisible();
  expect(server.mutations).toHaveLength(2);
  expect(server.mutations[1]!.body).toMatchObject({ revision: 4, display_name: status === 409 ? "Mitt lokale namn" : "Namnet på tenaren" });
  expectOnlyOptional(server.mutations[1]!.body);
});

for (const scenario of ["model", "off", "optional-outage"] as const) test(`channel status separates ${scenario} from access and ordinary text availability`, async ({ page }) => {
  const server = await fixture(page, { enabled: scenario !== "off", channelEnabled: true,
    modelConfigured: scenario !== "model", optionalOutage: scenario === "optional-outage" });
  await page.goto(destination());
  const dialog = dialogFor(page);
  const expected = scenario === "model" ? "Modelloppsett er ikkje aktivert" : scenario === "off" ? "Avslått i kretsen" : "Kanaltilgang er på";
  await expect(dialog).toContainText(expected);
  await expect(dialog).not.toContainText("Ingen kanaltilgang");
  if (scenario === "optional-outage") {
    await expect(dialog).not.toContainText("Modelloppsett er ikkje aktivert");
    await part(dialog, "Bilettolking");
    await expect(dialog).toContainText("Det seier ikkje at vanlege tekstsvar er utilgjengelege");
    await expect(dialog.getByRole("checkbox", { name: "Tolk bilete i meldingar", exact: true })).toBeChecked();
    await dialog.getByRole("button", { name: "Tilbake", exact: true }).click();
    await part(dialog, "Biletskaping");
    await expect(dialog).toContainText("Eit eksisterande val blir bevart, og du kan slå det av");
    await expect(dialog.getByRole("checkbox", { name: "Lag bilete av agenten", exact: true })).toBeChecked();
  }
  expect(server.mutations).toEqual([]);
});

test("dark settings remain inside the viewport at 320px, phone and desktop widths", async ({ page }, testInfo) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await page.setViewportSize({ width: 320, height: 780 });
  const server = await fixture(page);
  const dialog = await openList(page);
  const capture = async (name: string, width: number) => {
    await expect(dialog).toBeVisible();
    expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
    const bounds = await dialog.boundingBox(); expect(bounds).not.toBeNull();
    expect(bounds!.x).toBeGreaterThanOrEqual(0); expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(width);
    await page.screenshot({ path: testInfo.outputPath(name), fullPage: true });
  };
  await capture("agent-list-320-dark.png", 320);
  await dialog.getByRole("button", { name: "Maria", exact: true }).click();
  await expect(dialog).toContainText("Ingen kanaltilgang");
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(dialog.getByRole("button", { name: "Til agentlista", exact: true })).toBeHidden();
  await expect(dialog.locator(".sp-workspace-page-header").getByRole("button")).toBeVisible();
  await capture("agent-overview-390-dark.png", 390);
  await part(dialog, "Vêr");
  await dialog.getByRole("checkbox", { name: "Vêrdata", exact: true }).check();
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(dialog.getByRole("button", { name: "Til agentlista", exact: true })).toBeVisible();
  await capture("agent-weather-1280-dark.png", 1280);
  expect(server.mutations).toEqual([]);
});


test("the channel-menu entry fences pending reads across two administrators and role A-B-A without requiring circle administration", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const server = await fixture(page, { circleRole: "member" });
  await page.goto(`/?participant=agent-settings-owner&channel=${privateChannel}`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  const access = destinationSurface(preview, "Kanaldetaljar: Privat arbeid", true).locator(".sp-agent-channel-access");
  const openAccess = async () => {
    if (!await preview.getByRole("textbox", { name: "Skriv melding", exact: true }).isVisible()) {
      const nav = preview.getByRole("navigation", { name: "Samtalar", exact: true });
      if (!await nav.isVisible()) {
        const show = preview.getByRole("button", { name: "Vis samtalar", exact: true });
        if (await show.isVisible()) await show.click();
        else await preview.getByRole("button", { name: "Samtalar", exact: true }).click();
      }
      const group = nav.locator(`[data-conversation-group="circle:${circle}"]`);
      const disclosure = group.locator(".sp-product-nav-disclosure");
      if (await disclosure.getAttribute("aria-expanded") === "false") await disclosure.click();
      await group.getByRole("button", { name: "# Privat arbeid", exact: true }).click();
    }
    await preview.locator(".sp-mobile-header-channel-actions").getByRole("button", { name: "Kanalval", exact: true }).click();
    await preview.getByRole("group", { name: "Kanalval: Privat arbeid", exact: true })
      .getByRole("button", { name: "Agentar i kanalen", exact: true }).click();
  };
  server.holdNextChannelRead(); await openAccess();
  await expect(access).toContainText("Hentar agentval");
  await expect.poll(() => server.channelOwners).toEqual(["agent-settings-owner"]);
  server.switchAccount("agent-settings-other-owner");
  await expect(access).toHaveCount(0);
  await server.releaseChannelRead();
  await openAccess();
  await expect(access.getByRole("checkbox", { name: "Maria", exact: true })).not.toBeChecked();
  expect(server.channelOwners).toEqual(["agent-settings-owner", "agent-settings-other-owner"]);
  await destinationSurface(preview, "Kanaldetaljar: Privat arbeid", true).getByRole("button", { name: "Lukk", exact: true }).click();
  server.holdNextChannelRead(); await openAccess();
  await expect(access).toContainText("Hentar agentval");
  await expect.poll(() => server.channelOwners.length).toBe(3);
  server.setChannelRole("member");
  await expect(access).toHaveCount(0);
  server.setChannelRole("moderator");
  await expect.poll(() => server.channelOwners.length).toBe(4);
  await expect(access.getByRole("checkbox", { name: "Maria", exact: true })).not.toBeChecked();
  await server.releaseChannelRead();
  await expect(access.getByRole("checkbox", { name: "Maria", exact: true })).not.toBeChecked();
  expect(server.channelOwners).toEqual(["agent-settings-owner", "agent-settings-other-owner", "agent-settings-other-owner", "agent-settings-other-owner"]);
  expect(server.mutations).toEqual([]);
});


test("root circle-role owner-member-owner rejects an old save completion after a new draft starts", async ({ page }) => {
  const server = await fixture(page);
  const dialog = await openList(page);
  await dialog.getByRole("button", { name: "Maria", exact: true }).click();
  await dialog.getByRole("button", { name: "Oppførsel", exact: true }).click();
  const name = dialog.getByRole("textbox", { name: "Namn", exact: true });
  await name.fill("Gamal innsending");
  server.holdNextSave();
  await dialog.getByRole("button", { name: "Lagre endringar", exact: true }).click();
  await expect.poll(() => server.mutations.length).toBe(1);
  server.setCircleRole("member");
  await expect(dialog).toHaveCount(0);
  server.holdNextListRead(); server.setCircleRole("owner");
  await expect(dialog).toContainText("Lastar agentar");
  await expect.poll(() => server.listGets.length).toBe(2);
  await server.releaseListRead();
  await expect(name).toHaveValue("Maria");
  await name.fill("Nyare utkast");
  await server.releaseSave();
  await expect(name).toHaveValue("Nyare utkast");
  await expect(dialog.getByRole("button", { name: "Lagre endringar", exact: true })).toBeEnabled();
  await expect(dialog).not.toContainText("Endringa er lagra");
  expect(new URL(page.url()).searchParams.get("panel")).toBe("behaviour");
  expect(server.mutations).toHaveLength(1);
});

test("a list GET captured before save cannot lower the confirmed revision after client-side remount", async ({ page }) => {
  const server = await fixture(page);
  const dialog = await openList(page);
  await dialog.getByRole("button", { name: "Maria", exact: true }).click();
  await dialog.getByRole("button", { name: "Oppførsel", exact: true }).click();
  await dialog.getByRole("textbox", { name: "Stikkord og svarføringar, eitt per linje", exact: true }).fill("Stadfesta nye føringar");
  server.holdNextSave();
  await dialog.getByRole("button", { name: "Lagre endringar", exact: true }).click();
  await expect.poll(() => server.mutations.length).toBe(1);
  // Traverse the existing UI history; the account-owned workspace survives this unmount.
  for (let step = 0; step < 4 && new URL(page.url()).searchParams.get("section") === "chat-agents"; step++) {
    const previous = page.url(); await page.goBack();
    await expect.poll(() => page.url()).not.toBe(previous);
  }
  await expect(dialog).toHaveCount(0);
  server.holdNextListRead();
  const preview = page.locator("#sproyt-react-preview");
  await preview.getByRole("button", { name: "Val for Testkrets", exact: true }).click();
  await preview.getByRole("button", { name: "Agentar i Testkrets", exact: true }).click();
  await expect(dialog).toContainText("Lastar agentar");
  await expect.poll(() => server.listGets.length).toBe(2);
  expect(server.listGets).toEqual([3, 3]);
  await server.releaseSave();
  await expect.poll(() => server.listGets.includes(4)).toBe(true);
  await expect(dialog.getByRole("button", { name: "Maria", exact: true })).toBeVisible();
  await server.releaseListRead();
  await dialog.getByRole("button", { name: "Maria", exact: true }).click();
  await dialog.getByRole("button", { name: "Oppførsel", exact: true }).click();
  await expect(dialog.getByRole("textbox", { name: "Stikkord og svarføringar, eitt per linje", exact: true })).toHaveValue("Stadfesta nye føringar");
  await dialog.getByRole("textbox", { name: "Namn", exact: true }).fill("Maria etter remount");
  await dialog.getByRole("button", { name: "Lagre endringar", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Oppførsel", exact: true })).toBeVisible();
  expect(server.mutations).toHaveLength(2);
  expect(server.mutations[1]!.body).toMatchObject({ revision: 4, display_name: "Maria etter remount", response_phrases: ["Stadfesta nye føringar"] });
});


for (const transition of ["moderator", "absent"] as const) test(`pending drafts recover after the circle scope becomes ${transition} without accepting the old completion`, async ({ page }) => {
  const server = await fixture(page);
  const dialog = await openList(page);
  await dialog.getByRole("button", { name: "Maria", exact: true }).click();
  await dialog.getByRole("button", { name: "Oppførsel", exact: true }).click();
  const name = dialog.getByRole("textbox", { name: "Namn", exact: true });
  const save = dialog.getByRole("button", { name: "Lagre endringar", exact: true });
  await name.fill("Utkast over endra tilgang");
  server.holdNextSave(); await save.click();
  await expect.poll(() => server.mutations.length).toBe(1);
  if (transition === "absent") {
    server.setCirclePresent(false);
    await expect(dialog).toHaveCount(0);
    server.holdNextListRead(); server.setCirclePresent(true);
  } else {
    server.holdNextListRead(); server.setCircleRole("moderator");
  }
  await expect(dialog).toContainText("Lastar agentar");
  await expect.poll(() => server.listGets.length).toBe(2);
  await server.releaseSave();
  await server.releaseListRead();
  await expect(name).toHaveValue("Utkast over endra tilgang");
  await expect(name).toBeEnabled(); await expect(save).toBeEnabled();
  await expect(dialog).not.toContainText("Endringa er lagra");
  expect(server.mutations).toHaveLength(1);
});
