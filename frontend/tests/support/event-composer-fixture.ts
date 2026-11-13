import { readFile } from "node:fs/promises";
import { expect, type Page, type WebSocketRoute } from "@playwright/test";
import { eventImage } from "./event-image";

const owner = "events-viewer";
const channel = "00000000-0000-7000-8000-000000009911";
const eventId = "00000000-0000-7000-8000-000000009912";
const mediaId = "00000000-0000-7000-8000-000000009913";
const date = "2026-01-01T00:00:00Z";
const row = (id: string, sequence: number, body: string) => ({ id, channel_id: channel, parent_message_id: null, sequence, body, sent_at: date, sender_id: owner, sender_display_name: "Nora" });
export async function eventComposerFixture(page: Page, deep = false, offPage = false, initialEnabled = true, topics = false, eventOverride?: Partial<{ title: string; code: string }>) {
  let owner = "events-viewer";
  const other = "00000000-0000-7000-8000-000000009921";
  const sends: { channel_id: string; body: string }[] = [];
  let rejectNext = false;
  let enabled = initialEnabled;
  let socket: WebSocketRoute | undefined;
  const image = await eventImage(page);
  await page.route("**/api/v1/me/favorite-circles?**", route => route.fulfill({ json: [] }));
  const bundle = await readFile(new URL("../../dist/app.js", import.meta.url), "utf8");
  if (process.env.SPROYT_E2E_EMBEDDED_CLIENT !== "1") await page.route(/\/assets\/app\/[^/]+\/app.js$/, route => route.fulfill({ contentType: "text/javascript", body: bundle }));
  let event = { id: eventId, channel_id: channel, creator_id: channel, events_enabled: enabled, title: "Konsert med eit langt og lesbart namn", code: "konsert", aliases: ["gammal"],
    starts_at: "2027-01-01T18:00:00Z", timezone: "Europe/Oslo", ends_at: null, location: "Grendahuset", description: "**Ta med vener**", cancelled: false,
    revision: 2, updated_at: date, cover_media_id: mediaId as string | null, my_rsvp: "following", can_manage: true, needs_attention: true, images: [{ media_id: mediaId, contributor_id: owner }], images_next_after: null,
    ...eventOverride };
  let summaryOverride: Record<string, unknown> | undefined;
  const summary = () => summaryOverride ?? { as_of: "2026-11-13T12:00:00Z", upcoming_count: event.cancelled ? 0 : offPage ? 101 : 1, ongoing_count: 0, next_event: event.cancelled ? null : event };
  let revoked = false;
  const changes: string[] = [];
  const prefixes: string[] = [];
  const collectionReads: (string | null)[] = [];
  await page.route(new RegExp(`/api/v1/channels/${channel}/events(?:[/?]|$)`), route => {
    if (revoked) return route.fulfill({ status: 403, json: { error: "Tilgangen er fjerna" } });
    const path = new URL(route.request().url()).pathname;
    const search = new URL(route.request().url()).searchParams.get("search");
    if (search !== null) return route.fulfill({ json: { events: [{ ...event, id: "00000000-0000-7000-8000-000000009922", code: "samling-b", aliases: [], title: "Samling B utanfor første side" }], next_after: null } });
    const prefix = new URL(route.request().url()).searchParams.get("prefix");
    if (prefix !== null) { prefixes.push(prefix); return route.fulfill({ json: { events: [event], next_after: null } }); }
    const after = new URL(route.request().url()).searchParams.get("after");
    if (path.endsWith("/events")) collectionReads.push(after);
    if (offPage && after) return route.fulfill({ json: { events: [event], next_after: null, attention_count: 7, summary: summary() } });
    expect(route.request().headers()["x-sproyt-account"]).toBe(owner);
    if (path.endsWith("/responses")) return route.fulfill({ json: { responses: [{ user_id: owner, display_name: "Nora", status: event.my_rsvp }], next_after: null } });
    if (path.endsWith("/references")) return route.fulfill({ json: { events: [event] } });
    if (path.endsWith("/messages")) return route.fulfill({ json: { messages: [row("match", 8, `**Rik melding** #arr:konsert\n\n[[media:${mediaId}|image/png|konsert.png]]`)], next_before: null } });
    if (route.request().method() !== "GET") {
      changes.push(path.split("/").at(-1)!);
      if (path.endsWith("/rsvp")) event = { ...event, my_rsvp: route.request().postDataJSON().status };
      else if (path.endsWith("/acknowledge")) event = { ...event, needs_attention: false };
      else if (path.endsWith("/cancel")) event = { ...event, cancelled: true };
      else if (route.request().method() === "PUT") event = { ...event, ...route.request().postDataJSON() };
      else if (path.endsWith("/images")) event = { ...event, images: [{ media_id: mediaId, contributor_id: owner }] };
      return route.fulfill({ json: event });
    }
    return route.fulfill({ json: path.endsWith("/events") ? offPage ? { events: Array.from({ length: 100 }, (_, index) => ({ ...event, id: `00000000-0000-7000-8000-${String(index + 20000).padStart(12, "0")}`, title: `Andre arrangement ${index}`, code: `event-${index}`, aliases: [], needs_attention: false })), next_after: "next-page", attention_count: 7, summary: summary() } : { events: [event], next_after: null, attention_count: event.needs_attention ? 1 : 0, summary: summary() } : event });
  });
  await page.route(new RegExp(`/api/v1/channels/${channel}/topics(?:[/?]|$)`), route => route.fulfill({ json:
    new URL(route.request().url()).pathname.endsWith("/matches")
      ? { matches: [{ message_id: "topic-match", parent_message_id: null, sequence: 7, body: "Turprat", sender_display_name: "Nora", created_at: date, message: row("topic-match", 7, "**Turprat**") }], next_before: null }
      : { enabled: topics, can_manage: true, topics: topics ? [{ key: "tur", label: "Tur", count: 2 }, { key: "mat", label: "Mat og oppskrifter", count: 3 }, { key: "hus", label: "Hus og hage", count: 2 }] : [] }
  }));
  await page.route(new RegExp(`/api/v1/media/${mediaId}(?:[/?]|$)`), route => new URL(route.request().url()).pathname.endsWith("/status")
    ? route.fulfill({ json: { state: "legacy" } }) : route.fulfill({ contentType: "image/png", body: image }));
  await page.route(new RegExp(`/api/v1/channels/${other}/events(?:[/?]|$)`), route => route.fulfill({ json: { events: [], next_after: null, attention_count: 0, summary: { as_of: date, upcoming_count: 0, ongoing_count: 0, next_event: null } } }));
  await page.route(/\/api\/v1\/media-uploads\/config(?:\?|$)/, route => route.fulfill({ json: { enabled: false } }));
  await page.route(new RegExp(`/channels/${channel}/media(?:[?]|$)`), route => route.fulfill({ json: { media: { id: mediaId, channel_id: channel, original_filename: "foto.png", content_type: "image/png" } } }));
  await page.routeWebSocket(/\/ws(?:\?|$)/, connection => { socket = connection; connection.onMessage(data => {
    const command = JSON.parse(String(data));
    const reply = (type: string, payload: unknown = {}) => connection.send(JSON.stringify({ protocol: "sproyt.chat.v1", type, request_id: command.request_id, payload }));
    switch (command.type) {
      case "hello": reply("hello", { participant_id: owner }); break;
      case "ping": reply("pong"); break;
      case "list_users": reply("users_listed", { users: [] }); break;
      case "list_my_circles": reply("circles_listed", { circles: [] }); break;
      case "list_my_channels": reply("channels_listed", { channels: [{ id: other, slug: "annan", name: "Annan kanal", kind: "public", circle_id: null, direct_user_id: null, description: "", role: "owner", last_read_sequence: 0, latest_sequence: 0, events_enabled: true }, { id: channel, slug: "events", events_enabled: enabled, name: "Arrangementskanalen", kind: "public", circle_id: null, direct_user_id: null, description: "", role: "owner", last_read_sequence: 10, latest_sequence: 10 }] }); break;
      case "list_channel_users": reply("channel_users_listed", { channel_id: channel, users: [] }); break;
      case "list_mentions": reply("mentions_listed", { mentions: [] }); break;
      case "list_tasks": reply("tasks_listed", { tasks: [] }); break;
      case "subscribe_channel": reply("subscription_started", { channel_id: command.payload.channel_id, history: command.payload.channel_id === channel ? [row("latest", 10, offPage ? "Hei kanalen" : "Hei #arr:gammal")] : [] }); break;
      case "list_thread_summaries": reply("thread_summaries_listed", { channel_id: command.payload.channel_id, summaries: [] }); break;
      case "list_channel_reactions": reply("channel_reactions_listed", { channel_id: command.payload.channel_id, reactions: [] }); break;
      case "load_recent_messages": reply("messages_loaded", { channel_id: command.payload.channel_id, messages: [] }); break;
      case "send_message":
        sends.push(command.payload);
        if (rejectNext) { rejectNext = false; setTimeout(() => reply("error", { code: "forbidden", message: "Sendinga vart avvist" }), 30); }
        else reply("message_accepted", { message: { ...row(`sent-${sends.length}`, 11 + sends.length, command.payload.body), channel_id: command.payload.channel_id, sender_id: owner } });
        break;
    }
  }); });
  await page.goto(`/?participant=${owner}&channel=${channel}&ui=react${deep ? `&event=${eventId}` : ""}`);
  const app = page.locator("#sproyt-react-preview");
  if (enabled) await expect(app.getByRole("tab", { name: /Arrangement/ })).toBeVisible();
  else if (deep) await expect(app.getByRole("heading", { name: /Konsert med/, level: 2 })).toBeVisible();
  else await expect(app.getByRole("textbox", { name: "Skriv melding", exact: true })).toBeEnabled();
  return { app, sends, rejectNext: () => { rejectNext = true; }, switchAccount: () => { owner = "event-writer-b"; socket!.send(JSON.stringify({ protocol: "sproyt.chat.v1", type: "hello", payload: { participant_id: owner } })); }, changes, prefixes, collectionReads, getEvent: () => event, setSummary: (value: Record<string, unknown>) => { summaryOverride = value; }, revoke: () => { revoked = true; }, setEnabled(value: boolean) {
    enabled = value; event = { ...event, events_enabled: value };
    socket!.send(JSON.stringify({ protocol: "sproyt.chat.v1", type: "circles_changed" }));
  } };
}
