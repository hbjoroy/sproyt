import { expect, test, type Page } from "@playwright/test";
import { createServer, type ServerResponse } from "node:http";
import { readFile } from "node:fs/promises";
import { build } from "esbuild";
import { fileURLToPath } from "node:url";

test.use({ serviceWorkers: "allow" });
const channel = "00000000-0000-7000-8000-000000001861";
const observer = "00000000-0000-7000-8000-000000001862";
const media = "00000000-0000-7000-8000-000000001863";

test("worker never acknowledges failed file storage and rejects unsupported share payloads", async ({ page }) => {
  const worker = (await readFile(new URL("../../assets/share-inbox.js", import.meta.url), "utf8")) + "\n" + (await readFile(new URL("../../assets/service-worker.js", import.meta.url), "utf8"));
  const helper = await readFile(new URL("../../assets/share-inbox.js", import.meta.url), "utf8");
  const server = createServer((request, response) => {
    const path = new URL(request.url!, "http://fixture").pathname;
    if (path === "/service-worker.js" || path === "/helper.js") { response.writeHead(200, { "content-type": "text/javascript", "service-worker-allowed": "/" }); response.end(path === "/helper.js" ? helper : worker); }
    else if (path === "/auth/share-identity") { response.writeHead(200, { "content-type": "application/json" }); response.end('{"user_id":"actor"}'); }
    else { response.writeHead(200, { "content-type": "text/html; charset=utf-8" }); response.end('<!doctype html><title>Share storage</title><script src="/helper.js"></script>'); }
  });
  await new Promise<void>(resolve => server.listen(0, "127.0.0.1", resolve));
  const address = server.address(); if (!address || typeof address === "string") throw new Error("No fixture");
  try {
    await page.goto(`http://127.0.0.1:${address.port}/`);
    await page.evaluate(async () => { await navigator.serviceWorker.register("/service-worker.js"); await navigator.serviceWorker.ready; });
    await expect.poll(() => page.evaluate(() => Boolean(navigator.serviceWorker.controller))).toBe(true);
    const result = await page.evaluate(async () => {
      const form = new FormData(); form.set("text", "Private share"); form.set("image", new File(["original image bytes"], "original.png", { type: "image/png" }));
      const response = await fetch("/share-target", { method: "POST", body: form });
      const items = await globalThis.SproytShareInbox.list("actor");
      return { status: response.status, body: await response.text(), items: await Promise.all(items.map(async item => ({ text: item.text, filename: item.file?.name, content: await item.file?.text() }))) };
    });
    if (result.status === 503) { expect(result.body).toContain("Delinga er ikkje teken imot"); expect(result.items).toEqual([]); }
    else { expect(result.status).toBe(200); expect(result.items).toEqual([{ text: "Private share", filename: "original.png", content: "original image bytes" }]); }
    const rejected = await page.evaluate(async () => {
      const form = new FormData(); form.set("image", new File(["unsupported"], "document.txt", { type: "text/plain" }));
      const response = await fetch("/share-target", { method: "POST", body: form }); return { status: response.status, text: await response.text() };
    });
    expect(rejected.status).toBe(503); expect(rejected.text).toContain("støtta bilete");
  } finally { await page.goto("about:blank"); await new Promise<void>(resolve => server.close(() => resolve())); }
});

async function conversations(page: Page, actor: () => string, sends: any[], acknowledgements: Array<() => void>) {
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => route.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => route.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: actor() }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [channel, observer].map((id, index) => ({ id, slug: index ? "observer" : "share", name: index ? "Berre lesing" : "Deling", kind: "public", circle_id: null,
        direct_user_id: null, is_direct: false, description: "", role: index ? "observer" : "member", last_read_sequence: 0, latest_sequence: 0 })) }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: command.payload.channel_id, history: [] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
      case "load_recent_messages": case "load_older_messages": reply("messages_loaded", { channel_id: channel, messages: [] }); break;
      case "send_message":
        sends.push(command);
        acknowledgements.push(() => reply("message_accepted", { message: {
          id: media, channel_id: channel, parent_message_id: null, sequence: 1, sender_id: actor(), sender_display_name: "Du",
          body: command.payload.body, sent_at: "2026-10-05T00:00:00Z", edited_at: null, deleted_at: null
        } }));
        break;
    }
  }));
}

test("real share worker persists multipart through login, preserves drafts, isolates accounts and fences logout races", async ({ page, context, baseURL, browserName }) => {
  test.skip(browserName === "webkit", "Receiving share-target MVP is for installed Chrome/Android; WebKit does not register PWA share targets.");
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 390, height: 844 });
  const source = await page.request.get(`${baseURL}/?participant=share-fixture`);
  expect(source.ok()).toBe(true);
  const template = await source.text();
  const bundle = await readFile(new URL("../dist/app.js", import.meta.url), "utf8");
  const worker = (await readFile(new URL("../../assets/share-inbox.js", import.meta.url), "utf8")) + "\n" + (await readFile(new URL("../../assets/service-worker.js", import.meta.url), "utf8"));
  const helper = await build({ stdin: { contents: 'export { shareInbox } from "./src/share-target"; export { createDurableOutbox } from "./src/durable-outbox";', resolveDir: fileURLToPath(new URL("..", import.meta.url)) },
    bundle: true, write: false, format: "esm", platform: "browser", target: "es2022" });
  let actor: string | null = "actor", identityFailure = false, holdIdentity = false, uploadFailure = false;
  const held: ServerResponse[] = [], uploads: Buffer[] = [], sends: any[] = [], acknowledgements: Array<() => void> = [];
  const server = createServer(async (request, response) => {
    try {
      const path = new URL(request.url!, "http://fixture").pathname;
      if (path === "/auth/share-identity") {
        if (holdIdentity) { held.push(response); return; }
        response.writeHead(identityFailure ? 503 : actor ? 200 : 401, { "content-type": "application/json", "cache-control": "no-store" }); response.end(JSON.stringify({ user_id: actor }));
      } else if (path === "/auth/login") { response.writeHead(200, { "content-type": "text/html; charset=utf-8" }); response.end('<title>Login</title><a href="/fixture-login">Logg inn i Sprøyt</a>'); }
      else if (path === "/fixture-login") { actor = "actor"; response.writeHead(303, { location: "/" }); response.end(); }
      else if (path === "/auth/logout") { actor = null; response.writeHead(303, { location: "/auth/login" }); response.end(); }
      else if (path === `/api/v1/channels/${channel}/media`) {
        const parts: Buffer[] = []; for await (const part of request) parts.push(Buffer.from(part)); uploads.push(Buffer.concat(parts));
        response.writeHead(uploadFailure ? 403 : 200, { "content-type": "application/json" }); response.end(JSON.stringify({ media: { id: media, channel_id: channel, original_filename: "shared.png", content_type: "image/png" } }));
      } else if (path === "/fixture-state.js") { response.writeHead(200, { "content-type": "text/javascript" }); response.end(helper.outputFiles[0]!.text); }
      else if (path === "/service-worker.js") { response.writeHead(200, { "content-type": "text/javascript", "cache-control": "no-store", "service-worker-allowed": "/" }); response.end(worker); }
      else if (/^\/assets\/app\/[^/]+\/app.js$/.test(path)) { response.writeHead(200, { "content-type": "text/javascript" }); response.end(bundle); }
      else if (path === "/" || path === "/share-target") {
        if (!actor) { response.writeHead(303, { location: "/auth/login" }); response.end(); }
        else { response.writeHead(200, { "content-type": "text/html", "content-security-policy": source.headers()["content-security-policy"] }); response.end(template); }
      } else {
        const upstream = await fetch(`${baseURL}${request.url}`);
        response.writeHead(upstream.status, { "content-type": upstream.headers.get("content-type") ?? "application/octet-stream" }); response.end(Buffer.from(await upstream.arrayBuffer()));
      }
    } catch { response.writeHead(500); response.end("Fixture failed"); }
  });
  await new Promise<void>(resolve => server.listen(0, "127.0.0.1", resolve));
  const address = server.address(); if (!address || typeof address === "string") throw new Error("No fixture server");
  const url = `http://127.0.0.1:${address.port}`;
  const helperUrl = "/fixture-state.js";
  try {
    await conversations(page, () => actor ?? "anonymous", sends, acknowledgements);
    await page.goto(url);
    await page.evaluate(channel => { localStorage.setItem(`sproyt.channel-draft.v1.${channel}`, "Mitt vanlege utkast"); localStorage.setItem("sproyt.active-channel.v1", channel); }, channel);
    await page.reload();
    await expect.poll(() => page.evaluate(() => Boolean(navigator.serviceWorker.controller))).toBe(true);
    const post = async (navigation = false, from = page) => from.evaluate(async navigation => {
      const canvas = document.createElement("canvas"); canvas.width = 2; canvas.height = 2;
      const blob = await new Promise<Blob>(resolve => canvas.toBlob(value => resolve(value!), "image/png"));
      const file = new File([blob], "shared.png", { type: "image/png" });
      if (navigation) {
        const form = document.createElement("form"); form.action = "/share-target"; form.method = "POST"; form.enctype = "multipart/form-data";
        for (const [name, value] of [["text", "Delt tekst"], ["url", "https://example.test/shared"]]) { const input = document.createElement("input"); input.name = name!; input.value = value!; form.append(input); }
        const image = document.createElement("input"); image.type = "file"; image.name = "image"; const transfer = new DataTransfer(); transfer.items.add(file); image.files = transfer.files; form.append(image); document.body.append(form); form.requestSubmit(); return 0;
      }
      const form = new FormData(); form.set("text", "Delt tekst"); form.set("url", "https://example.test/shared"); form.set("image", file);
      return (await fetch("/share-target", { method: "POST", body: form })).status;
    }, navigation);
    actor = null; await post(true);
    await expect(page.getByRole("link", { name: "Logg inn i Sprøyt" })).toBeVisible();
    await page.getByRole("link", { name: "Logg inn i Sprøyt" }).click();
    await page.getByRole("button", { name: /Motteke deling/ }).click();
    const dialog = page.getByRole("dialog", { name: "Motteke deling", exact: true });
    await expect(dialog).not.toContainText("Delt tekst");
    await dialog.getByRole("button", { name: "Bruk delinga med denne kontoen" }).click();
    await expect(dialog.getByRole("textbox", { name: "Delt tekst eller lenkje" })).toHaveValue("Delt tekst\n\nhttps://example.test/shared");
    expect(sends).toHaveLength(0); expect(uploads).toHaveLength(0);
    expect(await dialog.getByRole("combobox").locator("option").allTextContents()).not.toContain("Felles · Berre lesing");
    await dialog.getByRole("combobox").selectOption(channel);
    uploadFailure = true; await dialog.getByRole("button", { name: "Send delinga", exact: true }).click();
    await expect(dialog).toContainText("HTTP 403"); expect(sends).toHaveLength(0);
    uploadFailure = false;
    await dialog.getByRole("button", { name: "Lukk delinga", exact: true }).click(); await page.reload();
    await page.getByRole("button", { name: /Motteke deling/ }).click(); await expect(dialog.getByRole("combobox")).toHaveValue(channel);
    await dialog.getByRole("button", { name: "Send delinga", exact: true }).click();
    await expect.poll(() => sends.length).toBeGreaterThan(0);
    const original = sends[0]; expect(original.payload.body).toContain(`[[media:${media}|image/png|shared.png]]`);
    expect(uploads[0]!.includes(Buffer.from('filename="shared.png"'))).toBe(true);
    await page.reload(); await expect.poll(() => sends.length).toBeGreaterThan(1);
    expect(new Set(sends.map(command => command.request_id)).size).toBe(1);
    expect(new Set(sends.map(command => command.payload.body)).size).toBe(1);
    const composer = page.locator("#sproyt-react-preview").getByRole("textbox", { name: "Skriv melding", exact: true });
    await expect(composer).toHaveValue("Mitt vanlege utkast");
    actor = "other"; await page.reload(); await expect(page.getByRole("button", { name: /Motteke deling/ })).toHaveCount(0);
    actor = "actor"; await page.reload();
    // Two tabs receiving the same OS payload resolve to one pending receipt.
    const second = await context.newPage(); await conversations(second, () => actor ?? "anonymous", sends, acknowledgements); await second.goto(url);
    await Promise.all([post(), post(false, second)]);
    const count = await page.evaluate(async helperUrl => { const { shareInbox } = await import(helperUrl); return (await shareInbox.list("actor")).filter((item: any) => !item.done).length; }, helperUrl);
    expect(count).toBe(1);
    await post();
    expect(await page.evaluate(async helperUrl => { const { shareInbox } = await import(helperUrl); return (await shareInbox.list("actor")).filter((item: any) => !item.done).length; }, helperUrl)).toBe(count);
    await page.getByRole("button", { name: /Motteke deling/ }).click();
    await second.getByRole("button", { name: /Motteke deling/ }).click();
    const uploadsBeforeRetry = uploads.length;
    await Promise.all([page, second].map(tab => tab.getByRole("button", { name: "Prøv den opphavlege sendinga igjen", exact: true }).click()));
    await expect.poll(() => sends.length).toBeGreaterThan(2);
    expect(new Set(sends.map(command => command.request_id)).size).toBe(1);
    expect(uploads.length).toBe(uploadsBeforeRetry);
    acknowledgements.slice(-2).forEach(acknowledge => acknowledge());
    await expect(dialog).toContainText("Delinga er send.");
    await dialog.getByRole("button", { name: "Lukk delinga", exact: true }).click();
    await expect(composer).toHaveValue("Mitt vanlege utkast");
    await expect.poll(() => page.evaluate(async helperUrl => {
      const { createDurableOutbox } = await import(helperUrl); const outbox = createDurableOutbox(); return (await outbox.setUser("actor")).length;
    }, helperUrl)).toBe(0);
    identityFailure = true; expect(await post()).toBe(503); identityFailure = false;
    holdIdentity = true;
    await page.evaluate(() => { const form = new FormData(); form.set("text", "Before logout"); (globalThis as any).pendingCapture = fetch("/share-target", { method: "POST", body: form }).then(response => response.status); });
    await expect.poll(() => held.length).toBe(1);
    await second.evaluate(async helperUrl => { const { shareInbox } = await import(helperUrl); await shareInbox.logout(); }, helperUrl);
    holdIdentity = false; held.splice(0).forEach(response => { response.writeHead(200, { "content-type": "application/json" }); response.end('{"user_id":"actor"}'); });
    expect(await page.evaluate(() => (globalThis as any).pendingCapture)).toBe(503);
    expect(await second.evaluate(async helperUrl => { const { shareInbox } = await import(helperUrl); return await shareInbox.list("actor"); }, helperUrl)).toEqual([]);
    await second.close();
  } finally { held.forEach(response => response.end()); await page.goto("about:blank"); await new Promise<void>(resolve => server.close(() => resolve())); }
});
