import { expect, test, type Page } from "@playwright/test";

async function sendMessages(page: Page, count: number, prefix: string) {
  const input = page.locator("#body");
  await expect(input).toBeEnabled({ timeout: 15_000 });
  for (let index = 0; index < count; index++) {
    await input.fill(`${prefix} ${String(index).padStart(2, "0")} ${"innhald ".repeat(12)}`);
    await input.press("Enter");
    await expect(input).toHaveValue("");
  }
}

async function visibleAnchor(page: Page, viewportSelector: string) {
  return page.locator(viewportSelector).evaluate((viewport: HTMLElement) => {
    const top = viewport.getBoundingClientRect().top;
    const message = [...viewport.querySelectorAll<HTMLElement>("[data-message-id]")]
      .find(candidate => candidate.getBoundingClientRect().bottom > top + 1);
    if (!message?.dataset.messageId) throw new Error("timeline has no visible message anchor");
    return { id: message.dataset.messageId, offset: message.getBoundingClientRect().top - top };
  });
}

test("React timeline loads older history at the top and keeps the visible message anchored", async ({ browser }) => {
  test.setTimeout(120_000);
  const sender = await browser.newPage();
  await sender.goto("/?participant=playwright-scroll-seed&ui=legacy", { waitUntil: "domcontentloaded" });
  await sendMessages(sender, 58, `historikk-${Date.now()}`);
  await sender.close();

  const page = await browser.newPage({ viewport: { width: 1200, height: 650 } });
  let olderRequests = 0;
  let readSequence = 0;
  page.on("websocket", socket => {
    socket.on("framereceived", ({ payload }) => {
      const event = JSON.parse(String(payload));
      if (event.type === "read_marker_updated") readSequence = Math.max(readSequence, event.payload.membership.last_read_sequence);
    });
    socket.on("framesent", ({ payload }) => {
    const command = JSON.parse(String(payload));
    if (command.type === "load_recent_messages" && command.payload.before) olderRequests++;
    });
  });
  await page.goto("/?participant=playwright-scroll-reader&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const timeline = preview.locator(".sp-channel-pane > .sp-timeline");
  // A new reader starts at first unread. Explicitly read to the end, then open
  // a new session so this contract exercises manual paging from the latest page.
  await expect(preview.getByRole("button", { name: "Gå til siste", exact: true }).filter({ visible: true })).toBeVisible();
  await expect.poll(() => timeline.locator("[data-message-id]").count()).toBeGreaterThan(0);
  const latestSequence = await timeline.locator("[data-message-sequence]").evaluateAll(messages =>
    Math.max(...messages.map(message => Number((message as HTMLElement).dataset.messageSequence))));
  await preview.getByRole("button", { name: "Gå til siste", exact: true }).filter({ visible: true }).click();
  await expect.poll(() => readSequence).toBeGreaterThanOrEqual(latestSequence);
  await page.reload({ waitUntil: "domcontentloaded" });
  olderRequests = 0;
  await expect.poll(() => timeline.locator("[data-message-id]").count(), { timeout: 15_000 }).toBeGreaterThan(0);
  await expect.poll(() => timeline.evaluate(element => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeLessThan(2);
  const initialCount = await timeline.locator("[data-message-id]").count();
  await expect(timeline.getByRole("button", { name: "Last eldre meldingar" })).toBeVisible();

  const anchor = await timeline.evaluate((element: HTMLElement) => {
    element.scrollTop = 0;
    const top = element.getBoundingClientRect().top;
    const message = [...element.querySelectorAll<HTMLElement>("[data-message-id]")]
      .find(candidate => candidate.getBoundingClientRect().bottom > top + 1)!;
    const anchor = { id: message.dataset.messageId!, offset: message.getBoundingClientRect().top - top };
    element.dispatchEvent(new Event("scroll"));
    return anchor;
  });
  await expect.poll(() => olderRequests).toBeGreaterThan(0);
  // The test database is shared by the named-suite run, so earlier tests may
  // already have added enough messages to fetch several older pages while the
  // viewport remains at the top. Verify that content was prepended and the
  // reading anchor held instead of assuming one request or a fixed total.
  await expect.poll(() => timeline.locator("[data-message-id]").count()).toBeGreaterThan(initialCount);
  await expect.poll(async () => {
    const restored = await timeline.locator(`[data-message-id="${anchor.id}"]`).evaluate((message: HTMLElement) =>
      message.getBoundingClientRect().top - message.closest<HTMLElement>(".sp-timeline")!.getBoundingClientRect().top);
    return Math.abs(restored - anchor.offset);
  }).toBeLessThan(3);
  await page.close();
});

test("React thread preserves reading position through resize and reveals an own reply", async ({ page }) => {
  test.setTimeout(90_000);
  let sockets = 0;
  let readCommands = 0;
  page.on("websocket", socket => {
    sockets++;
    socket.on("framesent", ({ payload }) => {
      const command = JSON.parse(String(payload));
      if (command.type === "mark_thread_read") readCommands++;
    });
  });
  await page.setViewportSize({ width: 1400, height: 650 });
  await page.goto("/?participant=playwright-thread-scroll&ui=react", { waitUntil: "domcontentloaded" });
  const preview = page.locator("#sproyt-react-preview");
  const composer = preview.locator(".sp-channel-pane").getByRole("textbox", { name: "Skriv melding" });
  await expect(composer).toBeEnabled({ timeout: 15_000 });
  const rootText = `scrolltråd ${Date.now()}`;
  await composer.fill(rootText);
  await composer.press("Enter");
  const root = preview.locator(".sp-channel-pane [data-message-id]").filter({ hasText: rootText });
  await root.getByRole("button", { name: "Svar i tråd" }).click();
  const thread = preview.locator(".sp-thread-pane");
  const replies = thread.locator(".sp-thread-replies");
  const reply = thread.getByRole("textbox", { name: "Svar i tråden" });
  await expect(reply).toBeFocused();
  for (let index = 0; index < 14; index++) {
    await reply.fill(`trådhistorikk ${index} ${"langt svar ".repeat(8)}`);
    await reply.press("Enter");
    await expect(reply).toHaveValue("");
  }
  await expect(replies.locator("[data-message-id]")).toHaveCount(15);
  await replies.evaluate((element: HTMLElement) => {
    element.scrollTop = Math.floor(element.scrollHeight / 3);
    element.dispatchEvent(new Event("scroll"));
  });
  const anchor = await visibleAnchor(page, "#sproyt-react-preview .sp-thread-replies");
  await replies.locator("[data-message-id]").first().evaluate((element: HTMLElement) => {
    element.style.paddingBottom = "260px";
  });
  await expect.poll(async () => {
    const restored = await replies.locator(`[data-message-id="${anchor.id}"]`).evaluate((message: HTMLElement) =>
      message.getBoundingClientRect().top - message.closest<HTMLElement>(".sp-thread-replies")!.getBoundingClientRect().top);
    return Math.abs(restored - anchor.offset);
  }).toBeLessThan(3);

  const ownReply = `eige svar skal visast ${Date.now()}`;
  await reply.fill(ownReply);
  await reply.press("Enter");
  const ownMessage = replies.locator("[data-message-id]").filter({ hasText: ownReply });
  await expect(ownMessage).toBeVisible();
  await expect.poll(async () => {
    const viewportBox = await replies.boundingBox();
    const messageBox = await ownMessage.boundingBox();
    if (!viewportBox || !messageBox) return Number.POSITIVE_INFINITY;
    return Math.max(0, messageBox.y + messageBox.height - (viewportBox.y + viewportBox.height));
  }).toBeLessThanOrEqual(1);
  await expect(reply).toBeFocused();
  expect(readCommands).toBeGreaterThan(0);
  expect(sockets).toBe(1);
});
