import { destinationSurface } from "./support/destination-surface";
import { expect, test, type Page, type Locator } from "@playwright/test";

const circle = "00000000-0000-7000-8000-000000002121";
const channel = "00000000-0000-7000-8000-000000002122";
const agentId = "00000000-0000-7000-8000-000000002123";
async function fixture(page: Page, imageMode = false) {
  let agent = { agent_id: agentId, circle_id: circle, display_name: "Vêrven", trigger_words: ["vêr"], response_phrases: ["Svar kort"],
    enabled: false, revision: 3, worker_available: false, vision_enabled: false, vision_available: true,
    image_generation: null, image_generation_available: true,
    weather: imageMode ? null : { location: "Bjorøy", latitude: 60.322, longitude: 5.199 } };
  let fail = true;
  const updates: any[] = [];
  const creations: any[] = [];
  const createdAgents: typeof agent[] = [];
  await page.route(/\/api\/v1\/circles\/[^/]+\/chat-agents(?:\/[^?]+)?(?:\?|$)/, async route => {
    if (route.request().method() === "GET") { await route.fulfill({ json: { agents: [agent, ...createdAgents], worker_available: true, vision_available: agent.vision_available,
      image_generation_available: agent.image_generation_available, image_identities: [{ id: "maria-v1", label: "Maria" }] } }); return; }
    const input = route.request().postDataJSON();
    if (route.request().method() === "POST") {
      creations.push(input);
      const created = { ...agent, ...input, agent_id: "00000000-0000-7000-8000-000000002124", revision: 1,
        weather: null, vision_enabled: false, image_generation: null };
      createdAgents.push(created);
      return route.fulfill({ json: created });
    }
    updates.push(input);
    if (fail) { fail = false; await route.fulfill({ status: 503, body: "Kunne ikkje lagre. Prøv igjen." }); return; }
    agent = { ...agent, ...input, revision: agent.revision + 1 };
    await route.fulfill({ json: agent });
  });
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => route.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: "owner" }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [[{ id: circle, slug: "weather", name: "Vêrkrets", created_by: "owner", created_at: "2026-01-01T00:00:00Z" }, "owner"]] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, slug: "weather", name: "Vêrprat", kind: "public", circle_id: circle, direct_user_id: null, is_direct: false,
        description: "", role: "owner", last_read_sequence: 0, latest_sequence: 0 }] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: [] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
      case "load_recent_messages": reply("messages_loaded", { channel_id: channel, messages: [] }); break;
    }
  }));
  return { updates, creations, disableVision: () => { agent = { ...agent, vision_available: false }; },
    disableImages: () => { agent = { ...agent, image_generation_available: false }; } };
}

async function openAgents(page: Page) {
  await page.goto(`/?participant=weather-owner&channel=${channel}`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  const conversations = preview.getByRole("button", { name: "Samtalar", exact: true });
  if (await conversations.isVisible()) await conversations.click();
  await preview.getByRole("button", { name: "Val for Vêrkrets", exact: true }).click();
  await preview.getByRole("button", { name: "Agentar i Vêrkrets", exact: true }).click();
  const dialog = destinationSurface(preview, "Agentar i Vêrkrets", true);
  await dialog.getByRole("button", { name: "Vêrven", exact: true }).click();
  return dialog;
}
async function skill(dialog: Locator, name: string) {
  await dialog.getByRole("button", { name, exact: true }).click();
}
async function backToList(_page: Page, dialog: Locator) {
  if (!await dialog.getByRole("button", { name: "Ny agent", exact: true }).isVisible())
    await dialog.getByRole("button", { name: "Til agentlista", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Ny agent", exact: true })).toBeVisible();
}
async function createAgent(page: Page, dialog: Locator) {
  await backToList(page, dialog);
  await dialog.getByRole("button", { name: "Ny agent", exact: true }).click();
  await expect(dialog.getByRole("textbox")).toHaveCount(3);
  await expect(dialog.getByRole("checkbox")).toHaveCount(0);
  await dialog.getByRole("textbox", { name: "Namn", exact: true }).fill("Ny hjelpar");
  await dialog.getByRole("textbox", { name: "Triggerord eller -frasar, eitt per linje", exact: true }).fill("hjelp");
  await dialog.getByRole("textbox", { name: "Stikkord og svarføringar, eitt per linje", exact: true }).fill("Svar kort");
  await dialog.getByRole("button", { name: "Opprett agent", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Slå på Ny hjelpar", exact: true })).toBeDisabled();
}
function onlyPart(input: Record<string, unknown>, part: string) {
  for (const optional of ["memory_enabled", "vision_enabled", "image_generation", "weather", "sea_temperature", "ferry_port"])
    if (optional !== part) expect(input).not.toHaveProperty(optional);
}

test("agent image opt-in requires identity, preserves a failed save and can be disabled during an outage", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ colorScheme: "dark" });
  const { updates, creations, disableImages } = await fixture(page, true);
  const dialog = await openAgents(page);
  await skill(dialog, "Biletskaping");
  const enabled = dialog.getByRole("checkbox", { name: "Lag bilete av agenten", exact: true });
  const occasional = dialog.getByRole("checkbox", { name: "Del eit bilete av og til i samtalen", exact: true });
  const identity = dialog.getByRole("combobox", { name: "Visuell identitet", exact: true });
  const save = dialog.getByRole("button", { name: "Lagre endringar", exact: true });
  await expect(enabled).not.toBeChecked(); await enabled.check();
  await expect(save).toBeDisabled(); await identity.selectOption("maria-v1");
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
  await expect(occasional).not.toBeChecked(); await occasional.check();
  await save.click(); await expect(dialog).toContainText("Kunne ikkje lagre");
  await expect(identity).toHaveValue("maria-v1"); await expect(occasional).toBeChecked();
  await expect(save).toBeDisabled();
  await dialog.getByRole("button", { name: "Hent lagra oppsett", exact: true }).click();
  await expect(identity).toHaveValue("maria-v1"); await expect(occasional).toBeChecked();
  await save.click();
  await expect(dialog.getByRole("button", { name: "Oppførsel", exact: true })).toBeVisible();
  expect(updates[0]).toEqual(updates[1]);
  expect(updates[1]).toMatchObject({ image_generation: { identity_id: "maria-v1", enabled: true, occasional: true }, revision: 3 });
  onlyPart(updates[1], "image_generation");
  await skill(dialog, "Biletskaping");
  await expect(enabled).toBeChecked(); await expect(occasional).toBeChecked();
  await expect(save).toBeDisabled();
  expect(updates).toHaveLength(2);
  disableImages(); await page.reload();
  await expect(enabled).toBeEnabled(); await expect(occasional).toBeDisabled();
  await expect(identity).toBeDisabled();
  await enabled.uncheck(); await save.click();
  await expect(dialog.getByRole("button", { name: "Oppførsel", exact: true })).toBeVisible();
  expect(updates[2]).toMatchObject({ image_generation: { identity_id: "maria-v1", enabled: false, occasional: false }, revision: 4 });
  onlyPart(updates[2], "image_generation");
  await createAgent(page, dialog);
  expect(creations[0]).toEqual({ display_name: "Ny hjelpar", trigger_words: ["hjelp"], response_phrases: ["Svar kort"], enabled: false });
  await skill(dialog, "Biletskaping");
  await expect(enabled).not.toBeChecked(); await expect(enabled).toBeDisabled();
});

test("fixed weather settings preserve edits through failure, reject invalid coordinates and default new locations without GPS", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const { updates, creations, disableVision } = await fixture(page);
  const dialog = await openAgents(page);
  await skill(dialog, "Vêr");
  const location = dialog.getByRole("textbox", { name: "Stad", exact: true });
  const latitude = dialog.getByRole("spinbutton", { name: "Breiddegrad", exact: true });
  const longitude = dialog.getByRole("spinbutton", { name: "Lengdegrad", exact: true });
  const save = dialog.getByRole("button", { name: "Lagre endringar", exact: true });
  const vision = dialog.getByRole("checkbox", { name: "Tolk bilete i meldingar", exact: true });
  await expect(vision).toHaveCount(0);
  await expect(dialog.getByRole("checkbox", { name: "Vêrdata", exact: true })).toBeChecked();
  await expect(location).toHaveValue("Bjorøy"); await expect(latitude).toHaveValue("60.322"); await expect(longitude).toHaveValue("5.199");
  await expect(dialog).toContainText("vi brukar ikkje GPS-posisjonen din");
  await latitude.fill("91"); await expect(save).toBeDisabled();
  await latitude.fill(""); await expect(save).toBeDisabled();
  await latitude.fill("37.085"); await longitude.fill("181"); await expect(save).toBeDisabled();
  await longitude.fill("25.148"); await location.fill(" "); await expect(save).toBeDisabled();
  await location.fill("  Parikia  "); await save.click();
  await expect(dialog).toContainText("Kunne ikkje lagre");
  await expect(location).toHaveValue("  Parikia  "); await expect(latitude).toHaveValue("37.085");
  await expect(save).toBeDisabled();
  await dialog.getByRole("button", { name: "Hent lagra oppsett", exact: true }).click();
  await expect(location).toHaveValue("  Parikia  "); await expect(latitude).toHaveValue("37.085");
  await save.click();
  await expect(dialog.getByRole("button", { name: "Oppførsel", exact: true })).toBeVisible();
  expect(updates).toHaveLength(2); expect(updates[0]).toEqual(updates[1]);
  expect(updates[1]).toMatchObject({ weather: { location: "Parikia", latitude: 37.085, longitude: 25.148 }, revision: 3, enabled: false });
  onlyPart(updates[1], "weather");
  await skill(dialog, "Vêr");
  await expect(location).toHaveValue("Parikia"); await expect(latitude).toHaveValue("37.085");
  await dialog.getByRole("button", { name: "Tilbake", exact: true }).click();
  await skill(dialog, "Bilettolking");
  await expect(vision).not.toBeChecked(); await vision.check(); await save.click();
  await expect(dialog.getByRole("button", { name: "Oppførsel", exact: true })).toBeVisible();
  expect(updates[2]).toMatchObject({ vision_enabled: true, revision: 4, enabled: false });
  onlyPart(updates[2], "vision_enabled");
  await createAgent(page, dialog);
  expect(creations[0]).toEqual({ display_name: "Ny hjelpar", trigger_words: ["hjelp"], response_phrases: ["Svar kort"], enabled: false });
  await skill(dialog, "Vêr");
  await dialog.getByRole("checkbox", { name: "Vêrdata", exact: true }).check();
  await expect(location).toHaveValue("Parikia"); await expect(latitude).toHaveValue("37.085"); await expect(longitude).toHaveValue("25.148");
  await expect(dialog).toContainText("Vêrtenesta er ikkje konfigurert.");
  await dialog.getByRole("button", { name: "Forkast endringar", exact: true }).click();
  await dialog.getByRole("button", { name: "Tilbake", exact: true }).click();
  await skill(dialog, "Bilettolking");
  await expect(vision).not.toBeChecked(); await expect(vision).toBeEnabled();
  await dialog.getByRole("button", { name: "Tilbake", exact: true }).click();
  await backToList(page, dialog);
  await dialog.getByRole("button", { name: "Vêrven", exact: true }).click();
  disableVision(); await page.reload();
  await skill(dialog, "Bilettolking");
  await expect(vision).toBeChecked(); await expect(vision).toBeEnabled();
  await expect(dialog).toContainText("Bilettolking er ikkje tilgjengeleg i dette oppsettet");
  await vision.uncheck(); await expect(vision).toBeDisabled(); await save.click();
  await expect(dialog.getByRole("button", { name: "Oppførsel", exact: true })).toBeVisible();
  expect(updates[3]).toMatchObject({ vision_enabled: false, revision: 5 });
  onlyPart(updates[3], "vision_enabled");
});
