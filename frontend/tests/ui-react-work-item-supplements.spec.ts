import { expect, test, type Page } from "@playwright/test";

const channel = "00000000-0000-7000-8000-000000002081";
const source = "00000000-0000-7000-8000-000000002082";
const taskId = "00000000-0000-7000-8000-000000002083";
const taskMessage = "00000000-0000-7000-8000-000000002084";
const itemId = "00000000-0000-7000-8000-000000002085";
async function fixture(page: Page) {
  let revision = 1;
  let sourceReads = 0;
  const additions: any[] = [];
  const submissions: any[] = [];
  const supplements: any[] = [];
  let completed = false;
  const add = (body: string) => supplements.push({ id: crypto.randomUUID(), actor_name: "Innmeldar", body, created_at: "2026-10-04T20:30:00.000Z" });
  const item = () => ({ id: itemId, source_message_id: source, revision, title: "Skrivefeltet forsvinn", description: "Meldt frå mobilen",
    application_name: "Sprøyt", status: "reviewing", can_supplement: !completed, supplements });
  const task = () => ({ id: taskId, message_id: taskMessage, work_item_id: itemId, revision, title: "Skrivefeltet forsvinn", description: "Meldt frå mobilen",
    application_name: "Sprøyt", status: completed ? "completed" : "pending", process_status: "waiting", delivery_status: "ready", category: "bug", priority: "untriaged",
    decision_status: null, assignee_name: "Behandlar", can_decide: !completed, blocked: false, node_id: "review", can_request_information: true,
    information_request: null, information_response: null, supplements });
  await page.route(/\/work-items\/applications(?:\?|$)/, route => route.fulfill({ json: [] }));
  await page.route(/\/work-items\/source\//, route => {
    sourceReads++;
    if (sourceReads === 1) return route.fulfill({ status: 503, body: "Mellombels feil." });
    return route.fulfill({ json: [item()] });
  });
  await page.route(/\/work-items\/[^/]+\/supplements(?:\?|$)/, route => {
    const body = route.request().postDataJSON(); additions.push(body);
    if (additions.length === 1) return route.fulfill({ status: 503, body: "Mellombels feil. Prøv igjen." });
    if (additions.length === 3) { revision++; add("Ny detalj frå ei anna eining"); return route.fulfill({ status: 409, body: "Saka er endra. Hent ho på nytt." }); }
    revision++; add(body.body); return route.fulfill({ json: item() });
  });
  await page.route(/\/api\/v1\/work-item-tasks\//, route => {
    if (route.request().method() === "POST") {
      submissions.push(route.request().postDataJSON());
      if (submissions.length === 1) { revision++; add("Informasjon medan svaret var uklart"); return route.fulfill({ status: 503, body: "Svaret er uklart. Prøv igjen." }); }
      completed = true; revision++;
    }
    return route.fulfill({ json: task() });
  });
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => route.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: "actor" }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, slug: "supplements", name: "Arbeid", kind: "public", circle_id: null, direct_user_id: null, is_direct: false,
        description: "", role: "member", last_read_sequence: 2, latest_sequence: 2 }] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: [
        { id: source, channel_id: channel, sequence: 1, sender_id: "actor", sender_display_name: "Innmeldar", body: "Skrivefeltet forsvinn", sent_at: "2026-10-04T20:00:00Z" },
        { id: taskMessage, channel_id: channel, sequence: 2, sender_id: "actor", sender_display_name: "Behandlar", body: `[[work-item-task:${taskId}]]`, sent_at: "2026-10-04T20:01:00Z" }
      ] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
      case "load_recent_messages": reply("messages_loaded", { channel_id: channel, messages: [] }); break;
    }
  }));
  return { additions, submissions, sourceReads: () => sourceReads };
}

test("source supplements retry exactly, recover from conflict and require explicit review of polled new information", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const server = await fixture(page);
  await page.goto(`/?participant=supplements&channel=${channel}`);
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(composer).toBeEnabled(); await composer.fill("Samtaleutkastet står");
  const review = preview.locator(".sp-work-item-task");
  await review.locator(".sp-work-item-task-summary").click();
  await review.getByRole("combobox", { name: "Prioritet", exact: true }).selectOption("high");
  await review.getByRole("combobox", { name: "Avgjerd", exact: true }).selectOption("needs_information");
  const question = review.getByRole("textbox", { name: "Spørsmål til innmeldar", exact: true });
  await question.fill("Kva nettlesar?");
  expect(server.sourceReads()).toBe(0);
  const overflow = preview.locator(`[data-message-id="${source}"]`).getByRole("button", { name: "Fleire meldingsval", exact: true });
  const firstLookup = page.waitForResponse(response => response.url().includes("/work-items/source/") && response.status() === 503);
  await overflow.click();
  await (await firstLookup).finished();
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => resolve())));
  const menu = preview.getByRole("dialog", { name: "Meldingsval", exact: true });
  await expect(menu.getByRole("button", { name: "Arbeidssaker", exact: true })).toHaveCount(0);
  expect(server.sourceReads()).toBe(1);
  await menu.getByRole("button", { name: "Lukk meldingsvala", exact: true }).click();
  await overflow.click();
  await menu.getByRole("button", { name: "Arbeidssaker", exact: true }).click();
  const dialog = preview.getByRole("dialog", { name: "Arbeidssaker", exact: true });
  await expect(dialog).toContainText("Registrerte saker frå meldinga");
  await expect(dialog.getByRole("button", { name: "Registrer", exact: true })).toHaveCount(0);
  await dialog.getByRole("button", { name: "Legg til informasjon", exact: true }).click();
  const extra = dialog.getByRole("textbox", { name: "Ny informasjon", exact: true });
  await extra.fill("Også på iPhone");
  await dialog.getByRole("button", { name: "Legg til informasjon", exact: true }).click();
  await expect(dialog).toContainText("Mellombels feil"); await expect(extra).toHaveValue("Også på iPhone"); await expect(extra).toBeDisabled();
  await dialog.getByRole("button", { name: "Prøv same informasjon igjen", exact: true }).click();
  await expect(dialog).toContainText("Informasjonen er lagd til saka");
  expect(server.additions[0]).toEqual(server.additions[1]); expect(server.additions[1].expected_revision).toBe(1);
  await expect(dialog.locator("time")).not.toHaveText("2026-10-04T20:30:00.000Z");
  await dialog.getByRole("button", { name: "Legg til informasjon", exact: true }).click(); await extra.fill("Eit nytt utkast");
  await dialog.getByRole("button", { name: "Legg til informasjon", exact: true }).click();
  await expect(dialog).toContainText("Saka er endra"); await expect(extra).toHaveValue("Eit nytt utkast");
  await dialog.getByRole("button", { name: "Hent saka på nytt", exact: true }).click();
  await expect(dialog).toContainText("Ny detalj frå ei anna eining");
  await dialog.getByRole("button", { name: "Legg til informasjon", exact: true }).click();
  await expect(dialog).toContainText("Informasjonen er lagd til saka");
  expect(server.additions[3].expected_revision).toBe(3); expect(server.additions[3].request_id).not.toBe(server.additions[2].request_id);
  await dialog.getByRole("button", { name: "Lukk saksregistrering", exact: true }).click();
  await expect(review).toContainText("Ny informasjon eller endring", { timeout: 10_000 });
  await expect(question).toHaveValue("Kva nettlesar?"); await expect(review.getByRole("combobox", { name: "Prioritet", exact: true })).toHaveValue("high");
  const send = review.getByRole("button", { name: "Send spørsmål", exact: true });
  await expect(send).toBeDisabled(); expect(server.submissions).toHaveLength(0);
  await review.getByRole("button", { name: "Eg har lese den nye informasjonen", exact: true }).click();
  await send.click(); await expect(review).toContainText("Svaret er uklart");
  await expect(question).toBeDisabled(); await expect(question).toHaveValue("Kva nettlesar?");
  await expect(review).toContainText("Informasjon medan svaret var uklart", { timeout: 10_000 });
  await expect(review.getByRole("button", { name: "Eg har lese den nye informasjonen", exact: true })).toBeDisabled();
  await send.click(); await expect(review.locator(".sp-work-item-task-summary")).toContainText("Fullført");
  expect(server.submissions[0]).toEqual(server.submissions[1]); expect(server.submissions[0].expected_revision).toBe(4);
  await expect(composer).toHaveValue("Samtaleutkastet står");
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});
