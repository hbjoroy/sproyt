import { expect, test, type Page } from "@playwright/test";

const circle = "00000000-0000-7000-8000-000000002711";
const channel = "00000000-0000-7000-8000-000000002712";
const id = (n: number) => `00000000-0000-7000-8000-${String(27120 + n).padStart(12, "0")}`;
async function fixture(page: Page, available = true) {
  const agents = Array.from({ length: 7 }, (_, index) => ({ agent_id: id(index), circle_id: circle,
    display_name: index === 0 ? "Verten" : index === 6 ? "Avslått agent" : `Fagven ${index}`,
    trigger_words: ["hei"], response_phrases: ["Svar kort"], enabled: index !== 6, revision: 3, worker_available: true }));
  let policy = { available, revision: 0, target_agent_ids: [] as string[], guidance: "", agent_revision: 3 };
  const updates: Record<string, unknown>[] = [];
  const normalUpdates: Record<string, unknown>[] = [];
  const sourceReads: number[] = [];
  let fail = true;
  await page.route(/\/api\/v1\/circles\/[^/]+\/chat-agents(?:\/[^?]+)?(?:\?|$)/, async route => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/handoff")) {
      if (route.request().method() === "GET") return route.fulfill({ json: policy });
      const body = route.request().postDataJSON(); updates.push(body);
      if (fail) { fail = false; return route.fulfill({ status: 409, body: "Oppsettet er endra. Prøv igjen." }); }
      policy = { ...policy, ...body, revision: policy.revision + 1, agent_revision: policy.agent_revision + 1 };
      return route.fulfill({ json: policy });
    }
    if (route.request().method() === "GET") {
      agents[0]!.revision = policy.agent_revision;
      sourceReads.push(agents[0]!.revision);
      return route.fulfill({ json: { agents, worker_available: true } });
    }
    const body = route.request().postDataJSON(); normalUpdates.push(body);
    return route.fulfill({ json: { ...agents[0], ...body, revision: Number(body.revision) + 1 } });
  });
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => route.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: "owner" }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [[{ id: circle, slug: "handoff", name: "Agentkrets", created_by: "owner", created_at: "2026-01-01T00:00:00Z" }, "owner"]] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, slug: "prat", name: "Prat", kind: "public", circle_id: circle, direct_user_id: null, is_direct: false, description: "", role: "owner", last_read_sequence: 0, latest_sequence: 0 }] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: [] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
      case "load_recent_messages": reply("messages_loaded", { channel_id: channel, messages: [] }); break;
    }
  }));
  return { updates, normalUpdates, sourceReads };
}

async function open(page: Page) {
  await page.goto(`/?participant=handoff-owner&channel=${channel}`);
  const preview = page.locator("#sproyt-react-preview");
  await expect(preview.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  const conversations = preview.getByRole("button", { name: "Samtalar", exact: true });
  if (await conversations.isVisible()) await conversations.click();
  await preview.getByRole("button", { name: "Val for Agentkrets", exact: true }).click();
  await preview.getByRole("button", { name: "Agentar i Agentkrets", exact: true }).click();
  const dialog = preview.getByRole("dialog", { name: "Agentar i Agentkrets", exact: true });
  await dialog.getByRole("button", { name: "Verten", exact: true }).click();
  await dialog.getByRole("button", { name: "Samarbeid", exact: true }).click();
  return dialog;
}

test("agent handoff bounds recipients, preserves failed drafts, and refreshes source revision before normal editing", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ colorScheme: "dark" });
  const { updates, normalUpdates, sourceReads } = await fixture(page);
  const dialog = await open(page);
  const policy = dialog.getByRole("region", { name: "Videresending frå Verten", exact: true });
  await expect(policy.getByRole("checkbox")).toHaveCount(5);
  await expect(policy.getByRole("checkbox", { name: "Verten", exact: true })).toHaveCount(0);
  await expect(policy.getByRole("checkbox", { name: "Avslått agent", exact: true })).toHaveCount(0);
  for (let n = 1; n <= 4; n++) await policy.getByRole("checkbox", { name: `Fagven ${n}`, exact: true }).check();
  await expect(policy.getByRole("checkbox", { name: "Fagven 5", exact: true })).toBeDisabled();
  const guidance = policy.getByRole("textbox", { name: "Når bør agenten sende vidare? (valfritt)", exact: true });
  await guidance.fill("Spør fagvenen når ein treng eit oppslag.");
  const save = policy.getByRole("button", { name: "Lagre videresending", exact: true });
  await save.click();
  await expect(policy).toContainText("Oppsettet er endra");
  await expect(guidance).toHaveValue("Spør fagvenen når ein treng eit oppslag.");
  await expect(policy.getByRole("checkbox", { checked: true })).toHaveCount(4);
  await expect(save).toBeDisabled();
  await policy.getByRole("button", { name: "Hent lagra oppsett", exact: true }).click();
  await expect(guidance).toHaveValue("Spør fagvenen når ein treng eit oppslag.");
  await expect(policy.getByRole("checkbox", { checked: true })).toHaveCount(4);
  await save.click();
  await expect(policy).toHaveCount(0);
  expect(updates).toHaveLength(2);
  expect(updates[0]).toEqual(updates[1]);
  expect(updates[1]).toMatchObject({ revision: 0, target_agent_ids: [id(1), id(2), id(3), id(4)] });
  expect(sourceReads).toContain(4);
  await dialog.getByRole("button", { name: "Oppførsel", exact: true }).click();
  await dialog.getByRole("textbox", { name: "Stikkord og svarføringar, eitt per linje", exact: true }).fill("Svar kort og presist");
  await dialog.getByRole("button", { name: "Lagre endringar", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Oppførsel", exact: true })).toBeVisible();
  expect(normalUpdates[0]).toMatchObject({ revision: 4, response_phrases: ["Svar kort og presist"] });
  for (const skill of ["memory_enabled", "vision_enabled", "image_generation", "weather", "sea_temperature", "ferry_port"]) expect(normalUpdates[0]).not.toHaveProperty(skill);
  expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
});

test("handoff remains unconfigured when unavailable and leaves ordinary agent management usable", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const { updates } = await fixture(page, false);
  const dialog = await open(page);
  await expect(dialog).toContainText("Videresending er ikkje tilgjengeleg");
  await expect(dialog.getByRole("button", { name: "Lagre videresending", exact: true })).toBeDisabled();
  await dialog.getByRole("button", { name: "Avbryt videresending", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Oppførsel", exact: true })).toBeEnabled();
  await page.keyboard.press("Escape");
  await expect(dialog.getByRole("button", { name: "Ny agent", exact: true })).toBeEnabled();
  expect(updates).toHaveLength(0);
});
