import { openChannelMenu } from "./support/channel-menu";
import { readFile } from "node:fs/promises";
import { expect, test, type Page } from "@playwright/test";
import { destinationSurface } from "./support/destination-surface";

const circleId = "00000000-0000-7000-8000-000000008711";
const channelId = "00000000-0000-7000-8000-000000008712";
const agentId = "00000000-0000-7000-8000-000000008713";
async function setup(page: Page, channelRole: string, circleRole: string, withAgent = false, initialHidden = false) {
  await page.route("**/api/v1/me/favorite-circles?**", route => route.fulfill({ json: [] }));
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url), "utf8");
  if (process.env.SPROYT_E2E_EMBEDDED_CLIENT !== "1") await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  let currentRole = channelRole, hidden = initialHidden;
  let failNextHide = false;
  const memberId = "channel-governance-member";
  const roles = new Map([["channel-settings-owner", channelRole], [memberId, "member"]]);
  const governanceWrites: unknown[] = [];
  const governance = () => ({ hidden, members: [...roles].map(([user_id, role]) => ({ user_id, role })) });
  await page.route("**/api/v1/channels/*/governance?**", route => route.fulfill({ json: governance() }));
  await page.route("**/api/v1/channels/*/hidden?**", route => {
    if (failNextHide) { failNextHide = false; return route.fulfill({ status: 503, json: { error: "Prøv igjen seinare" } }); }
    const body = route.request().postDataJSON(); governanceWrites.push(body); hidden = body.hidden;
    return route.fulfill({ json: governance() });
  });
  await page.route("**/api/v1/channels/*/members/*/role?**", route => {
    const body = route.request().postDataJSON(); governanceWrites.push(body);
    const user = decodeURIComponent(route.request().url().split("/").at(-2)!); roles.set(user, body.role);
    if (user === "channel-settings-owner") currentRole = body.role;
    return route.fulfill({ json: governance() });
  });
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
      case "list_my_channels": reply("channels_listed", { channels: [{ id: channelId, slug: "settings", name: "Privat kanalflate", kind: "private", circle_id: circleId, direct_user_id: null, is_direct: false, description: "Ei samla kanalflate.", hidden, role: currentRole, last_read_sequence: 0, latest_sequence: 0 }] }); break;
      case "list_channel_users": reply("channel_users_listed", { channel_id: channelId, users: [
        { id: "channel-settings-owner", display_name: "Du", handle: null, kind: "human", external_provider: null, external_subject: null, created_at: "2026-01-01T00:00:00Z", status_text: "", status_emoji: "", status_expires_at: null },
        { id: memberId, display_name: "Kari", handle: null, kind: "human", external_provider: null, external_subject: null, created_at: "2026-01-01T00:00:00Z", status_text: "", status_emoji: "", status_expires_at: null }
      ] }); break;
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
  return { failHide: () => { failNextHide = true; }, surface: destinationSurface(preview, "Kanaldetaljar: Privat kanalflate", true), reads: () => reads, writes, governanceWrites, revoke: () => { currentRole = "member"; roles.set("channel-settings-owner", "member"); } };
}


test("moderator can hide globally and step down; no authority remains after fresh revocation", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const server = await setup(page, "moderator", "member");
  await server.surface.getByRole("button", { name: "Gøym kanalen for alle", exact: true }).click();
  await expect(server.surface.getByRole("button", { name: "Ta fram kanal", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Gøymde kanalar (1)", exact: true })).toHaveCount(0);
  const circle = page.locator(`[data-conversation-group="circle:${circleId}"]`);
  await expect(circle.getByRole("button", { name: "Andre kanalar (1)", exact: true })).toHaveAttribute("aria-expanded", "true");
  await expect(circle.getByRole("button", { name: "# Privat kanalflate", exact: true })).toBeVisible();
  await server.surface.getByRole("tab", { name: "Medlemmer", exact: true }).click();
  await server.surface.getByRole("button", { name: "Gjer Kari til moderator", exact: true }).click();
  await expect(server.surface.getByRole("button", { name: "Fjern moderatorrettar for Kari", exact: true })).toHaveCount(0);
  await server.surface.getByRole("button", { name: "Tre ut som moderator", exact: true }).click();
  await expect(server.surface.getByRole("button", { name: "Tre ut som moderator", exact: true })).toHaveCount(0);
  await server.surface.getByRole("tab", { name: "Om", exact: true }).click();
  await expect(server.surface.getByRole("button", { name: "Ta fram kanal", exact: true })).toHaveCount(0);
  expect(server.governanceWrites).toEqual([{ hidden: true }, { role: "moderator" }, { role: "member" }]);
});

test("stale directory role cannot authorize controls after focus revalidation", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const server = await setup(page, "moderator", "owner");
  await expect(server.surface.getByRole("button", { name: "Gøym kanalen for alle", exact: true })).toBeVisible();
  server.revoke();
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect(server.surface.getByRole("button", { name: "Gøym kanalen for alle", exact: true })).toHaveCount(0);
  await server.surface.getByRole("tab", { name: "Medlemmer", exact: true }).click();
  await expect(server.surface.getByRole("button", { name: "Gjer Kari til moderator", exact: true })).toHaveCount(0);
  expect(server.governanceWrites).toEqual([]);
});

for (const role of ["owner", "moderator"]) test(`${role} can retry a failed restore without losing the action`, async ({ page }) => {
  const server = await setup(page, role, "member");
  await server.surface.getByRole("button", { name: "Gøym kanalen for alle", exact: true }).click();
  const restore = server.surface.getByRole("button", { name: "Ta fram kanal", exact: true });
  await expect(restore).toBeVisible();
  server.failHide();
  await restore.click();
  await expect(restore).toBeEnabled();
  await restore.click();
  await expect(server.surface.getByRole("button", { name: "Gøym kanalen for alle", exact: true })).toBeVisible();
  expect(server.governanceWrites).toEqual([{ hidden: true }, { hidden: false }]);
  await expect(page.getByRole("button", { name: "Andre kanalar (1)", exact: true })).toHaveCount(0);
});

test("ordinary members cannot restore a channel", async ({ page }) => {
  const server = await setup(page, "member", "owner", false, true);
  await expect(server.surface.getByRole("button", { name: "Ta fram kanal", exact: true })).toHaveCount(0);
  await expect(server.surface.getByRole("button", { name: "Gøym kanalen for alle", exact: true })).toHaveCount(0);
  expect(server.governanceWrites).toEqual([]);
});
