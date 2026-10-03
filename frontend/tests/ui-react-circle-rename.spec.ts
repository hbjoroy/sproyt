import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";

test.setTimeout(45_000);
const circleId = "00000000-0000-7000-8000-000000000001";
const channelId = "00000000-0000-7000-8000-000000000002";
const token = "a".repeat(40);
async function fixture(page: Page, role: "owner" | "member" = "owner", holdInvite = false) {
  let name = "Før namnebyte";
  let socket: WebSocketRoute;
  let rejectRename = true;
  let held: (() => void) | undefined;
  const commands: any[] = [];
  const circle = () => ({ id: circleId, slug: "stable-slug", name, created_by: "owner", created_at: "2026-01-01T00:00:00Z" });
  const message = { id: "message-1", channel_id: channelId, sequence: 1, sender_id: "peer", sender_display_name: "Peer", body: `[[invite:${token}]]`, sent_at: "2026-01-01T00:00:00Z" };
  const emit = (type: string, payload?: unknown, request_id?: string) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, ...(payload === undefined ? {} : { payload }), ...(request_id ? { request_id } : {}) }));
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    socket = route;
    route.onMessage(data => {
      const command = JSON.parse(String(data)); commands.push(command);
      const reply = (type: string, payload?: unknown) => emit(type, payload, command.request_id);
      switch (command.type) {
        case "hello": reply("hello", { participant_id: role }); break;
        case "ping": reply("pong"); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [[circle(), role]] }); break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "list_joinable_channels": reply("joinable_channels_listed", { circle_id: circleId, channels: [] }); break;
        case "list_my_channels": reply("channels_listed", { channels: [{ id: channelId, slug: "stable-channel", name: "Prat", kind: "private", circle_id: circleId, direct_user_id: null, is_direct: false, description: "", role, last_read_sequence: 1, latest_sequence: 1 }] }); break;
        case "subscribe_channel": reply("subscription_started", { channel_id: channelId, history: [message] }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channelId, summaries: [] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channelId, reactions: [] }); break;
        case "load_recent_messages": reply("messages_loaded", { channel_id: channelId, messages: [] }); break;
        case "inspect_invitation": {
          const preview = { target: { type: "circle", circle_id: circleId }, circle_name: name, channel_name: null, invited_by: "owner", invited_by_name: "Owner", expires_at: "2027-01-01T00:00:00Z", accepted_count: 0, declined_count: 0, response: null };
          if (holdInvite) { held = () => reply("invitation_inspected", { token, invitation: preview }); holdInvite = false; }
          else reply("invitation_inspected", { token, invitation: preview });
          break;
        }
        case "rename_circle":
          if (rejectRename) { rejectRename = false; reply("error", { code: "unavailable", message: "Prøv igjen" }); }
          else { name = command.payload.name; reply("circle_renamed", { circle: circle() }); emit("circles_changed"); }
          break;
      }
    });
  });
  await page.goto(`/?participant=rename-${role}&channel=${channelId}`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  return { preview, commands, changed: (value: string) => { name = value; emit("circles_changed"); }, release: () => held?.(), close: () => socket.close({ code: 1012, reason: "test reconnect" }) };
}

for (const width of [1280, 390]) test(`owner circle rename is prefilled, cancellable and retains failed input at ${width}px`, async ({ page }) => {
  await page.setViewportSize({ width, height: 844 });
  const { preview, commands } = await fixture(page);
  await preview.getByRole("textbox", { name: "Skriv melding" }).fill("Bevar utkastet");
  if (width === 390) await preview.getByRole("button", { name: "Samtalar", exact: true }).click();
  const open = async (name = "Før namnebyte") => {
    await preview.getByRole("button", { name: `Val for ${name}`, exact: true }).click();
    await preview.getByRole("button", { name: `Endre namn på ${name}`, exact: true }).click();
  };
  await open();
  const dialog = preview.getByRole("dialog", { name: "Endre kretsnamn", exact: true });
  const field = dialog.getByRole("textbox", { name: "Kretsnamn", exact: true });
  await expect(field).toHaveValue("Før namnebyte"); await expect(field).toBeFocused();
  await field.fill("Avbryte"); await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  expect(commands.filter(c => c.type === "rename_circle")).toHaveLength(0);
  await expect(preview.getByRole("button", { name: "Val for Før namnebyte", exact: true })).toBeFocused();
  await open();
  await field.fill("   "); await expect(dialog.getByRole("button", { name: "Lagre namn" })).toBeDisabled();
  await field.fill("Ω".repeat(121)); await expect(dialog.getByRole("button", { name: "Lagre namn" })).toBeDisabled();
  await field.fill("  Ny krets  "); await dialog.getByRole("button", { name: "Lagre namn" }).click();
  await expect(dialog).toContainText("Prøv igjen"); await expect(field).toHaveValue("  Ny krets  ");
  await dialog.getByRole("button", { name: "Lagre namn" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(preview.getByRole("heading", { name: "Ny krets", exact: true })).toBeVisible();
  expect(commands.filter(c => c.type === "rename_circle").map(c => c.payload)).toEqual([{ circle_id: circleId, name: "Ny krets" }, { circle_id: circleId, name: "Ny krets" }]);
  if (width === 390) await preview.getByRole("button", { name: "# Prat", exact: true }).click();
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toHaveValue("Bevar utkastet");
  await page.reload();
  await expect(preview.getByRole("textbox", { name: "Skriv melding" })).toBeEnabled();
  if (width === 390) await preview.getByRole("button", { name: "Samtalar", exact: true }).click();
  await expect(preview.getByRole("heading", { name: "Ny krets", exact: true })).toBeVisible();
});

test("member live and reconnect names refresh without owner action, including a pending invitation preview", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 844 });
  const server = await fixture(page, "member", true);
  await server.preview.getByRole("button", { name: "Val for Før namnebyte", exact: true }).click();
  await expect(server.preview.getByRole("button", { name: /Endre namn på/ })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect.poll(() => server.commands.filter(c => c.type === "inspect_invitation").length).toBeGreaterThan(0);
  server.changed("Namn frå eigaren");
  await expect(server.preview.getByRole("heading", { name: "Namn frå eigaren", exact: true })).toBeVisible();
  server.release();
  const card = server.preview.locator("[data-react-invitation]");
  await expect(card).toContainText("Namn frå eigaren");
  await expect(card).not.toContainText("Før namnebyte");
  await page.reload();
  await expect(server.preview.getByRole("heading", { name: "Namn frå eigaren", exact: true })).toBeVisible();
  await expect(card).toContainText("Namn frå eigaren");
});
