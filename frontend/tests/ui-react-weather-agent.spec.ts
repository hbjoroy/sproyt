import { expect, test, type Page } from "@playwright/test";

const circle = "00000000-0000-7000-8000-000000002121";
const channel = "00000000-0000-7000-8000-000000002122";
const agentId = "00000000-0000-7000-8000-000000002123";
async function fixture(page: Page) {
  let agent = { agent_id: agentId, circle_id: circle, display_name: "Vêrven", trigger_words: ["vêr"], response_phrases: ["Svar kort"],
    enabled: false, revision: 3, worker_available: false, vision_enabled: false, vision_available: true, weather: { location: "Bjorøy", latitude: 60.322, longitude: 5.199 } };
  let fail = true;
  const updates: any[] = [];
  await page.route(/\/api\/v1\/circles\/[^/]+\/chat-agents(?:\/[^?]+)?(?:\?|$)/, async route => {
    if (route.request().method() === "GET") { await route.fulfill({ json: { agents: [agent], worker_available: true, vision_available: agent.vision_available } }); return; }
    const input = route.request().postDataJSON(); updates.push(input);
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
  return { updates, disableVision: () => { agent = { ...agent, vision_available: false }; } };
}

test("fixed weather settings preserve edits through failure, reject invalid coordinates and default new locations without GPS", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const { updates, disableVision } = await fixture(page);
  await page.goto(`/?participant=weather-owner&channel=${channel}`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  await preview.getByRole("button", { name: "Samtalar", exact: true }).click();
  await preview.getByRole("button", { name: "Val for Vêrkrets", exact: true }).click();
  await preview.getByRole("button", { name: "Agentar i Vêrkrets", exact: true }).click();
  const dialog = preview.getByRole("dialog", { name: "Agentar i Vêrkrets", exact: true });
  await dialog.getByRole("button", { name: "Rediger", exact: true }).click();
  const location = dialog.getByRole("textbox", { name: "Stad", exact: true });
  const latitude = dialog.getByRole("spinbutton", { name: "Breiddegrad", exact: true });
  const longitude = dialog.getByRole("spinbutton", { name: "Lengdegrad", exact: true });
  const save = dialog.getByRole("button", { name: "Lagre agent", exact: true });
  const vision = dialog.getByRole("checkbox", { name: "Tolk bilete i meldingar", exact: true });
  await vision.check();
  await expect(dialog.getByRole("checkbox", { name: "Vêrdata", exact: true })).toBeChecked();
  await expect(location).toHaveValue("Bjorøy"); await expect(latitude).toHaveValue("60.322"); await expect(longitude).toHaveValue("5.199");
  await expect(dialog.getByRole("checkbox", { name: "Aktiv", exact: true })).toBeDisabled();
  await expect(dialog).toContainText("vi brukar ikkje GPS-posisjonen din");
  await latitude.fill("91"); await expect(save).toBeDisabled();
  await latitude.fill(""); await expect(save).toBeDisabled();
  await latitude.fill("37.085"); await longitude.fill("181"); await expect(save).toBeDisabled();
  await longitude.fill("25.148"); await location.fill(" "); await expect(save).toBeDisabled();
  await location.fill("  Parikia  "); await save.click();
  await expect(dialog).toContainText("Kunne ikkje lagre");
  await expect(location).toHaveValue("  Parikia  "); await expect(latitude).toHaveValue("37.085");
  await expect(vision).toBeChecked();
  await save.click(); await expect(dialog.getByRole("heading", { name: "Rediger Vêrven", exact: true })).toHaveCount(0);
  expect(updates).toHaveLength(2);
  expect(updates[0]).toEqual(updates[1]);
  expect(updates[1]).toMatchObject({ weather: { location: "Parikia", latitude: 37.085, longitude: 25.148 }, revision: 3, enabled: false, vision_enabled: true });
  await dialog.getByRole("button", { name: "Rediger", exact: true }).click();
  await expect(location).toHaveValue("Parikia"); await expect(latitude).toHaveValue("37.085");
  await expect(vision).toBeChecked();
  await dialog.getByRole("button", { name: "Avbryt", exact: true }).click();
  await dialog.getByRole("button", { name: "Ny agent", exact: true }).click();
  await expect(vision).not.toBeChecked(); await expect(vision).toBeEnabled();
  await dialog.getByRole("checkbox", { name: "Vêrdata", exact: true }).check();
  await expect(location).toHaveValue("Parikia"); await expect(latitude).toHaveValue("37.085"); await expect(longitude).toHaveValue("25.148");
  await expect(dialog.getByRole("checkbox", { name: "Aktiv", exact: true })).toBeDisabled();
  await expect(dialog).toContainText("Vêrtenesta er ikkje klar.");
  await dialog.getByRole("button", { name: "Avbryt", exact: true }).click();
  await dialog.getByRole("button", { name: "Lukk agentar", exact: true }).click();
  disableVision();
  await preview.getByRole("button", { name: "Val for Vêrkrets", exact: true }).click();
  await preview.getByRole("button", { name: "Agentar i Vêrkrets", exact: true }).click();
  await dialog.getByRole("button", { name: "Rediger", exact: true }).click();
  await expect(vision).toBeChecked(); await expect(vision).toBeEnabled();
  await expect(dialog).toContainText("Bilettolking er ikkje tilgjengeleg enno");
  await vision.uncheck(); await expect(vision).toBeDisabled();
  await save.click();
  expect(updates[2]).toMatchObject({ vision_enabled: false, revision: 4 });
});
