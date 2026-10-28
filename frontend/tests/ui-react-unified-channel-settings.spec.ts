import { openChannelMenu } from "./support/channel-menu";
import { expect, test, type Page } from "@playwright/test";
import { destinationSurface } from "./support/destination-surface";

const circleId = "00000000-0000-7000-8000-000000008711";
const channelId = "00000000-0000-7000-8000-000000008712";
const agentId = "00000000-0000-7000-8000-000000008713";
async function setup(page: Page, channelRole: string, circleRole: string, withAgent = false) {
  let reads = 0;
  const writes: unknown[] = [];
  await page.route("**/api/v1/channels/*/chat-agents**", route => {
    if (route.request().method() === "GET") reads++;
    else writes.push(route.request().postDataJSON());
    return route.fulfill({ json: { access_revision: 1, selection_available: true, agents: withAgent ? [{ agent_id: agentId, display_name: "Kanalhjelpar", agent_enabled: true, enabled: false }] : [] } });
  });
  await page.routeWebSocket(/\/ws(?:\?|$)/, socket => socket.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: "channel-settings-owner" }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [[{ id: circleId, slug: "settings", name: "Innstillingskrets", created_by: "channel-settings-owner", created_at: "2026-01-01T00:00:00Z" }, circleRole]] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [{ id: channelId, slug: "settings", name: "Privat kanalflate", kind: "private", circle_id: circleId, direct_user_id: null, is_direct: false, description: "Ei samla kanalflate.", role: channelRole, last_read_sequence: 0, latest_sequence: 0 }] }); break;
      case "list_channel_users": reply("channel_users_listed", { channel_id: channelId, users: [] }); break;
      case "list_circle_users": reply("circle_users_listed", { circle_id: circleId, users: [] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: channelId, history: [] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channelId, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channelId, reactions: [] }); break;
      case "load_recent_messages": reply("messages_loaded", { channel_id: channelId, messages: [] }); break;
    }
  }));
  await page.goto(`/?participant=channel-settings-owner&channel=${channelId}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  await openChannelMenu(preview);
  await preview.getByRole("group", { name: "Kanalval: Privat kanalflate", exact: true })
    .getByRole("button", { name: "Kanaldetaljar", exact: true }).click();
  return { surface: destinationSurface(preview, "Kanaldetaljar: Privat kanalflate", true), reads: () => reads, writes };
}

test("one channel surface keeps members, access and notifications together without implicit agent changes", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const server = await setup(page, "moderator", "member");
  const navigation = server.surface.getByRole("tablist", { name: "Kanalinnstillingar", exact: true });
  for (const name of ["Om", "Medlemmer", "Agentar", "Varsel"]) await expect(navigation.getByRole("tab", { name, exact: true })).toBeVisible();
  expect(server.reads()).toBe(0);
  await navigation.getByRole("tab", { name: "Agentar", exact: true }).click();
  await expect(server.surface).toContainText("Ingen agentar i kretsen enno.");
  expect(server.reads()).toBe(1);
  expect(server.writes).toEqual([]);
  await expect(server.surface.getByRole("button", { name: "Opne agentoppsett for denne kanalen", exact: true })).toHaveCount(0);
  await navigation.getByRole("tab", { name: "Varsel", exact: true }).click();
  await expect(navigation.getByRole("tab", { name: "Varsel", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(server.surface.getByRole("heading", { name: "Dine varsel for denne kanalen", exact: true })).toBeVisible();
  await expect(server.surface.getByRole("heading", { name: "Agentar", exact: true })).toBeHidden();
});

test("circle owner with a private channel member role cannot use private agent access", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const server = await setup(page, "member", "owner");
  await server.surface.getByRole("tablist", { name: "Kanalinnstillingar", exact: true }).getByRole("tab", { name: "Agentar", exact: true }).click();
  expect(server.reads()).toBe(0);
  await expect(server.surface.getByRole("button", { name: "Vis agenttilgang", exact: true })).toHaveCount(0);
  await expect(server.surface.getByRole("button", { name: "Opne agentoppsett for denne kanalen", exact: true })).toHaveCount(0);
});
test("channel section tabs preserve unfinished description and member searches and support keyboard selection", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const { surface } = await setup(page, "owner", "owner");
  const tabs = surface.getByRole("tablist", { name: "Kanalinnstillingar", exact: true });
  const description = surface.getByLabel("Kanalomtale (Markdown)");
  await description.fill("Omtale som ikkje er lagra");
  await tabs.getByRole("tab", { name: "Medlemmer", exact: true }).click();
  await expect(description).toBeHidden();
  await surface.getByRole("searchbox", { name: "Finn kanalmedlem", exact: true }).fill("Søk som skal stå");
  await tabs.getByRole("tab", { name: "Om", exact: true }).click();
  await expect(description).toHaveValue("Omtale som ikkje er lagra");
  await tabs.getByRole("tab", { name: "Om", exact: true }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(tabs.getByRole("tab", { name: "Medlemmer", exact: true })).toBeFocused();
  await expect(tabs.getByRole("tab", { name: "Medlemmer", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(surface.getByRole("searchbox", { name: "Finn kanalmedlem", exact: true })).toHaveValue("Søk som skal stå");
});
test("circle manager opens the named agent directly with the current channel selected", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.route("**/api/v1/circles/*/chat-agents**", route => route.fulfill({ json: {
    agents: [{ agent_id: agentId, circle_id: circleId, display_name: "Kanalhjelpar", trigger_words: ["hei"], response_phrases: [], enabled: true, revision: 1, worker_available: true }], worker_available: true,
  } }));
  const { surface, writes } = await setup(page, "moderator", "owner", true);
  await surface.getByRole("tab", { name: "Agentar", exact: true }).click();
  await surface.getByRole("button", { name: "Opne agentoppsett for Kanalhjelpar", exact: true }).click();
  const agentPage = destinationSurface(page.locator("#sproyt-react-preview"), "Agentar i Innstillingskrets", true);
  await expect(agentPage.getByRole("heading", { name: "Kanalhjelpar", exact: true })).toBeVisible();
  await expect(agentPage.getByLabel("Kanal", { exact: true })).toHaveValue(channelId);
  expect(writes).toEqual([]);
});