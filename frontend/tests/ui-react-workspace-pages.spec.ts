import { expect, test, type Locator, type Page, type WebSocketRoute } from "@playwright/test";
import type { ChatMessage, UserProfile } from "../src/types";

const channelId = "00000000-0000-7000-8000-000000009101";
const globalId = "00000000-0000-7000-8000-000000009102";
const circleId = "00000000-0000-7000-8000-000000009001";
const owner = "workspace-reader";
const messageId = (sequence: number) => `workspace-${sequence}`;
const profile: UserProfile = { id: owner, kind: "human", display_name: "Kari", handle: "kari", external_provider: null,
  external_subject: null, created_at: "2026-01-01T00:00:00Z", status_text: "", status_emoji: "", status_expires_at: null, early_adopter: false };
type Command = { type: string; request_id?: string; payload?: { channel_id?: string; sequence?: number; before?: number; limit?: number } };

/** Matching request IDs and actual subscription history exercise the application,
 * including its routing, unread counts and scroll controller. */
async function fixture(page: Page, options: { holdMentions?: boolean } = {}) {
  const commands: Command[] = [];
  const errors: string[] = [];
  let socket: WebSocketRoute;
  const mentionReads: Command[] = [];
  let account = owner;
  const messages: ChatMessage[] = Array.from({ length: 50 }, (_, index) => ({ id: messageId(index + 1), channel_id: channelId,
    parent_message_id: null, sender_id: "peer", sender_display_name: "Ada", sequence: index + 1,
    body: `Melding ${index + 1}. ${"Her er den lange samanhengen i samtalen. ".repeat(24)}`,
    sent_at: "2026-01-01T12:00:00Z", edited_at: null, deleted_at: null }));
  const channels = [{ id: channelId, slug: "prat", name: "Prat", circle_id: circleId, kind: "public", direct_user_id: null,
    is_direct: false, description: "Samtalen vår", role: "member", last_read_sequence: 50, latest_sequence: 50 },
  { id: globalId, slug: "felles", name: "Felles prat", circle_id: null, kind: "public", direct_user_id: null,
    is_direct: false, description: "Open samtale", role: "member", last_read_sequence: 2, latest_sequence: 202 }];
  const emit = (type: string, payload: unknown, request_id?: string) => socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, payload, request_id }));
  page.on("pageerror", error => errors.push(error.message));
  await page.route(/\/api\/v1\/me\/favorite-circles(?:\?.*)?$/, route => route.fulfill({ json: [] }));
  await page.route("**/api/v1/me/statuses", route => route.fulfill({ json: [] }));
  await page.route("**/auth/session", route => route.fulfill({ json: { refresh_after_seconds: 3600 } }));
  await page.route("**/api/v1/me/notifications?*", route => route.fulfill({ json: { enabled: false, subscriptions: 0, public_key: null,
    channel_ids: [], preferences: { mode: "instant", direct_messages: true, mentions: true } } }));
  await page.routeWebSocket(/\/ws(?:\?|$)/, route => {
    socket = route;
    route.onMessage(data => {
      const command = JSON.parse(String(data)) as Command; commands.push(command);
      const reply = (type: string, payload: unknown = {}) => emit(type, payload, command.request_id);
      const channel = command.payload?.channel_id ?? channelId;
      switch (command.type) {
        case "hello": reply("hello", { participant_id: account }); break;
        case "ping": reply("pong"); break;
        case "list_users": reply("users_listed", { users: [profile] }); break;
        case "list_my_circles": reply("circles_listed", { circles: [[{ id: circleId, name: "Paros", slug: "paros", created_by: owner, created_at: profile.created_at }, "member"]] }); break;
        case "list_my_channels": reply("channels_listed", { channels }); break;
        case "list_mentions":
          mentionReads.push(command);
          if (!options.holdMentions) reply("mentions_listed", { mentions: [20, 25].map(sequence => ({ read: false, channel_name: "Prat", message: messages[sequence - 1] })) });
          break;
        case "list_tasks": reply("tasks_listed", { tasks: Array.from({ length: 8 }, (_, index) => ({ id: `task-${index}`, source_message_id: messageId(20), channel_id: channelId,
          channel_name: "Prat", assignee_id: owner, created_by: "peer", process_link_id: null, title: `Oppgåve ${index}`, status: "open", created_at: profile.created_at, completed_at: null })) }); break;
        case "accept_invitation": reply("error", { code: "invalid_token", message: "Invitasjonen er ikkje gyldig" }); break;
        case "subscribe_channel": reply("subscription_started", { channel_id: channel, history: channel === channelId ? messages : [] }); break;
        case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: channel, summaries: [] }); break;
        case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: channel, reactions: [] }); break;
        case "load_recent_messages": reply("messages_loaded", { channel_id: channel, messages: messages.filter(message => message.sequence < (command.payload?.before ?? Infinity)).slice(-(command.payload?.limit ?? 50)) }); break;
        case "list_channel_users": reply("channel_users_listed", { channel_id: channel, users: [profile] }); break;
        case "mark_read": reply("read_marker_updated", { membership: { channel_id: channel, user_id: owner, role: "member", last_read_sequence: command.payload?.sequence ?? 50 } }); break;
      }
    });
  });
  await page.goto(`/?participant=workspace&ui=react&channel=${channelId}`);
  const app = page.locator("#sproyt-react-preview");
  const composer = app.getByRole("textbox", { name: "Skriv melding", exact: true });
  await expect(composer).toBeEnabled();
  return { app, composer, commands, errors, mentionReads,
    replyMentions(command: Command, sequences: number[] = []) {
      emit("mentions_listed", { mentions: sequences.map(sequence => ({ read: false, channel_name: "Prat", message: messages[sequence - 1] })) }, command.request_id);
    },
    failMentions(command: Command) { emit("error", { code: "unavailable", message: "Omtalane kunne ikkje hentast. Prøv igjen." }, command.request_id); },
    switchOwner(next: string) { account = next; emit("hello", { participant_id: next }); },
    append() {
    const message: ChatMessage = { ...messages[0]!, id: messageId(messages.length + 1), sequence: messages.length + 1, body: "Ny melding medan innstillingane er opne." };
    messages.push(message); emit("chat", { event: { type: "message_accepted", message } }); return message;
  } };
}

const timeline = (page: Page) => page.locator("#sproyt-react-preview .sp-channel-pane > .sp-timeline");
const frame = (page: Page) => page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
async function ordinaryPage(page: Page, title: string) {
  const region = page.locator(".sp-main [data-workspace-page]");
  await expect(region).toHaveCount(1);
  await expect(region.getByRole("heading", { name: title, level: 1, exact: true })).toBeVisible();
  await expect(page.locator("dialog[open]")).toHaveCount(0);
  return region;
}
async function anchor(viewport: Locator) {
  return viewport.evaluate(element => {
    const bounds = element.getBoundingClientRect();
    const item = [...element.querySelectorAll<HTMLElement>("[data-message-id]")].find(item => item.getBoundingClientRect().bottom > bounds.top + 1)!;
    return { id: item.dataset.messageId!, offset: item.getBoundingClientRect().top - bounds.top };
  });
}

test("inbox distinguishes held and failed reads and fences old account generations", async ({ page }) => {
  const f = await fixture(page, { holdMentions: true });
  await expect.poll(() => f.mentionReads.length).toBe(1);
  const bootstrap = f.mentionReads[0]!;
  const inbox = () => f.app.getByRole("button", { name: /^Innboks(?:,|$)/ });
  await inbox().click();
  let region = await ordinaryPage(page, "Innboks");
  await region.getByRole("button", { name: "Omtalar", exact: true }).click();
  await expect.poll(() => f.mentionReads.length).toBe(2);
  await expect(region.getByText("Lastar omtalar.", { exact: true })).toBeVisible();
  await expect(region.getByRole("heading", { name: "Ingen omtalar", exact: true })).toHaveCount(0);
  f.replyMentions(bootstrap, [20, 25]);
  await frame(page);
  await expect(region.getByText("Lastar omtalar.", { exact: true })).toBeVisible();
  await expect(inbox().locator(".sp-badge")).toHaveCount(0);
  f.failMentions(f.mentionReads[1]!);
  await expect(region.getByText("Omtalane kunne ikkje hentast. Prøv igjen.", { exact: false })).toBeVisible();
  await expect(region.getByRole("heading", { name: "Ingen omtalar", exact: true })).toHaveCount(0);
  await region.getByRole("button", { name: "Prøv igjen", exact: true }).click();
  await expect.poll(() => f.mentionReads.length).toBe(3);
  f.replyMentions(f.mentionReads[2]!, [20, 25]);
  await expect(inbox().locator(".sp-badge")).toHaveText("2");
  await expect(region.locator(".sp-inbox-entry")).toHaveCount(2);
  f.switchOwner("workspace-other-reader");
  await expect.poll(() => f.mentionReads.length).toBeGreaterThanOrEqual(4);
  await expect(inbox().locator(".sp-badge")).toHaveCount(0);
  await inbox().click();
  region = await ordinaryPage(page, "Innboks");
  await region.getByRole("button", { name: "Omtalar", exact: true }).click();
  await expect(region.getByText("Lastar omtalar.", { exact: true })).toBeVisible();
  await frame(page);
  const otherRead = f.mentionReads.at(-1)!;
  f.replyMentions(bootstrap, [20, 25]);
  await frame(page);
  await expect(region.locator(".sp-inbox-entry")).toHaveCount(0);
  f.replyMentions(otherRead);
  await expect(region.getByRole("heading", { name: "Ingen omtalar", exact: true })).toBeVisible();
  f.switchOwner(owner);
  await expect.poll(() => f.mentionReads.at(-1)?.request_id).not.toBe(otherRead.request_id);
  await inbox().click();
  region = await ordinaryPage(page, "Innboks");
  const mentions = region.getByRole("button", { name: "Omtalar", exact: true });
  if (await mentions.getAttribute("aria-pressed") !== "true") await mentions.click();
  await expect(region.getByText("Lastar omtalar.", { exact: true })).toBeVisible();
  await frame(page);
  const returnedRead = f.mentionReads.at(-1)!;
  f.replyMentions(bootstrap, [20, 25]);
  f.replyMentions(otherRead, [20]);
  await frame(page);
  await expect(inbox().locator(".sp-badge")).toHaveCount(0);
  await expect(region.locator(".sp-inbox-entry")).toHaveCount(0);
  f.replyMentions(returnedRead);
  await expect(region.getByRole("heading", { name: "Ingen omtalar", exact: true })).toBeVisible();
  expect(f.errors).toEqual([]);
});

test("delayed page heading focus preserves an invitation field already being edited", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const { app, errors } = await fixture(page);
  await app.getByRole("navigation", { name: "Samtalar", exact: true }).getByRole("button", { name: "Kom i gang", exact: true }).click();
  const guide = await ordinaryPage(page, "Kom i gang");
  await expect(guide.getByRole("heading", { name: "Kom i gang", exact: true, level: 1 })).toBeFocused();
  // Hold the page's pending frames so input can deterministically arrive before
  // the navigation focus callback, rather than depending on runner timing.
  await page.evaluate(() => {
    const original = window.requestAnimationFrame, cancel = window.cancelAnimationFrame;
    const pending = new Map<number, FrameRequestCallback>();
    let id = -1;
    window.requestAnimationFrame = callback => { pending.set(id, callback); return id--; };
    window.cancelAnimationFrame = handle => { if (!pending.delete(handle)) cancel.call(window, handle); };
    (window as typeof window & { __flushPageFocusFrames?: () => void }).__flushPageFocusFrames = () => {
      window.requestAnimationFrame = original; window.cancelAnimationFrame = cancel;
      for (const callback of pending.values()) callback(performance.now());
      pending.clear();
    };
  });
  await guide.getByRole("button", { name: "Bruk invitasjonslenkje eller kode", exact: true }).evaluate(button => (button as HTMLButtonElement).click());
  const invitation = await ordinaryPage(page, "Bruk invitasjon");
  const token = invitation.getByRole("textbox", { name: "Invitasjonskode eller lenkje", exact: true });
  await token.focus(); // focusin, without pointerdown or keydown
  await token.fill("kort");
  await page.evaluate(() => (window as typeof window & { __flushPageFocusFrames?: () => void }).__flushPageFocusFrames?.());
  await expect(token).toBeFocused();
  await expect(token).toHaveValue("kort");
  await invitation.getByRole("button", { name: "Godta invitasjon", exact: true }).click();
  await expect(invitation).toContainText(/gyldig invitasjonskode|Invitasjonen er ikkje gyldig/);
  await expect(invitation).not.toContainText("Invitasjonen er godteken");
  expect(errors).toEqual([]);
});

for (const width of [1280, 390]) {
  test(`ordinary settings preserve mounted chat, draft and reading anchor at ${width}px`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    const { app, composer, append, commands, errors } = await fixture(page);
    await composer.fill("Uferdig melding på lesestaden");
    await timeline(page).locator(`[data-message-id="${messageId(30)}"]`).waitFor();
    await frame(page);
    await timeline(page).evaluate((element, id) => {
      const message = element.querySelector<HTMLElement>(`[data-message-id="${id}"]`)!;
      element.dispatchEvent(new WheelEvent("wheel", { deltaY: -400 }));
      element.scrollTop += message.getBoundingClientRect().top - element.getBoundingClientRect().top - 24;
      element.dispatchEvent(new Event("scroll"));
    }, messageId(30));
    await frame(page);
    const saved = await anchor(timeline(page));
    const composerNode = await composer.elementHandle();
    const before = page.url();
    await app.getByRole("button", { name: "Innstillingar", exact: true }).click();
    let region = await ordinaryPage(page, "Innstillingar");
    await expect(region.getByRole("heading", { level: 1 })).toBeFocused();
    await expect(app.locator(".sp-discussion")).toHaveAttribute("inert", "");
    await expect(app.locator(".sp-discussion")).toBeHidden();
    await region.getByRole("button", { name: "Profil og status", exact: false }).click();
    region = await ordinaryPage(page, "Profil og status");
    await expect(region.getByLabel("Visningsnamn", { exact: true })).toHaveValue("Kari");
    await page.goBack();
    region = await ordinaryPage(page, "Innstillingar");
    await region.getByRole("button", { name: "Varslingsinnstillingar", exact: false }).click();
    region = await ordinaryPage(page, "Varslingsinnstillingar");
    await expect(region.getByLabel("Varslingsmodus", { exact: true })).toHaveValue("instant");
    const incoming = append();
    await expect(timeline(page).locator(`[data-message-id="${incoming.id}"]`)).toBeAttached();
    await expect(region.getByRole("heading", { level: 1 })).toBeFocused();
    await page.screenshot({ path: testInfo.outputPath(`settings-${width}.png`) });
    await page.goBack();
    region = await ordinaryPage(page, "Innstillingar");
    await region.getByRole("button", { name: "Lukk innstillingane", exact: true }).click();
    await expect(page).toHaveURL(before);
    await expect(composer).toHaveValue("Uferdig melding på lesestaden");
    expect(await composerNode!.evaluate(node => node.isConnected)).toBe(true);
    await expect.poll(async () => {
      const offset = await timeline(page).locator(`[data-message-id="${saved.id}"]`).evaluate(element => element.getBoundingClientRect().top - element.closest(".sp-timeline")!.getBoundingClientRect().top);
      return Math.abs(offset - saved.offset);
    }).toBeLessThan(3);
    expect(commands.filter(command => ["send_message", "create_channel", "set_user_profile"].includes(command.type))).toEqual([]);
    expect(errors).toEqual([]);
  });

  test(`Felles and channel details use typed ordinary destinations at ${width}px`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    const { app, commands, errors } = await fixture(page);
    await app.getByRole("button", { name: "Innstillingar", exact: true }).click();
    let region = await ordinaryPage(page, "Innstillingar");
    await region.getByRole("searchbox", { name: "Søk i innstillingar" }).fill("Felles");
    await region.getByRole("button", { name: "Kanalar i Felles", exact: false }).click();
    region = await ordinaryPage(page, "Kanalar i Felles");
    await expect.poll(() => new URL(page.url()).searchParams.get("task")).toBe("global-channels");
    await region.getByRole("button", { name: "Medlemmer i Felles prat", exact: true }).click();
    region = await ordinaryPage(page, "Kanaldetaljar: Felles prat");
    await expect.poll(() => new URL(page.url()).searchParams.get("channel")).toBe(globalId);
    await page.screenshot({ path: testInfo.outputPath(`channel-details-${width}.png`) });
    await page.goBack();
    await ordinaryPage(page, "Kanalar i Felles");
    expect(commands.filter(command => ["create_channel", "join_channel", "leave_channel"].includes(command.type))).toEqual([]);
    expect(errors).toEqual([]);
  });

  test(`inbox source and Back preserve mention-only badge at ${width}px`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 900 });
    const { app, composer, errors } = await fixture(page);
    await composer.fill("Utkast før innboksen");
    const trigger = app.getByRole("button", { name: "Innboks, 2 uleste omtalar", exact: true });
    await expect(trigger.locator(".sp-badge")).toHaveText("2");
    await trigger.click();
    const region = await ordinaryPage(page, "Innboks");
    await expect(region.getByRole("button", { name: "Uleste (100+)", exact: true })).toBeVisible();
    await expect(region.getByRole("button", { name: "Personlege oppgåver (8)", exact: true })).toBeVisible();
    await expect(region.getByRole("button", { name: "Omtalar (2)", exact: true })).toHaveAttribute("aria-pressed", "true");
    const inboxURL = page.url();
    const entry = region.locator(".sp-inbox-entry").filter({ hasText: "Melding 20." });
    await entry.getByRole("button", { name: "Opne kjelda", exact: true }).click();
    await expect.poll(() => new URL(page.url()).searchParams.get("message")).toBe(messageId(20));
    await expect(page.locator(".sp-main [data-workspace-page]")).toHaveCount(0);
    await expect(timeline(page).locator(`[data-message-id="${messageId(20)}"]`)).toBeInViewport();
    await page.goBack();
    await expect(page).toHaveURL(inboxURL);
    const restored = await ordinaryPage(page, "Innboks");
    await expect(restored.getByRole("button", { name: "Omtalar (2)", exact: true })).toHaveAttribute("aria-pressed", "true");
    await expect(trigger.locator(".sp-badge")).toHaveText("2");
    await page.screenshot({ path: testInfo.outputPath(`inbox-${width}.png`) });
    await page.goBack();
    await expect(composer).toHaveValue("Utkast før innboksen");
    expect(errors).toEqual([]);
  });
}
