import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";

test.setTimeout(45_000);
const circle = "00000000-0000-7000-8000-000000003101";
const channel = "00000000-0000-7000-8000-000000003102";
const agent = "00000000-0000-7000-8000-000000003103";
const otherAgent = "00000000-0000-7000-8000-000000003104";
const note = "00000000-0000-7000-8000-000000003105";
const source = "00000000-0000-7000-8000-000000003106";

async function fixture(page: Page) {
  let socket: WebSocketRoute;
  let identity = "member";
  let reject = false;
  let held: (() => Promise<void>) | undefined;
  let hold = false;
  let failRead = false;
  let memory = { circle_id: circle, agent_id: agent, enabled: false, agent_enabled: true,
    collection_available: false, collection_started_at: null, revision: 1, memory_epoch: 2,
    history_compactions: 0, unavailable_notes: 2, notes: [{ id: note, channel_id: channel, kind: "preference",
      content: { text: "Eg vil lære gresk.", participant_ids: [] }, origin: "automatic", evidence: "user_stated",
      revision: 1, created_at: 1700000000, updated_at: 1700000000, expires_at: null, source_message_ids: [source] }] };
  const writes: any[] = [];
  await page.route(`**/api/v1/me/circles/${circle}/chat-agents?*`, route => route.fulfill({ json: { agents: [
    { agent_id: agent, display_name: "Maria" }, { agent_id: otherAgent, display_name: "Nikos" }] } }));
  await page.route(`**/api/v1/me/circles/${circle}/chat-agents/*/memory**`, async route => {
    const selected = route.request().url().includes(otherAgent) ? otherAgent : agent;
    if (route.request().method() === "GET") {
      if (failRead) { failRead = false; await route.fulfill({ status: 503, body: "unavailable" }); return; }
      const snapshot = { ...memory, agent_id: selected, notes: selected === otherAgent ? [] : memory.notes };
      if (hold && selected === agent) {
        hold = false;
        await new Promise<void>(resolve => { held = async () => { try { await route.fulfill({ json: snapshot }); } catch { /* aborted fetch */ } resolve(); }; });
      } else await route.fulfill({ json: snapshot });
      return;
    }
    const input = route.request().postDataJSON(); writes.push(input);
    if (reject) { reject = false; await route.fulfill({ status: 409, body: "conflict" }); return; }
    memory = { ...memory, revision: memory.revision + 1, memory_epoch: memory.memory_epoch + 1 };
    if ("enabled" in input) memory.enabled = input.enabled;
    if (input.action === "correct") memory.notes = memory.notes.map(item => ({ ...item, content: { ...item.content, text: input.text }, origin: "user", evidence: "user_confirmed" }));
    if (input.action === "confirm") memory.notes = memory.notes.map(item => ({ ...item, evidence: "user_confirmed" }));
    if (input.action === "forget") memory.notes = [];
    if (input.action === "reset") { memory.notes = []; memory.unavailable_notes = 0; }
    await route.fulfill({ json: memory });
  });
  const emit = (type: string, payload: unknown, request_id?: string) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload, request_id }));
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    socket = route; route.onMessage(data => {
      const command = JSON.parse(String(data));
      const reply = (type: string, payload: unknown = {}) => emit(type, payload, command.request_id);
      switch (command.type) {
        case "hello": reply("hello", { participant_id: identity }); break;
        case "ping": reply("pong"); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [[{ id: circle, slug: "memory", name: "Testkrets", created_by: "owner", created_at: "2026-01-01T00:00:00Z" }, "member"]] }); break;
        case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, slug: "memory", name: "Prat", kind: "private", circle_id: circle, direct_user_id: null, is_direct: false, description: "", role: "member", last_read_sequence: 0, latest_sequence: 0 }] }); break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: [] }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
        case "load_recent_messages": reply("messages_loaded", { channel_id: channel, messages: [] }); break;
      }
    });
  });
  return { writes, conflict: () => { reject = true; }, failRead: () => { failRead = true; }, hold: () => { hold = true; }, release: async () => held?.(),
    switchAccount: () => { identity = "other-member"; emit("hello", { participant_id: identity }); } };
}
async function open(page: Page) {
  const preview = page.locator("#sproyt-react-preview");
  const back = preview.getByRole("button", { name: "Samtalar", exact: true });
  if (await back.isVisible()) await back.click();
  await preview.getByRole("button", { name: "Val for Testkrets", exact: true }).click();
  await preview.getByRole("button", { name: "Mitt agentminne i Testkrets", exact: true }).click();
  return preview.getByRole("dialog", { name: "Mitt agentminne", exact: true });
}
test("ordinary member controls memory with conflict recovery and compact themed layout", async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => localStorage.setItem("sproyt.theme.v1", "light"));
  const server = await fixture(page);
  await page.goto(`/?participant=memory-member&channel=${channel}`);
  await expect(page.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  const dialog = await open(page);
  await expect(dialog).toContainText("Eg vil lære gresk.");
  await expect(dialog).toContainText("Læring er ikkje aktivert enno");
  await expect(dialog).toContainText("2 notat er utilgjengelege");
  await page.screenshot({ path: testInfo.outputPath("memory-mobile-light.png") });
  await dialog.getByRole("checkbox", { name: "Tillat minne om meg" }).click();
  await expect(dialog.getByRole("checkbox")).toBeChecked();
  await expect(dialog).toContainText("Minnevalet er lagra");
  await dialog.getByRole("button", { name: "Stadfest", exact: true }).click();
  await expect(dialog).toContainText("Stadfesta av deg");
  await dialog.getByRole("button", { name: "Rett", exact: true }).click();
  const draft = dialog.getByRole("textbox", { name: "Rett minnenotatet" });
  await draft.fill("🌴".repeat(257));
  await expect(dialog.getByRole("button", { name: "Lagre retting" })).toBeDisabled();
  await draft.fill("Eg føretrekk nynorsk og litt gresk.");
  server.conflict();
  await dialog.getByRole("button", { name: "Lagre retting" }).click();
  await expect(dialog).toContainText("ei anna eining");
  await expect(draft).toHaveValue("Eg føretrekk nynorsk og litt gresk.");
  await dialog.getByRole("button", { name: "Hent minnet på nytt" }).click();
  await expect(draft).toHaveValue("Eg føretrekk nynorsk og litt gresk.");
  await dialog.getByRole("button", { name: "Lagre retting" }).click();
  await expect(dialog).toContainText("Stadfesta av deg");
  await dialog.locator("summary").click();
  await expect(dialog.getByRole("link", { name: "Kjeldemelding 1" })).toHaveAttribute("href", `/?channel=${channel}&message=${source}`);
  const bounds = await dialog.boundingBox();
  expect(bounds!.x).toBeGreaterThanOrEqual(0); expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(390);
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await dialog.getByRole("button", { name: "Gløym", exact: true }).click();
  await dialog.getByRole("button", { name: "Avbryt", exact: true }).click();
  await expect(dialog).toContainText("Eg føretrekk nynorsk");
  await dialog.getByRole("button", { name: "Gløym", exact: true }).click();
  await dialog.getByRole("button", { name: "Ja, gløym" }).click();
  await expect(dialog).toContainText("Ingen synlege minnenotat");
  await dialog.getByRole("button", { name: "Nullstill mitt minne hos Maria" }).click();
  await dialog.getByRole("button", { name: "Ja, gløym" }).click();
  await expect(dialog).not.toContainText("2 notat er utilgjengelege");
  await dialog.getByRole("button", { name: "Lukk", exact: true }).click();
  await expect(page.getByRole("button", { name: "Val for Testkrets", exact: true })).toBeFocused();
  await page.locator("#sproyt-react-preview").getByRole("button", { name: "Meny", exact: true }).click();
  await page.locator("#sproyt-react-preview").getByRole("button", { name: "Byt tema", exact: true }).click();
  const darkDialog = await open(page);
  await expect(darkDialog).toContainText("Ingen synlege minnenotat");
  await page.screenshot({ path: testInfo.outputPath("memory-mobile-dark.png") });
  await darkDialog.getByRole("checkbox").click();
  await expect(darkDialog.getByRole("checkbox")).not.toBeChecked();
  await page.keyboard.press("Escape");
  await expect(darkDialog).toHaveCount(0);
  expect(server.writes.map(write => write.action ?? "choice")).toEqual(["choice", "confirm", "correct", "correct", "forget", "reset", "choice"]);
});
test("late memory response cannot cross agent, close or account boundaries", async ({ page }) => {
  const server = await fixture(page);
  await page.goto(`/?participant=memory-member&channel=${channel}`);
  await expect(page.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  server.hold(); const dialog = await open(page);
  await expect(dialog).toContainText("Hentar minnet");
  await dialog.getByLabel("Agent", { exact: true }).selectOption(otherAgent);
  await expect(dialog).toContainText("Ingen synlege minnenotat");
  await server.release();
  await expect(dialog).not.toContainText("Eg vil lære gresk");
  await dialog.getByLabel("Agent", { exact: true }).selectOption(agent);
  await expect(dialog).toContainText("Eg vil lære gresk");
  server.hold(); await dialog.getByRole("button", { name: "Hent minnet på nytt" }).click();
  await expect(dialog).toContainText("Hentar minnet");
  await dialog.getByRole("button", { name: "Lukk", exact: true }).click();
  await server.release();
  await expect(dialog).toHaveCount(0);
  await open(page);
  await expect(dialog).toContainText("Eg vil lære gresk");
  server.hold(); await dialog.getByRole("button", { name: "Hent minnet på nytt" }).click();
  await expect(dialog).toContainText("Hentar minnet");
  server.switchAccount();
  await expect(dialog).toHaveCount(0);
  await server.release();
  await expect(page.getByText("Eg vil lære gresk", { exact: true })).toHaveCount(0);
});
test("failed read exposes retry without offering mutations on an unknown snapshot", async ({ page }) => {
  const server = await fixture(page);
  await page.goto(`/?participant=memory-member&channel=${channel}`);
  await expect(page.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  server.failRead(); const dialog = await open(page);
  await expect(dialog).toContainText("Kunne ikkje hente eller lagre minnet");
  await expect(dialog.getByRole("checkbox")).toHaveCount(0);
  await dialog.getByRole("button", { name: "Hent minnet på nytt" }).click();
  await expect(dialog).toContainText("Eg vil lære gresk");
  expect(server.writes).toEqual([]);
});
