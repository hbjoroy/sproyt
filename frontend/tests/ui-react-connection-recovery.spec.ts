import { expect, test, type Route } from "@playwright/test";

const channelId = "00000000-0000-7000-8000-000000009201";

type ProbeHarness = { attempts: number; commands: string[]; openProbe(): void; failProbe(): void };

test("a failed WebSocket candidate preserves healthy SSE inbox cache and its pending read", async ({ page }) => {
  await page.clock.install();
  await page.addInitScript(({ channelId }) => {
    const sockets: FixtureSocket[] = [];
    const commands: string[] = [];
    class FixtureSocket extends EventTarget {
      static CONNECTING = 0; static OPEN = 1; static CLOSING = 2; static CLOSED = 3;
      readyState = 0;
      constructor() {
        super(); sockets.push(this);
        // Two failed initial WebSockets select the real SSE adapter. The next
        // WebSocket stays a controllable candidate beside that healthy adapter.
        if (sockets.length <= 2) queueMicrotask(() => this.close());
      }
      send(data: string) { commands.push((JSON.parse(data) as { type: string }).type); }
      close(code = 1006, reason = "fixture probe failure") {
        if (this.readyState === 3) return;
        this.readyState = 3; this.dispatchEvent(new CloseEvent("close", { code, reason }));
      }
      open() { this.readyState = 1; this.dispatchEvent(new Event("open")); }
    }
    class FixtureEventSource extends EventTarget {
      private timer: number;
      constructor(address: string) {
        super();
        this.timer = window.setInterval(() => this.dispatchEvent(new Event("heartbeat")), 1000);
        queueMicrotask(() => {
          this.dispatchEvent(new Event("open"));
          const url = new URL(address);
          if (url.searchParams.get("channel_id") === channelId) this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify({
            protocol: "sproyt.chat.v1", type: "subscription_started", request_id: url.searchParams.get("request_id"),
            payload: { channel_id: channelId, history: [] }
          }) }));
        });
      }
      close() { window.clearInterval(this.timer); }
    }
    Object.defineProperty(window, "WebSocket", { value: FixtureSocket });
    Object.defineProperty(window, "EventSource", { value: FixtureEventSource });
    Object.defineProperty(window, "sseProbeFixture", { value: {
      get attempts() { return sockets.length; }, commands,
      openProbe() { sockets.at(-1)!.open(); }, failProbe() { sockets.at(-1)!.close(); }
    } });
  }, { channelId });
  const mentions = [1, 2].map(sequence => ({ read: false, channel_name: "Prat", message: {
    id: `sse-mention-${sequence}`, channel_id: channelId, parent_message_id: null, sender_id: "peer", sender_display_name: "Ada",
    body: `Omtale ${sequence}`, sequence, sent_at: "2026-01-01T12:00:00Z", edited_at: null, deleted_at: null
  } }));
  let holdMentions = false;
  let heldRead: { route: Route; requestId: string } | undefined;
  let mentionReads = 0;
  const reply = (route: Route, type: string, request_id: string, payload: unknown = {}) => route.fulfill({ json: { protocol: "sproyt.chat.v1", type, request_id, payload } });
  await page.route("**/auth/session", route => route.fulfill({ json: { refresh_after_seconds: 3600 } }));
  await page.route(/\/api\/v1\/me\/favorite-circles(?:\?.*)?$/, route => route.fulfill({ json: [] }));
  await page.route("**/api/v1/commands?*", async route => {
    const command = route.request().postDataJSON() as { type: string; request_id: string; payload?: { channel_id?: string } };
    switch (command.type) {
      case "hello": await reply(route, "hello", command.request_id, { participant_id: "sse-inbox-reader" }); break;
      case "ping": await reply(route, "pong", command.request_id); break;
      case "list_users": await reply(route, "users_listed", command.request_id, { users: [] }); break;
      case "list_my_circles": await reply(route, "circles_listed", command.request_id, { circles: [] }); break;
      case "list_my_channels": await reply(route, "channels_listed", command.request_id, { channels: [{ id: channelId, slug: "prat", name: "Prat", circle_id: null,
        kind: "public", direct_user_id: null, is_direct: false, description: "", role: "member", last_read_sequence: 2, latest_sequence: 2 }] }); break;
      case "list_mentions":
        mentionReads++;
        if (holdMentions) heldRead = { route, requestId: command.request_id };
        else await reply(route, "mentions_listed", command.request_id, { mentions });
        break;
      case "list_tasks": await reply(route, "tasks_listed", command.request_id, { tasks: [] }); break;
      case "list_thread_summaries": await reply(route, "thread_summaries_listed", command.request_id, { channel_id: channelId, summaries: [] }); break;
      case "list_channel_reactions": await reply(route, "channel_reactions_listed", command.request_id, { channel_id: channelId, reactions: [] }); break;
      default: throw new Error(`Unexpected SSE fixture command ${command.type}`);
    }
  });
  await page.goto(`/?participant=sse-inbox&ui=react&channel=${channelId}`);
  // Advance only the controller's two known reconnect backoffs; no real sleeps.
  await expect.poll(async () => { await page.clock.runFor(1000); return page.evaluate(() => (window as unknown as { sseProbeFixture: ProbeHarness }).sseProbeFixture.attempts); }).toBe(2);
  const app = page.locator("#sproyt-react-preview");
  const inbox = app.getByRole("button", { name: "Innboks, 2 uleste omtalar", exact: true });
  await expect(inbox.locator(".sp-badge")).toHaveText("2");
  await expect(app.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  holdMentions = true;
  await inbox.click();
  const region = page.locator(".sp-main [data-workspace-page]");
  await expect.poll(() => mentionReads).toBe(2);
  await expect(region.getByText("Lastar omtalar.", { exact: true })).toBeVisible();
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => page.evaluate(() => (window as unknown as { sseProbeFixture: ProbeHarness }).sseProbeFixture.attempts)).toBe(3);
  await expect(inbox.locator(".sp-badge")).toHaveText("2");
  await page.evaluate(() => (window as unknown as { sseProbeFixture: ProbeHarness }).sseProbeFixture.openProbe());
  const candidateCommands = await page.evaluate(() => (window as unknown as { sseProbeFixture: ProbeHarness }).sseProbeFixture.commands);
  expect(candidateCommands).toContain("hello");
  expect(candidateCommands).not.toContain("list_mentions");
  expect(candidateCommands).not.toContain("list_tasks");
  await page.evaluate(() => (window as unknown as { sseProbeFixture: ProbeHarness }).sseProbeFixture.failProbe());
  await expect(inbox.locator(".sp-badge")).toHaveText("2");
  expect(mentionReads).toBe(2);
  expect(heldRead).toBeDefined();
  await reply(heldRead!.route, "mentions_listed", heldRead!.requestId, { mentions });
  await expect(region.locator(".sp-inbox-entry")).toHaveCount(2);
  await expect(region.getByText("Lastar omtalar.", { exact: true })).toHaveCount(0);
  await expect(region.getByRole("heading", { name: "Ingen omtalar", exact: true })).toHaveCount(0);
  await expect(app.locator(".sp-connection-logo-mark")).toHaveCount(0);
});

for (const width of [1280, 390]) {
  test(`offline recovery is visible, singleflight and preserves the document at ${width}px`, async ({ page, context }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    const commands: string[] = [];
    const errors: string[] = [];
    let holdSession = false;
    let held: Route | undefined;
    let recoveryRequests = 0;
    let countRecovery = false;
    const history = Array.from({ length: 35 }, (_, index) => ({ id: `recovery-message-${index + 1}`, channel_id: channelId, parent_message_id: null,
      sender_id: "peer", sender_display_name: "Ada", body: `Melding ${index + 1}. ${"Bevar lesestaden medan sambandet blir reparert. ".repeat(20)}`,
      sequence: index + 1, sent_at: "2026-01-01T12:00:00Z", edited_at: null, deleted_at: null }));
    page.on("pageerror", error => errors.push(error.message));
    await page.route(/\/api\/v1\/me\/favorite-circles(?:\?.*)?$/, route => route.fulfill({ json: [] }));
    await page.route("**/auth/session", async route => {
      if (countRecovery) recoveryRequests++;
      if (holdSession) { held = route; return; }
      await route.fulfill({ json: { refresh_after_seconds: 3600 } });
    });
    await page.routeWebSocket(/\/ws(?:\?|$)/, socket => {
      socket.onMessage(data => {
        const command = JSON.parse(String(data)) as { type: string; request_id?: string; payload?: { channel_id?: string; sequence?: number } };
        commands.push(command.type);
        const reply = (type: string, payload: unknown = {}) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload, request_id: command.request_id }));
        switch (command.type) {
          case "hello": reply("hello", { participant_id: "recovery-reader" }); break;
          case "ping": reply("pong"); break;
          case "list_users": reply("users_listed", { users: [] }); break;
          case "list_my_circles": reply("circles_listed", { circles: [] }); break;
          case "list_my_channels": reply("channels_listed", { channels: [{ id: channelId, slug: "recovery", name: "Samtalen", kind: "public", circle_id: null,
            direct_user_id: null, is_direct: false, description: "", role: "member", last_read_sequence: 35, latest_sequence: 35 }] }); break;
          case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
          case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
          case "subscribe_channel": reply("subscription_started", { channel_id: channelId, history }); break;
          case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channelId, summaries: [] }); break;
          case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channelId, reactions: [] }); break;
          case "mark_read": reply("read_marker_updated", { membership: { channel_id: channelId, user_id: "recovery-reader", role: "member", last_read_sequence: command.payload?.sequence ?? 35 } }); break;
        }
      });
    });
    await page.goto(`/?participant=recovery&ui=react&channel=${channelId}`);
    const app = page.locator("#sproyt-react-preview");
    const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
    await expect(composer).toBeEnabled();
    const composerNode = await composer.elementHandle();
    await expect(app.locator(".sp-connection-logo-mark")).toHaveCount(0);
    await expect(app.locator(".sp-connection-health:not(.sp-connection-health-details)")).toHaveCount(0);
    await expect(app.locator(".sp-connection-status, .sp-connection-dot")).toHaveCount(0);
    await composer.fill("Bevar dette gjennom sambandsreparasjonen");
    const timeline = app.locator(".sp-channel-pane > .sp-timeline");
    await expect(timeline.locator('[data-message-id="recovery-message-20"]')).toBeAttached();
    await timeline.evaluate(element => {
      const message = element.querySelector<HTMLElement>('[data-message-id="recovery-message-20"]')!;
      element.dispatchEvent(new WheelEvent("wheel", { deltaY: -400 }));
      element.scrollTop += message.getBoundingClientRect().top - element.getBoundingClientRect().top - 24;
      element.dispatchEvent(new Event("scroll"));
    });
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    const anchor = await timeline.evaluate(element => {
      const bounds = element.getBoundingClientRect();
      const item = [...element.querySelectorAll<HTMLElement>("[data-message-id]")].find(item => item.getBoundingClientRect().bottom > bounds.top + 1)!;
      return { id: item.dataset.messageId!, offset: item.getBoundingClientRect().top - bounds.top };
    });
    const before = page.url();
    const documentToken = await page.evaluate(() => {
      const token = crypto.randomUUID();
      (window as Window & { recoveryDocument?: string }).recoveryDocument = token;
      return token;
    });
    await context.setOffline(true);
    const retry = app.getByRole("button", { name: "Kopla til att", exact: true });
    await expect(retry).toBeVisible();
    await expect(app.locator(".sp-connection-logo-mark:visible")).toHaveAttribute("data-phase", "offline");
    await page.screenshot({ path: testInfo.outputPath(`offline-${width}.png`) });
    await retry.click();
    await expect(app.locator(".sp-connection-logo-mark:visible")).toHaveAttribute("data-phase", "offline");
    await expect(composer).toHaveValue("Bevar dette gjennom sambandsreparasjonen");
    await expect(page).toHaveURL(before);
    expect(await page.evaluate(() => (window as Window & { recoveryDocument?: string }).recoveryDocument)).toBe(documentToken);
    // WebKit rejects physical offline requests before route interception. Restore
    // the actual network and finish automatic recovery before holding HTTP.
    await context.setOffline(false);
    await expect(app.locator(".sp-connection-logo-mark")).toHaveCount(0);
    await expect(retry).toHaveCount(0);
    await expect(composer).toBeEnabled();
    await app.getByRole("button", { name: "Innstillingar", exact: true }).click();
    await page.locator(".sp-main [data-workspace-page]").getByRole("button", { name: "Hjelp og om Sprøyt", exact: true }).click();
    const help = page.locator(".sp-main [data-workspace-page]");
    await expect(help.getByRole("heading", { name: "Hjelp og om Sprøyt", exact: true })).toBeVisible();
    const manualRetry = help.getByRole("button", { name: "Kopla til att", exact: true });
    await expect(manualRetry).toBeEnabled();
    const helpURL = page.url();
    recoveryRequests = 0; held = undefined; holdSession = true; countRecovery = true;
    // Dispatch two real activations in one turn: the second must share the
    // pending operation even before React paints the disabled state.
    await manualRetry.evaluate(button => { (button as HTMLButtonElement).click(); (button as HTMLButtonElement).click(); });
    await expect.poll(() => recoveryRequests).toBe(1);
    await expect(manualRetry).toBeDisabled();
    expect(held).toBeDefined();
    expect(await composerNode!.evaluate(node => ({ connected: node.isConnected, value: (node as HTMLTextAreaElement).value })))
      .toEqual({ connected: true, value: "Bevar dette gjennom sambandsreparasjonen" });
    await expect(page).toHaveURL(helpURL);
    expect(recoveryRequests).toBe(1);
    holdSession = false;
    await held!.fulfill({ json: { refresh_after_seconds: 3600 } });
    await expect(manualRetry).toBeEnabled();
    await expect(app.locator(".sp-connection-logo-mark")).toHaveCount(0);
    expect(recoveryRequests).toBe(1);
    await help.getByRole("button", { name: "Tilbake til innstillingar", exact: true }).click();
    await page.locator(".sp-main [data-workspace-page]").getByRole("button", { name: "Lukk innstillingane", exact: true }).click();
    await expect(composer).toBeVisible();
    await expect(composer).toHaveValue("Bevar dette gjennom sambandsreparasjonen");
    await expect(page).toHaveURL(before);
    expect(await page.evaluate(() => (window as Window & { recoveryDocument?: string }).recoveryDocument)).toBe(documentToken);
    await expect.poll(() => timeline.locator(`[data-message-id="${anchor.id}"]`).evaluate((element, offset) =>
      Math.abs(element.getBoundingClientRect().top - element.closest(".sp-timeline")!.getBoundingClientRect().top - offset), anchor.offset)).toBeLessThan(3);
    expect(recoveryRequests).toBe(1);
    expect(commands.filter(command => command === "send_message")).toEqual([]);
    expect(errors).toEqual([]);
    await page.screenshot({ path: testInfo.outputPath(`reconnected-${width}.png`) });
  });
}
