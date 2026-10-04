import { expect, test, type Page } from "@playwright/test";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";

test.use({ serviceWorkers: "allow" });
const channel = "00000000-0000-7000-8000-000000001841";
const otherChannel = "00000000-0000-7000-8000-000000001842";
const requestId = "00000000-0000-7000-8000-000000001843";
const mediaId = "00000000-0000-7000-8000-000000001844";
const taskId = "00000000-0000-7000-8000-000000001845";
const pilotId = "00000000-0000-7000-8000-000000001846";
const messageId = (sequence: number) => `00000000-0000-7000-8000-${String(18400 + sequence).padStart(12, "0")}`;

async function conversations(page: Page) {
  const sends: unknown[] = [];
  const history = Array.from({ length: 35 }, (_, index) => ({ id: messageId(index + 1), channel_id: channel, sequence: index + 1,
    parent_message_id: null, sender_id: "other", sender_display_name: "Ven", body: index === 24 ? `[[work-item-task:${taskId}]]` : index === 25 ? `[[process-task:${pilotId}]]` : `Melding ${index + 1}. ` + "Leseposisjonen skal vere her etter oppdatering. ".repeat(4),
    sent_at: "2026-10-05T00:00:00Z", edited_at: null, deleted_at: null }));
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => route.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: "actor" }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, slug: "updates", name: "Oppdatering", kind: "public", circle_id: null,
        direct_user_id: null, is_direct: false, description: "", role: "member", last_read_sequence: 35, latest_sequence: 35 }] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: channel, history }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
      case "load_recent_messages": case "load_older_messages": reply("messages_loaded", { channel_id: channel, messages: [] }); break;
      case "send_message": sends.push(command); break;
    }
  }));
  await page.route(/\/work-items\/applications(?:\?|$)/, route => route.fulfill({ json: [] }));
  await page.route(/\/channels\/[^/]+\/media(?:\?|$)/, route => route.fulfill({ json: { media: {
    id: mediaId, channel_id: channel, original_filename: "utkast.png", content_type: "image/png"
  } } }));
  return sends;
}

test("menu update uses a real new worker and code revision while preserving drafts, reading anchors and journals", async ({ page, baseURL, browserName }) => {
  test.setTimeout(60_000);
  await page.setViewportSize({ width: 390, height: 844 });
  if (browserName === "webkit") await page.addInitScript(() => Object.defineProperty(navigator, "standalone", { value: true }));
  const source = await page.request.get(`${baseURL}/?participant=update-fixture`);
  expect(source.ok()).toBe(true);
  const template = await source.text();
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url), "utf8");
  const worker = await readFile(new URL("../../assets/service-worker.js", import.meta.url), "utf8");
  const helper = await build({ stdin: { contents: 'export { createDurableOutbox } from "./src/durable-outbox";', resolveDir: fileURLToPath(new URL("..", import.meta.url)) },
    bundle: true, write: false, format: "esm", platform: "browser", target: "es2022" });
  let revision = "old", failCheck = false, navigations = 0, workerReads = 0, decided = false, pilotCompleted = false;
  const server = createServer(async (request, response) => {
    try {
      const path = new URL(request.url!, "http://fixture").pathname;
      if (path === "/seed") { response.writeHead(200, { "content-type": "text/html" }); response.end("<!doctype html><title>State fixture</title>"); }
      else if (path === `/api/v1/channels/${channel}/media` && request.method === "POST") {
        request.resume(); response.writeHead(200, { "content-type": "application/json" });
        response.end(JSON.stringify({ media: { id: mediaId, channel_id: channel, original_filename: "utkast.png", content_type: "image/png" } }));
      } else if (path === `/api/v1/work-item-tasks/${taskId}` || path === `/api/v1/work-item-tasks/${taskId}/decide`) {
        if (request.method === "POST") { request.resume(); decided = true; }
        response.writeHead(200, { "content-type": "application/json" });
        response.end(JSON.stringify({ id: taskId, message_id: messageId(25), work_item_id: mediaId, revision: decided ? 2 : 1,
          title: "Ei vurdering", description: "Ulagra val skal vernast", application_name: "Sprøyt", status: decided ? "completed" : "pending",
          process_status: "waiting", delivery_status: "ready", category: "bug", priority: "untriaged", decision_status: null,
          assignee_name: "Behandlar", can_decide: !decided, blocked: false, node_id: "review", can_request_information: true,
          information_request: null, information_response: null, supplements: [] }));
      } else if (path === `/api/v1/process-pilot/tasks/${pilotId}` || path === `/api/v1/process-pilot/tasks/${pilotId}/complete`) {
        if (request.method === "POST") { request.resume(); pilotCompleted = true; }
        response.writeHead(200, { "content-type": "application/json" });
        response.end(JSON.stringify({ id: pilotId, message_id: messageId(26), instance_id: "pilot-instance", node_id: "first",
          status: pilotCompleted ? "completed" : "pending", can_complete: !pilotCompleted, assignee_id: "actor", assignee_name: "Du",
          title: "Prosessoppgåva", delivery_status: "ready", process_status: "waiting" }));
      } else if (path.endsWith("/work-items/applications")) { response.writeHead(200, { "content-type": "application/json" }); response.end("[]"); }
      else if (path === "/fixture-state.js") { response.writeHead(200, { "content-type": "text/javascript" }); response.end(helper.outputFiles[0]!.text); }
      else if (path === "/service-worker.js") {
        workerReads++; response.writeHead(200, { "content-type": "text/javascript", "cache-control": "no-store", "service-worker-allowed": "/" });
        response.end(`const fixtureRevision = ${JSON.stringify(revision)};\n${worker}`);
      } else if (/^\/assets\/app\/[^/]+\/app.js$/.test(path)) {
        response.writeHead(200, { "content-type": "text/javascript" }); response.end(`globalThis.fixtureCodeRevision = ${JSON.stringify(path.split("/")[3])};\n${bundle}`);
      } else if (path === "/") {
        if (request.headers["sec-fetch-mode"] === "navigate") navigations++;
        if (failCheck && request.headers["sec-fetch-mode"] !== "navigate") { response.writeHead(503, { "content-type": "text/html" }); response.end("Offline"); return; }
        response.writeHead(200, { "content-type": "text/html", "cache-control": "no-store", "content-security-policy": source.headers()["content-security-policy"] });
        response.end(template.replace(/\/assets\/app\/[^/]+\/app\.js/g, `/assets/app/${revision}/app.js`));
      } else {
        const proxied = await fetch(`${baseURL}${request.url}`);
        response.writeHead(proxied.status, { "content-type": proxied.headers.get("content-type") ?? "application/octet-stream" });
        response.end(Buffer.from(await proxied.arrayBuffer()));
      }
    } catch { response.writeHead(500); response.end("Fixture error"); }
  });
  await new Promise<void>(resolve => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Missing fixture server");
  const url = `http://127.0.0.1:${address.port}`;
  try {
    const sends = await conversations(page);
    await page.goto(`${url}/seed`);
    await page.evaluate(async ({ channel, otherChannel, requestId }) => {
      localStorage.setItem(`sproyt.channel-draft.v1.${channel}`, "Utkastet skal stå etter oppdatering");
      localStorage.setItem("sproyt.active-channel.v1", channel);
      localStorage.setItem("keep-login", "keep");
      sessionStorage.setItem("sproyt-work-item-decision:actor:fixture", JSON.stringify({ id: requestId, revision: 7, payload: "original decision" }));
      const helperUrl = "/fixture-state.js";
      const module = await import(helperUrl);
      const outbox = module.createDurableOutbox(); await outbox.setUser("actor");
      await outbox.enqueue({ requestId, channelId: otherChannel, parentMessageId: null, body: "Utan kvittering", draft: "Utan kvittering", media: [] });
    }, { channel, otherChannel, requestId });
    await page.goto(`${url}/?participant=updater`);
    const preview = page.locator("#sproyt-react-preview");
    const composer = preview.getByRole("textbox", { name: "Skriv melding", exact: true });
    await expect(composer).toHaveValue("Utkastet skal stå etter oppdatering");
    await expect.poll(() => page.evaluate(() => Boolean(navigator.serviceWorker.controller))).toBe(true);
    expect(await page.evaluate(() => (globalThis as any).fixtureCodeRevision)).toBe("old");
    const timeline = preview.locator(".sp-channel-pane > .sp-timeline");
    await timeline.evaluate(element => { element.dispatchEvent(new WheelEvent("wheel", { deltaY: -600 })); element.scrollTop = 1300; element.dispatchEvent(new Event("scroll")); });
    const captureAnchor = () => timeline.evaluate(element => {
      const bounds = element.getBoundingClientRect();
      const message = [...element.querySelectorAll<HTMLElement>("[data-message-id]")].find(item => item.getBoundingClientRect().bottom > bounds.top)!;
      return { id: message.dataset.messageId!, offset: message.getBoundingClientRect().top - bounds.top };
    });
    const photo = await page.evaluate(() => {
      const canvas = document.createElement("canvas"); canvas.width = 100; canvas.height = 100;
      canvas.getContext("2d")!.fillRect(0, 0, 100, 100); return canvas.toDataURL("image/png").split(",")[1]!;
    });
    await preview.locator('input[type="file"]').first().setInputFiles({ name: "utkast.png", mimeType: "image/png", buffer: Buffer.from(photo, "base64") });
    await expect(preview.getByRole("button", { name: "Fjern utkast.png", exact: true })).toBeVisible();
    const openMenu = async () => { await preview.getByRole("button", { name: "Meny", exact: true }).click(); await preview.getByRole("button", { name: "Meny og innstillingar", exact: true }).click(); };
    await openMenu();
    const menu = preview.getByRole("dialog", { name: "Meny og innstillingar", exact: true });
    await menu.getByRole("button", { name: "Oppdater appen", exact: true }).click();
    await expect(menu).toContainText("Send eller fjern dei valde vedlegga"); expect(navigations).toBe(1);
    await menu.getByRole("button", { name: "Lukk menyen", exact: true }).click();
    await preview.getByRole("button", { name: "Meny", exact: true }).click();
    await preview.getByRole("button", { name: "Fjern utkast.png", exact: true }).click();
    const review = preview.locator(".sp-work-item-task");
    await review.locator(".sp-work-item-task-summary").click();
    await review.getByRole("combobox", { name: "Prioritet", exact: true }).selectOption("high");
    await review.locator(".sp-work-item-task-summary").click();
    await openMenu(); await menu.getByRole("button", { name: "Prøv oppdateringa igjen", exact: true }).click();
    await expect(menu).toContainText("Fullfør redigeringa først"); expect(navigations).toBe(1);
    await menu.getByRole("button", { name: "Lukk menyen", exact: true }).click();
    await preview.getByRole("button", { name: "Meny", exact: true }).click();
    await review.locator(".sp-work-item-task-summary").click();
    await expect(review.getByRole("combobox", { name: "Prioritet", exact: true })).toHaveValue("high");
    await review.getByRole("button", { name: "Lagre avgjerd", exact: true }).click();
    await expect(review.locator(".sp-work-item-task-summary")).toContainText("Fullført");
    await review.locator(".sp-work-item-task-summary").click();
    const pilot = preview.locator(".sp-process-task");
    await pilot.locator("summary").click();
    await expect(pilot.getByRole("button", { name: "Fullfør oppgåva", exact: true })).toBeVisible();
    // Exercise a previously edited disclosure retained by the update guard.
    // Today's pilot has no text fields; the marker must still clear at terminal receipt.
    await pilot.dispatchEvent("input");
    await pilot.getByRole("button", { name: "Fullfør oppgåva", exact: true }).click();
    await expect(pilot).toHaveAttribute("data-task-status", "completed");
    await pilot.locator("summary").click();
    await openMenu(); failCheck = true;
    await menu.getByRole("button", { name: "Prøv oppdateringa igjen", exact: true }).click();
    await expect(menu).toContainText("Kunne ikkje hente appversjonen"); expect(navigations).toBe(1);
    const anchor = await captureAnchor();
    failCheck = false; revision = "new";
    await menu.getByRole("button", { name: "Prøv oppdateringa igjen", exact: true }).click();
    await expect(composer).toHaveValue("Utkastet skal stå etter oppdatering");
    await expect(preview).toContainText("Appen er oppdatert");
    expect(await page.evaluate(() => (globalThis as any).fixtureCodeRevision)).toBe("new");
    expect(navigations).toBe(2); expect(workerReads).toBeGreaterThanOrEqual(2);
    await expect.poll(() => timeline.locator(`[data-message-id="${anchor.id}"]`).evaluate((element, offset) => {
      const viewport = element.closest(".sp-timeline")!; return Math.abs(element.getBoundingClientRect().top - viewport.getBoundingClientRect().top - offset);
    }, anchor.offset)).toBeLessThan(2);
    const saved = await page.evaluate(async () => {
      const helperUrl = "/fixture-state.js";
      const module = await import(helperUrl); const outbox = module.createDurableOutbox();
      return { channel: localStorage.getItem("sproyt.active-channel.v1"), login: localStorage.getItem("keep-login"),
        decision: sessionStorage.getItem("sproyt-work-item-decision:actor:fixture"), pending: await outbox.setUser("actor") };
    });
    expect(saved.channel).toBe(channel); expect(saved.login).toBe("keep"); expect(saved.decision).toContain("original decision");
    expect(saved.pending.map((entry: any) => entry.requestId)).toEqual([requestId]); expect(sends).toEqual([]);
    await openMenu(); await menu.getByRole("button", { name: "Oppdater appen", exact: true }).click();
    await expect.poll(() => navigations).toBe(3); await expect(preview).toContainText("Appen er oppdatert");
    await page.waitForTimeout(250); expect(navigations).toBe(3);
  } finally { await page.goto("about:blank"); await new Promise<void>(resolve => server.close(() => resolve())); }
});
