import { expect, test, type Page } from "@playwright/test";

const circle = "00000000-0000-7000-8000-000000002051";
const channel = "00000000-0000-7000-8000-000000002052";
const agent = "00000000-0000-7000-8000-000000002053";

async function fixture(page: Page) {
  let enabled = false;
  let revision = 1;
  let selectionAvailable = true;
  let first = true;
  let rejectPending: (() => Promise<void>) | undefined;
  const updates: unknown[] = [];
  const view = () => ({ access_revision: revision, selection_available: selectionAvailable,
    agents: [{ agent_id: agent, display_name: "Kanalhjelpar", agent_enabled: true, enabled }] });
  await page.route(/\/api\/v1\/channels\/[^/]+\/chat-agents(?:\/[^?]+)?(?:\?|$)/, async route => {
    if (route.request().method() === "GET") { await route.fulfill({ json: view() }); return; }
    const input = route.request().postDataJSON(); updates.push(input);
    if (first) {
      first = false;
      await new Promise<void>(resolve => { rejectPending = async () => {
        await route.fulfill({ status: 409, body: "Valet vart endra på ei anna eining. Hent på nytt." }); resolve();
      }; });
    } else { enabled = input.enabled; revision++; await route.fulfill({ json: view() }); }
  });
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => route.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: "manager" }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [[{ id: circle, slug: "private-agent", name: "Testkrets", created_by: "manager", created_at: "2026-01-01T00:00:00Z" }, "member"]] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, slug: "private-agent", name: "Privat arbeid", kind: "private", circle_id: circle, direct_user_id: null, is_direct: false,
        description: "", role: "moderator", last_read_sequence: 0, latest_sequence: 0 }] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: [] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
      case "load_recent_messages": reply("messages_loaded", { channel_id: channel, messages: [] }); break;
    }
  }));
  return { updates, reject: async () => { await rejectPending?.(); }, externalChange: () => { enabled = true; revision = 2; }, gateOff: () => { selectionAvailable = false; } };
}

async function openAgents(page: Page) {
  const preview = page.locator("#sproyt-react-preview");
  const menu = preview.getByRole("button", { name: "Meny", exact: true });
  if (await menu.getAttribute("aria-expanded") !== "true") await menu.click();
  await preview.locator(".sp-mobile-channel-menu").getByRole("button", { name: "Kanalval", exact: true }).click();
  await preview.getByRole("dialog", { name: "Kanalval: Privat arbeid", exact: true })
    .getByRole("button", { name: "Agentar i kanalen", exact: true }).click();
  return preview.getByRole("dialog", { name: "Agentar i Privat arbeid", exact: true });
}

test("private channel selection keeps confirmed checkbox state during errors, reloads current choices and honours rollout gate at narrow width", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const server = await fixture(page);
  await page.goto(`/?participant=channel-agent-manager&channel=${channel}`);
  const composer = page.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(composer).toBeEnabled();
  await composer.fill("Utkastet skal stå");
  let dialog = await openAgents(page);
  const choice = dialog.getByRole("checkbox", { name: "Kanalhjelpar", exact: true });
  await expect(choice).not.toBeChecked();
  await expect(dialog).toContainText("dei siste 20 minutta");
  const bounds = await dialog.boundingBox();
  expect(bounds!.x).toBeGreaterThanOrEqual(0);
  expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(390);
  await choice.click();
  await expect(choice).toBeDisabled();
  await expect(choice).not.toBeChecked();
  await expect(dialog).toContainText("Lagrar agentval");
  await server.reject();
  await expect(dialog).toContainText("Valet vart endra på ei anna eining");
  await expect(choice).not.toBeChecked();
  await expect(choice).toBeDisabled();
  server.externalChange();
  await dialog.getByRole("button", { name: "Hent vala på nytt", exact: true }).click();
  await expect(choice).toBeChecked();
  await choice.click();
  await expect(choice).toBeEnabled();
  await expect(choice).not.toBeChecked();
  expect(server.updates).toEqual([{ enabled: true, access_revision: 1 }, { enabled: false, access_revision: 2 }]);
  await dialog.getByRole("button", { name: "Lukk kanalagentar", exact: true }).click();
  await expect(composer).toHaveValue("Utkastet skal stå");
  await page.reload();
  await expect(composer).toBeEnabled();
  dialog = await openAgents(page);
  await expect(choice).not.toBeChecked();
  await dialog.getByRole("button", { name: "Lukk kanalagentar", exact: true }).click();
  server.gateOff();
  dialog = await openAgents(page);
  await expect(choice).toBeDisabled();
  await expect(dialog).toContainText(/ikkje.*tilgjengeleg|utrulling|oppgrader/i);
});
