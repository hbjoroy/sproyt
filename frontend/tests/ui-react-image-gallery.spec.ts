import { expect, test, type Page } from "@playwright/test";
import { fileURLToPath } from "node:url";

type Room = "channel" | "direct" | "personal";
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const image = (n: number) => `[[media:${id(n)}|image/png|foto-${n}.png]]`;
const first = (n: number, page: Page) => page.getByRole("button", { name: `Vis foto-${n}.png i full storleik` });
const viewer = (page: Page, n: number) => page.getByRole("dialog", { name: `foto-${n}.png` });

test.beforeEach(async ({ page }) => {
  // The test server may reuse a prebuilt binary. Exercise the current frontend bundle.
  await page.route(/\/assets\/app\/[^/]+\/app\.js$/, route => route.fulfill({
    path: fileURLToPath(new URL("../dist/app.js", import.meta.url)), contentType: "text/javascript"
  }));
});

async function enter(page: Page, room: Room) {
  await page.route("**/api/v1/media/**", route => route.request().url().includes("/status")
    ? route.fulfill({ json: { state: "ready" } })
    : route.fulfill({ contentType: "image/svg+xml", body: '<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1000"/>' }));
  const message = (n: number, body: string, parent: string | null = null) => ({
    id: id(n), channel_id: room, parent_message_id: parent, sender_id: "reader", sender_display_name: "Lesar",
    body, sequence: n, sent_at: "2026-11-04T12:00:00Z", edited_at: null, deleted_at: null
  });
  const rows = Array.from({ length: 104 }, (_, i) => message(i + 1, `Melding ${i + 1}`));
  rows[0] = message(1, image(1));
  rows[101] = message(102, image(2) + ` [[media:${id(99)}|application/pdf|notat.pdf]] ` + image(3));
  rows[103] = message(104, image(4));
  const reply = message(105, image(5), id(102));
  const cursors: number[] = [];
  await page.routeWebSocket(/\/ws(?:\?|$)/, socket => socket.onMessage(data => {
    const cmd = JSON.parse(String(data));
    const emit = (type: string, payload: unknown) => socket.send(JSON.stringify({
      protocol: "sproyt.chat.v1", request_id: cmd.request_id, type, payload
    }));
    switch (cmd.type) {
      case "hello": emit("hello", { participant_id: "reader" }); break;
      case "ping": emit("pong", {}); break;
      case "list_users": emit("users_listed", { users: [] }); break;
      case "list_my_circles": emit("circles_listed", { circles: [] }); break;
      case "list_mentions": emit("mentions_listed", { mentions: [] }); break;
      case "list_tasks": emit("tasks_listed", { tasks: [] }); break;
      case "list_my_channels": emit("channels_listed", { channels: [{ id: room, name: room, slug: room, kind: "public",
        circle_id: null, direct_user_id: room === "direct" ? "other" : null, is_direct: room === "direct",
        is_personal: room === "personal", description: "", role: "member", last_read_sequence: 104, latest_sequence: 104 }] }); break;
      case "subscribe_channel": emit("subscription_started", { channel_id: room, history: rows.slice(-50) }); break;
      case "load_recent_messages": {
        const before = cmd.payload.before;
        cursors.push(before);
        emit("messages_loaded", { channel_id: room, messages: rows.filter(row => row.sequence < before).slice(-50) });
        break;
      }
      case "list_channel_reactions": emit("channel_reactions_listed", { channel_id: room, reactions: [] }); break;
      case "list_thread_summaries": emit("thread_summaries_listed", { channel_id: room, summaries: [
        { root_message_id: id(102), reply_count: 1, unread_count: 0, latest_sequence: 105 }
      ] }); break;
      case "load_thread": emit("thread_loaded", { root_message_id: id(102), messages: [rows[101], reply] }); break;
    }
  }));
  await page.goto(`/?participant=gallery-reader&channel=${room}&ui=react`);
  return cursors;
}

for (const room of ["channel", "direct", "personal"] as const) {
  test(`${room} gallery follows attachment order and pages past messages without images`, async ({ page }) => {
    const cursors = await enter(page, room);
    await first(2, page).click();
    await page.keyboard.press("ArrowRight");
    await expect(viewer(page, 3)).toBeVisible();
    await viewer(page, 3).getByRole("button", { name: "Neste bilete" }).click();
    await expect(viewer(page, 4).getByRole("link", { name: "Last ned foto-4.png" })).toHaveAttribute("href", new RegExp(id(4) + "/download"));
    await expect(viewer(page, 4).getByRole("button", { name: "Neste bilete" })).toBeDisabled();
    await page.keyboard.press("ArrowLeft");
    await page.keyboard.press("ArrowLeft");
    await expect(viewer(page, 2)).toBeVisible();
    await page.keyboard.press("ArrowLeft");
    await expect(viewer(page, 1)).toBeVisible();
    expect(cursors).toContain(55);
    expect(cursors).toContain(5);
    await expect(viewer(page, 1).getByRole("button", { name: "Førre bilete" })).toBeDisabled();
    await page.keyboard.press("ArrowRight");
    await expect(viewer(page, 2)).toBeVisible();
  });
}

test("the same root opens a separate gallery in its thread", async ({ page }) => {
  await enter(page, "channel");
  const channel = page.locator(".sp-channel-pane");
  await channel.locator(`[data-thread-trigger="${id(102)}"]`).first().click();
  const thread = page.locator(".sp-thread-pane");
  await thread.getByRole("button", { name: "Vis foto-3.png i full storleik" }).click();
  await page.keyboard.press("ArrowRight");
  await expect(viewer(page, 5)).toBeVisible();
  await expect(viewer(page, 5).getByRole("button", { name: "Neste bilete" })).toBeDisabled();
  await page.keyboard.press("ArrowLeft");
  await expect(viewer(page, 3)).toBeVisible();
  await page.keyboard.press("Escape");
  await thread.getByRole("button", { name: "Lukk tråden" }).click();
  await first(3, page).click();
  await page.keyboard.press("ArrowRight");
  await expect(viewer(page, 4)).toBeVisible();
});

test("a horizontal touch changes images in both directions and a zoomed drag pans", async ({ page, context, browserName }) => {
  test.skip(browserName !== "chromium", "Touch injection uses CDP");
  await enter(page, "channel");
  await first(2, page).click();
  const cdp = await context.newCDPSession(page);
  async function swipe(from: number, to: number) {
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: from, y: 270 }] });
    await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: to, y: 273 }] });
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  }
  await expect(viewer(page, 2).getByRole("button", { name: "Zoom inn" })).toBeEnabled();
  await swipe(320, 100);
  await expect(viewer(page, 3).getByRole("img")).toBeVisible();
  await swipe(100, 320);
  await expect(viewer(page, 2).getByRole("img")).toBeVisible();
  await viewer(page, 2).getByRole("button", { name: "Zoom inn" }).click();
  await swipe(320, 100);
  await expect(viewer(page, 2)).toBeVisible();
  await expect.poll(() => viewer(page, 2).getByRole("img").evaluate(img => new DOMMatrix(getComputedStyle(img).transform).m41)).toBeLessThan(0);
});

test("uploaded originals open and download in their sent order", async ({ page }, { project }) => {
  const participant = `gallery-upload-${project.name}`;
  await page.goto(`/?participant=${participant}&ui=react`);
  const pane = page.locator("#sproyt-react-preview .sp-channel-pane");
  const input = pane.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(input).toBeEnabled({ timeout: 15000 });
  const names = [`${participant}-raud.png`, `${participant}-blå.png`];
  const pngs = await page.evaluate(() => ["red", "blue"].map(color => {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 4;
    const ctx = canvas.getContext("2d")!;
    ctx.fillStyle = color;
    ctx.fillRect(0, 0, 4, 4);
    return canvas.toDataURL("image/png").split(",")[1]!;
  }));
  await input.focus();
  await pane.getByRole("button", { name: "Skriveverktøy", exact: true }).click();
  const chooser = page.waitForEvent("filechooser");
  await pane.getByRole("button", { name: "Legg ved bilete eller video" }).click();
  await (await chooser).setFiles(names.map((name, i) => ({ name, mimeType: "image/png", buffer: Buffer.from(pngs[i]!, "base64") })));
  await expect(pane.getByRole("button", { name: `Fjern ${names[1]}` })).toBeEnabled();
  await input.blur();
  await pane.getByRole("button", { name: "Send vedlegg", exact: true }).click();
  await pane.getByRole("button", { name: `Vis ${names[0]} i full storleik` }).click();
  await page.keyboard.press("ArrowRight");
  const dialog = page.getByRole("dialog", { name: names[1] });
  await expect(dialog.getByRole("img")).toBeVisible();
  const url = await dialog.getByRole("link", { name: `Last ned ${names[1]}` }).getAttribute("href");
  const response = await page.request.get(url!);
  expect(response.ok()).toBe(true);
  expect((await response.body()).toString("base64")).toBe(pngs[1]);
});
