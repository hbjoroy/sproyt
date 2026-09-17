import { expect, test, type Page } from "@playwright/test";

async function installInboxFixture(page: Page) {
  await page.addInitScript(() => {
    type Wire = { protocol: string; type: string; request_id?: string; payload?: Record<string, unknown> };
    const fixture: {
      socket?: WebSocket; enabled: boolean; participantId: string; channel?: Record<string, unknown>;
      source?: Record<string, unknown>; mentionRead: boolean; tasks: Record<string, unknown>[]; failReadOnce: boolean;
      emit?: (event: Wire) => void; activate?: () => void;
    } = { enabled: false, participantId: "", mentionRead: false, tasks: [], failReadOnce: true };
    const NativeWebSocket = window.WebSocket;
    const emit = (event: Wire) => fixture.socket?.dispatchEvent(new MessageEvent("message", { data: JSON.stringify(event) }));
    fixture.emit = emit;
    fixture.activate = () => {
      fixture.enabled = true;
      document.dispatchEvent(new Event("visibilitychange"));
    };
    window.WebSocket = class extends NativeWebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols);
        fixture.socket = this;
        this.addEventListener("message", event => {
          try {
            const frame = JSON.parse(String(event.data));
            if (frame.type === "hello") fixture.participantId = frame.payload.participant_id;
            if (frame.type === "channels_listed") fixture.channel = frame.payload.channels[0];
            if (frame.type === "message_accepted" && frame.payload.message.body.includes("Kjeldemelding")) fixture.source = frame.payload.message;
          } catch { /* The application owns validation of actual frames. */ }
        });
      }
      send(data: string | ArrayBufferLike | Blob | ArrayBufferView) {
        if (typeof data !== "string" || !fixture.enabled) { super.send(data); return; }
        const command = JSON.parse(data);
        const source = fixture.source;
        const mention = source ? { read: fixture.mentionRead, channel_name: String(fixture.channel?.name ?? "general"), message: source } : null;
        if (command.type === "list_my_channels" && fixture.channel) {
          setTimeout(() => emit({ protocol: "sproyt.chat.v1", type: "channels_listed", request_id: command.request_id, payload: {
            channels: [{ ...fixture.channel, last_read_sequence: 0, latest_sequence: 3 }]
          } }), 20); return;
        }
        if (command.type === "list_mentions") {
          setTimeout(() => emit({ protocol: "sproyt.chat.v1", type: "mentions_listed", request_id: command.request_id,
            payload: { mentions: mention ? [mention] : [] } }), 20); return;
        }
        if (command.type === "mark_mention_read") {
          if (fixture.failReadOnce) {
            fixture.failReadOnce = false;
            setTimeout(() => emit({ protocol: "sproyt.chat.v1", type: "error", request_id: command.request_id,
              payload: { code: "temporary", message: "Mellombels omtaleproblem" } }), 20); return;
          }
          fixture.mentionRead = true;
          setTimeout(() => emit({ protocol: "sproyt.chat.v1", type: "mention_read", request_id: command.request_id,
            payload: { message_id: source?.id } }), 20); return;
        }
        if (command.type === "list_tasks") {
          setTimeout(() => emit({ protocol: "sproyt.chat.v1", type: "tasks_listed", request_id: command.request_id,
            payload: { tasks: fixture.tasks } }), 20); return;
        }
        if (command.type === "create_task") {
          const task = { id: "preview-task", source_message_id: command.payload.source_message_id,
            channel_id: source?.channel_id, channel_name: String(fixture.channel?.name ?? "general"),
            assignee_id: fixture.participantId, created_by: fixture.participantId,
            process_link_id: command.payload.process_link_id, title: command.payload.title, status: "open",
            created_at: new Date().toISOString(), completed_at: null };
          fixture.tasks = [task];
          setTimeout(() => emit({ protocol: "sproyt.chat.v1", type: "task_created", request_id: command.request_id,
            payload: { task } }), 20); return;
        }
        if (command.type === "set_task_done") {
          const current = fixture.tasks[0];
          const task = { ...current, status: command.payload.done ? "done" : "open",
            completed_at: command.payload.done ? new Date().toISOString() : null };
          fixture.tasks = [task];
          setTimeout(() => emit({ protocol: "sproyt.chat.v1", type: "task_updated", request_id: command.request_id,
            payload: { task } }), 20); return;
        }
        super.send(data);
      }
    };
    Object.assign(window, { __inboxFixture: fixture });
  });
}

test("React inbox handles unread, mentions and tasks while preserving the conversation draft", async ({ page }) => {
  await installInboxFixture(page);
  await page.goto("/?participant=preview-inboxes&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  await composer.fill("Kjeldemelding for omtale"); await composer.press("Enter");
  await expect(preview.locator("[data-message-id]").filter({ hasText: "Kjeldemelding for omtale" })).toBeVisible();
  await composer.fill("utkastet skal overleve innboksen");
  await page.evaluate(() => (window as typeof window & { __inboxFixture: { activate(): void } }).__inboxFixture.activate());

  await preview.getByRole("button", { name: "Innboks og oppgåver", exact: true }).click();
  let dialog = preview.getByRole("dialog", { name: "Innboks og oppgåver" });
  await expect(dialog.getByText("3 uleste meldingar i 1 samtale")).toBeVisible();
  await dialog.getByRole("button", { name: "# general, 3 uleste meldingar" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(composer).toHaveValue("utkastet skal overleve innboksen");

  await preview.getByRole("button", { name: "Innboks og oppgåver", exact: true }).click();
  dialog = preview.getByRole("dialog", { name: "Innboks og oppgåver" });
  await dialog.getByRole("button", { name: "Omtalar" }).click();
  const mention = dialog.getByRole("article").filter({ hasText: "Kjeldemelding for omtale" });
  await expect(mention).toContainText("Ulest");
  await mention.getByRole("button", { name: "Marker lesen" }).click();
  await expect(mention).toContainText("Mellombels omtaleproblem");
  await mention.getByRole("button", { name: "Marker lesen" }).click();
  await expect(mention).toContainText("Lesen");

  await mention.getByRole("button", { name: "Lag oppgåve" }).click();
  await mention.getByLabel("Oppgåvetittel").fill("Følg opp omtalen");
  await mention.getByLabel("Heart-prosess-ID (valfritt)").fill("prosess-42");
  await mention.getByRole("button", { name: "Lagre oppgåve" }).click();
  const task = dialog.getByRole("article").filter({ hasText: "Følg opp omtalen" });
  await expect(task).toContainText("Heart prosess-42");
  await task.getByRole("button", { name: "Ferdig" }).click();
  await expect(task.getByRole("button", { name: "Opne igjen" })).toBeVisible();
  await task.getByRole("button", { name: "Opne igjen" }).click();
  await expect(task.getByRole("button", { name: "Ferdig" })).toBeVisible();

  await dialog.getByRole("button", { name: "Omtalar" }).click();
  await mention.getByRole("button", { name: "Opne kjelda" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(preview.locator(".sp-channel-pane").getByText("Kjeldemelding for omtale", { exact: true })).toBeVisible();
  await expect(composer).toHaveValue("utkastet skal overleve innboksen");
});

test("closing the React inbox restores focus and keeps an open thread draft", async ({ page }) => {
  await page.goto("/?participant=preview-inbox-focus&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  await composer.fill("Kjeldemelding med tråd"); await composer.press("Enter");
  const root = preview.locator("[data-message-id]").filter({ hasText: "Kjeldemelding med tråd" });
  await root.getByRole("button", { name: "Svar i tråd" }).click();
  const reply = preview.locator(".sp-thread-pane").getByRole("textbox", { name: "Svar i tråden" });
  await reply.fill("eit usendt trådutkast");
  const trigger = preview.getByRole("button", { name: "Innboks og oppgåver", exact: true });
  await trigger.click();
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  await expect(reply).toHaveValue("eit usendt trådutkast");
});
