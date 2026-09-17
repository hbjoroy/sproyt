import { expect, test, type Page } from "@playwright/test";

async function photo(page: Page, name = "landskap.png") {
  const data = await page.evaluate(() => {
    const canvas = document.createElement("canvas");
    canvas.width = 600; canvas.height = 100;
    canvas.getContext("2d")!.fillRect(0, 0, 600, 100);
    return canvas.toDataURL("image/png").split(",")[1]!;
  });
  return { name, mimeType: "image/png", buffer: Buffer.from(data, "base64") };
}

test("React sends real attachment-only messages and exposes an uncropped preview and original", async ({ page }) => {
  let sockets = 0;
  const sends: string[] = [];
  page.on("websocket", socket => {
    sockets++;
    socket.on("framesent", ({ payload }) => {
      const command = JSON.parse(String(payload));
      if (command.type === "send_message") sends.push(command.payload.body);
    });
  });
  await page.goto("/?participant=preview-media-send&ui=react");
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
  const message = preview.locator(".sp-channel-pane [data-message-id]").filter({ has: page.getByRole("img", { name: "landskap.png" }) });
  await expect(message).toBeVisible();
  const image = message.getByRole("img");
  await expect(image).toHaveCSS("object-fit", "contain");
  const url = await message.getByRole("link", { name: "Vis i full storleik" }).getAttribute("href");
  expect(url).toMatch(/^\/api\/v1\/media\/[0-9a-f-]+\?participant=/);
  await image.click();
  const lightbox = preview.getByRole("dialog", { name: "landskap.png" });
  await expect(lightbox.getByRole("img", { name: "landskap.png" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(lightbox).toHaveCount(0);
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
  await page.addInitScript(() => {
    const NativeWebSocket = window.WebSocket;
    window.WebSocket = class extends NativeWebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols);
        (window as unknown as { mediaTestSocket: WebSocket }).mediaTestSocket = this;
      }
    };
  });
  await page.goto("/?participant=preview-media-channel&ui=react");
  const preview = page.locator("#sproyt-react-preview");
  const input = preview.getByRole("textbox", { name: "Skriv melding" });
  await expect(input).toBeEnabled({ timeout: 15000 });
  const name = `Media-${Date.now()}`;
  await page.evaluate(channelName => {
    (window as unknown as { mediaTestSocket: WebSocket }).mediaTestSocket.send(JSON.stringify({
      protocol: "sproyt.chat.v1", type: "create_channel", request_id: crypto.randomUUID(),
      payload: { slug: channelName.toLowerCase(), name: channelName, kind: "public" }
    }));
  }, name);
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
