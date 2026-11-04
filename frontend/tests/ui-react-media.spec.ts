import { expect, test, type Page } from "@playwright/test";
import { createChannelViaUi } from "./support/channel-setup";

async function photo(page: Page, name = "landskap.png") {
  const data = await page.evaluate(() => {
    const canvas = document.createElement("canvas");
    canvas.width = 600; canvas.height = 100;
    canvas.getContext("2d")!.fillRect(0, 0, 600, 100);
    return canvas.toDataURL("image/png").split(",")[1]!;
  });
  return { name, mimeType: "image/png", buffer: Buffer.from(data, "base64") };
}

for (const ui of ["react", "legacy"] as const) {
  test(`${ui} receives native document markers as explicit downloads without preview requests`, async ({ page, context, browserName }) => {
    const participant = `document ${ui} + ${browserName} ${Date.now()}`;
    const documents = [
      { id: "00000000-0000-4000-8000-000000000071", type: "application/pdf", name: "Rå%20 [plan] & rapport.pdf" },
      { id: "00000000-0000-4000-8000-000000000072", type: "application/zip", name: "arkiv.zip" },
      { id: "00000000-0000-4000-8000-000000000073", type: "text/plain", name: "Blå%20 | [<em>notat</em>].txt" },
      { id: "00000000-0000-4000-8000-000000000074", type: "text/csv", name: "måling.csv" }
    ];
    const imageId = "00000000-0000-4000-8000-000000000075";
    const videoId = "00000000-0000-4000-8000-000000000076";
    const requests: string[] = [];
    // Context routing includes a separate native attachment navigation.
    await context.route(/\/api\/v1\/media\//, route => {
      const url = new URL(route.request().url());
      requests.push(url.pathname);
      if (url.pathname.startsWith(`/api/v1/media/${imageId}`)) return route.fulfill(url.pathname.endsWith("/status")
        ? { json: { state: "ready" } }
        : { contentType: "image/gif", body: Buffer.from("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7", "base64") });
      if (url.pathname === `/api/v1/media/${videoId}`) return route.fulfill({ contentType: "video/mp4", body: Buffer.alloc(0) });
      const document = documents.find(item => url.pathname === `/api/v1/media/${item.id}/download`);
      return route.fulfill(document ? { contentType: document.type, body: "synthetic document fixture", headers: {
        "content-disposition": "attachment; filename=\"server-normalized.pdf\"",
        "x-content-type-options": "nosniff"
      } } : { status: 415, body: "Documents do not have media previews" });
    });
    const body = [...documents.map(item => `[[media:${item.id}|${item.type}|${encodeURIComponent(item.name)}]]`),
      `[[media:${imageId}|image/gif|image.gif]]`, `[[media:${videoId}|video/mp4|video.mp4]]`].join("\n");
    // These are received messages from a native sender, not web uploads. Keep
    // the emitted wire marker fixture independent of server-owned file IDs.
    const channel = "document-download";
    await page.routeWebSocket(/\/ws(?:\?|$)/, socket => socket.onMessage(data => {
      const command = JSON.parse(String(data));
      const reply = (type: string, payload: unknown) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", request_id: command.request_id, type, payload }));
      switch (command.type) {
        case "hello": reply("hello", { participant_id: "reader" }); break;
        case "ping": reply("pong", {}); break;
        case "list_users": reply("users_listed", { users: [] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [] }); break;
        case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
        case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
        case "list_my_channels": reply("channels_listed", { channels: [{ id: channel, slug: channel, name: channel, kind: "public",
          circle_id: null, direct_user_id: null, is_direct: false, description: "", role: "member", last_read_sequence: 1, latest_sequence: 1 }] }); break;
        case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: [{ id: "native-document-message", channel_id: channel,
          parent_message_id: null, sender_id: "native-sender", sender_display_name: "Native sender", body, sequence: 1,
          sent_at: "2026-11-04T12:00:00Z", edited_at: null, deleted_at: null }] }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
        case "mark_read": reply("read_marker_updated", { membership: { channel_id: channel, user_id: "reader", role: "member",
          joined_at: "2026-11-04T12:00:00Z", last_read_sequence: 1 } }); break;
      }
    }));
    await page.goto(`/?participant=${encodeURIComponent(participant)}&ui=${ui}&channel=${channel}`);
    const surface = page.locator(ui === "react" ? "#sproyt-react-preview .sp-channel-pane" : "#messages");
    for (const document of documents) {
      const link = surface.getByRole("link", { name: `Last ned ${document.name}`, exact: true });
      await expect(link).toBeVisible();
      await expect(link).toHaveText("Last ned vedlegget");
      await expect(link).toHaveAttribute("href", `/api/v1/media/${document.id}/download?participant=${encodeURIComponent(participant)}`);
      await expect(link).toHaveAttribute("download", "");
      await expect(link).toHaveAttribute("target", "_blank");
      await expect(link).toHaveAttribute("rel", "noopener noreferrer");
      const figure = surface.locator("figure").filter({ has: page.getByRole("link", { name: `Last ned ${document.name}`, exact: true }) });
      await expect(figure.locator("figcaption")).toContainText(document.name);
      await expect(figure.locator("img,video,iframe,object,embed,em")).toHaveCount(0);
    }
    const documentRequests = () => requests.filter(path => documents.some(document => path.includes(document.id)));
    expect(documentRequests()).toEqual([]);
    const image = surface.locator(`img[src*="${imageId}/preview"]`);
    await expect(image).toBeVisible();
    const video = surface.locator(`video[src*="${videoId}"]`);
    await expect(video).toBeVisible();
    await expect(video).toHaveAttribute("controls", "");
    await expect(video).toHaveAttribute("preload", "metadata");
    const href = await surface.getByRole("link", { name: `Last ned ${documents[0]!.name}`, exact: true }).evaluate(element => (element as HTMLAnchorElement).href);
    // Chromium's download-attribute requests bypass Playwright routing. Use
    // the actual link destination in a native navigation so the synthetic
    // attachment response can prove the server's filename is authoritative.
    const downloadPage = await context.newPage();
    try {
      const downloaded = downloadPage.waitForEvent("download");
      await downloadPage.goto(href).catch((error: Error) => expect(error.message).toMatch(/ERR_ABORTED|Download is starting/));
      expect((await downloaded).suggestedFilename()).toBe("server-normalized.pdf");
    } finally { await downloadPage.close(); }
    expect(documentRequests()).toEqual([`/api/v1/media/${documents[0]!.id}/download`]);
  });
}

test("React sends real attachment-only messages and opens a full-screen zoomable original", async ({ page, context, browserName }) => {
  let sockets = 0;
  const sends: string[] = [];
  page.on("websocket", socket => {
    sockets++;
    socket.on("framesent", ({ payload }) => {
      const command = JSON.parse(String(payload));
      if (command.type === "send_message") sends.push(command.payload.body);
    });
  });
  await page.goto(`/?participant=preview-media-send-${browserName}-${Date.now()}&ui=react`);
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  await input.focus();
  await preview.getByRole("button", { name: "Skriveverktøy", exact: true }).click();
  const chooser = page.waitForEvent("filechooser");
  await preview.getByRole("button", { name: "Legg ved bilete eller video" }).click();
  await (await chooser).setFiles(await photo(page));
  const attachments = preview.getByRole("region", { name: "Valde vedlegg" });
  await expect(attachments.getByRole("button", { name: "Fjern landskap.png" })).toBeEnabled();
  await expect(attachments).toContainText("klar til å sendast");
  await input.blur();
  await expect(attachments.getByRole("button", { name: "Vis landskap.png", exact: true })).toBeVisible();
  await preview.getByRole("button", { name: "Send vedlegg", exact: true }).click();
  await expect(attachments.getByRole("button", { name: "Fjern landskap.png" })).toHaveCount(0);
  const message = preview.locator(".sp-channel-pane [data-message-id]").filter({ has: page.getByRole("img", { name: "landskap.png" }) }).last();
  await expect(message).toBeVisible();
  const image = message.getByRole("img");
  await expect(image).toHaveCSS("object-fit", "contain");
  await expect(message.getByRole("link", { name: "Last ned landskap.png" })).toHaveAttribute("download", "landskap.png");
  await expect(message.getByRole("link", { name: "Last ned landskap.png" })).toHaveAttribute("target", "_blank");
  await message.getByRole("button", { name: "Vis originalbiletet" }).click();
  const lightbox = preview.getByRole("dialog", { name: "landskap.png" });
  const original = lightbox.getByRole("img", { name: "landskap.png" });
  await expect(original).toBeVisible();
  await expect(lightbox.getByRole("link", { name: "Last ned landskap.png" })).toHaveAttribute("target", "_blank");
  const viewerDownload = page.waitForEvent("download");
  await lightbox.getByRole("link", { name: "Last ned landskap.png" }).click();
  await expect(lightbox.locator(".sp-media-download-notice")).toHaveText("Nedlasting starta");
  expect((await viewerDownload).suggestedFilename()).toBe("landskap.png");
  const viewport = page.viewportSize()!;
  const viewerBox = await lightbox.boundingBox();
  expect(viewerBox?.width).toBe(viewport.width);
  expect(viewerBox?.height).toBe(viewport.height);
  const fittedBox = await original.boundingBox();
  await lightbox.getByRole("button", { name: "Zoom inn" }).click();
  await expect(lightbox.getByRole("button", { name: "Tilbakestill biletvising" })).toHaveText("140%");
  const enlargedBox = await original.boundingBox();
  expect(enlargedBox!.width).toBeGreaterThan(fittedBox!.width);
  await lightbox.locator(".sp-image-viewer-surface").dblclick();
  await expect(lightbox.getByRole("button", { name: "Tilbakestill biletvising" })).toHaveText("100%");
  await lightbox.locator(".sp-image-viewer-surface").dblclick();
  await expect(lightbox.getByRole("button", { name: "Tilbakestill biletvising" })).toHaveText("250%");
  const reset = lightbox.getByRole("button", { name: "Tilbakestill biletvising" });
  await reset.click();
  if (browserName === "chromium") {
    const cdp = await context.newCDPSession(page);
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: 580, y: 360, id: 1 }, { x: 700, y: 360, id: 2 }] });
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: 520, y: 360, id: 1 }, { x: 760, y: 360, id: 2 }] });
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    await expect(reset).not.toHaveText("100%");
  }
  const url = await original.getAttribute("src");
  expect(url).toMatch(/^\/api\/v1\/media\/[0-9a-f-]+\?participant=/);
  await page.keyboard.press("Escape");
  await expect(lightbox).toHaveCount(0);
  const captionDownload = page.waitForEvent("download");
  await message.getByRole("link", { name: "Last ned landskap.png" }).click();
  await expect(message.locator(".sp-media-download-notice")).toHaveText("Nedlasting starta");
  expect((await captionDownload).suggestedFilename()).toBe("landskap.png");
  const response = await page.request.get(url!);
  expect(response.ok()).toBe(true);
  expect(sends).toHaveLength(1);
  expect(sends[0]).toContain("[[media:");
  expect(sockets).toBe(1);
});

test("React paste uploads files, reports recoverable errors, and removes selected attachments", async ({ page }) => {
  await page.goto("/?participant=preview-media-paste&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  await input.fill("behald teksten");
  await page.route(/\/channels\/[^/]+\/media(?:\?.*)?$/, route => route.fulfill({ status: 413, body: "Fila er for stor" }), { times: 1 });
  const file = await photo(page, "limt-inn.png");
  const paste = async () => input.evaluate((element, base64) => {
    const bytes = Uint8Array.from(atob(base64), c => c.charCodeAt(0));
    const transfer = new DataTransfer();
    transfer.items.add(new File([bytes], "limt-inn.png", { type: "image/png" }));
    element.dispatchEvent(new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: transfer }));
  }, file.buffer.toString("base64"));
  await paste();
  await expect(preview).toContainText("HTTP 413");
  await expect(input).toHaveValue("behald teksten");
  await paste();
  await expect(preview.getByRole("button", { name: "Fjern limt-inn.png" })).toBeEnabled();
  await expect(preview).not.toContainText("HTTP 413");
  await preview.getByRole("button", { name: "Fjern limt-inn.png" }).click();
  await expect(preview.getByRole("img", { name: "limt-inn.png" })).toHaveCount(0);
  await expect(input).toHaveValue("behald teksten");
});

test("an in-flight thread upload stays with its root and sends once after reopening", async ({ page }) => {
  await page.goto("/?participant=preview-media-thread&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const channel = preview.locator(".sp-channel-pane");
  const input = channel.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  const title = `media root ${Date.now()}`;
  await input.fill(title);
  await input.press("Enter");
  const root = channel.locator("[data-message-id]").filter({ hasText: title });
  await root.getByRole("button", { name: "Svar i tråd", exact: true }).click();
  const thread = preview.locator(".sp-thread-pane");
  await expect(thread.getByRole("textbox", { name: "Svar i tråden" })).toBeEnabled();
  let release!: () => void;
  const held = new Promise<void>(resolve => { release = resolve; });
  let started = false;
  await page.route(/\/channels\/[^/]+\/media(?:\?.*)?$/, async route => { started = true; await held; await route.continue(); }, { times: 1 });
  await thread.getByRole("textbox", { name: "Svar i tråden" }).focus();
  await thread.locator('input[type="file"]').setInputFiles(await photo(page, "tråd.png"));
  await expect.poll(() => started).toBe(true);
  await expect(thread).toContainText(/Behandlar fila|Gjer klar|Lastar opp/);
  await thread.getByRole("button", { name: "Lukk tråden" }).click();
  release();
  await expect(channel.getByRole("button", { name: "Fjern tråd.png" })).toHaveCount(0);
  await root.getByRole("button", { name: "Svar i tråd", exact: true }).click();
  await expect(thread.getByRole("button", { name: "Fjern tråd.png" })).toBeEnabled();
  await thread.getByRole("button", { name: "Send vedlegg", exact: true }).click();
  await expect(thread.locator("[data-message-id]").getByRole("img", { name: "tråd.png" })).toBeVisible();
  await expect(channel.locator("[data-message-id]").getByRole("img", { name: "tråd.png" })).toHaveCount(0);
});

test("image generation preserves uploaded references and does not publish", async ({ page }) => {
  let sends = 0;
  let references: string[] = [];
  await page.route(/\/api\/v1\/imagegen(?:\?.*)?$/, route => {
    if (route.request().method() === "POST") references = route.request().postDataJSON().reference_ids;
    return route.fulfill({ json: { jobs: [] } });
  });
  page.on("websocket", socket => socket.on("framesent", ({ payload }) => {
    if (JSON.parse(String(payload)).type === "send_message") sends++;
  }));
  await page.goto("/?participant=preview-media-imagegen&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  await input.focus();
  await preview.locator('input[type="file"]').setInputFiles(await photo(page, "referanse.png"));
  await expect(preview.getByRole("button", { name: "Fjern referanse.png" })).toBeEnabled();
  await input.fill('/imagegen "Teikn landskapet"');
  await input.press("Enter");
  await expect(input).toHaveValue("");
  await expect(preview.getByRole("button", { name: "Fjern referanse.png" })).toBeEnabled();
  expect(references).toHaveLength(1);
  expect(sends).toBe(0);
});

test("a multi-file upload keeps its original channel after navigation", async ({ page }) => {
  await page.goto("/?participant=preview-media-channel&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  const name = `Media-${Date.now()}`;
  await createChannelViaUi(page, name);
  await expect(preview.getByRole("heading", { name: `# ${name}`, exact: true })).toBeVisible();
  await preview.getByRole("button", { name: /# general/i }).click();
  await expect(input).toBeEnabled();
  const channels: string[] = [];
  let release!: () => void;
  const held = new Promise<void>(resolve => { release = resolve; });
  await page.route(/\/channels\/[^/]+\/media(?:\?.*)?$/, async route => {
    channels.push(new URL(route.request().url()).pathname.split("/")[4]!);
    if (channels.length === 1) await held;
    await route.continue();
  });
  await input.focus();
  await preview.locator('input[type="file"]').setInputFiles([await photo(page, "første.png"), await photo(page, "andre.png")]);
  await expect.poll(() => channels.length).toBe(1);
  await preview.getByRole("button", { name: new RegExp(`# ${name}`) }).click();
  release();
  await expect.poll(() => channels.length).toBe(2);
  expect(channels[0]).toBe(channels[1]);
  await expect(preview.getByRole("region", { name: "Valde vedlegg" })).toHaveCount(0);
  await preview.getByRole("button", { name: /# general/i }).click();
  await expect(preview.getByRole("button", { name: "Fjern første.png" })).toBeEnabled();
  await expect(preview.getByRole("button", { name: "Fjern andre.png" })).toBeEnabled();
});
