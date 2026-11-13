import { HttpClient } from "./api";
import { decodeChatMessage, isRecord, type ChatMessage } from "./types";

export const eventResponses = ["attending", "declined", "following", "unanswered"] as const;
export type EventResponse = typeof eventResponses[number];
export type EventInput = Readonly<{ title: string; code: string; starts_at: string; timezone: string; ends_at: string | null; location: string | null; description: string }>;
export type ChannelEvent = EventInput & Readonly<{
  id: string; channel_id: string; creator_id: string; cancelled: boolean; revision: number; updated_at: string;
  /** Missing only when a new bundle overlaps the earlier event backend. */
  events_enabled?: boolean;
  cover_media_id: string | null; my_rsvp: EventResponse; can_manage: boolean;
  needs_attention: boolean; aliases: readonly string[]; images_next_after: string | null;
  images: readonly Readonly<{ media_id: string; contributor_id: string }>[];
}>;
export type EventPerson = Readonly<{ user_id: string; display_name: string; status: EventResponse }>;
export type EventMessages = Readonly<{ messages: readonly ChatMessage[]; next_before: number | null }>;
export type EventCollectionSummary = Readonly<{
  as_of: string; upcoming_count: number; ongoing_count: number;
  next_event: Pick<ChannelEvent, "id" | "title" | "starts_at" | "ends_at" | "timezone"> | null;
}>;
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu;
export function decodeEventCollectionSummary(value: unknown): EventCollectionSummary {
  if (!isRecord(value) || typeof value.as_of !== "string" || !Number.isFinite(Date.parse(value.as_of))
    || !["upcoming_count", "ongoing_count"].every(key => Number.isSafeInteger(value[key]) && Number(value[key]) >= 0))
    throw new Error("Ugyldig arrangementssamandrag.");
  const next = value.next_event;
  if (next !== null && (!isRecord(next) || typeof next.id !== "string" || !uuid.test(next.id)
    || typeof next.title !== "string" || typeof next.timezone !== "string"
    || typeof next.starts_at !== "string" || !Number.isFinite(Date.parse(next.starts_at))
    || (next.ends_at !== null && (typeof next.ends_at !== "string" || !Number.isFinite(Date.parse(next.ends_at))))))
    throw new Error("Ugyldig neste arrangement.");
  return value as unknown as EventCollectionSummary;
}
export function decodeChannelEvent(value: unknown, channelId: string): ChannelEvent {
  if (!isRecord(value) || !["id", "channel_id", "creator_id"].every(key => typeof value[key] === "string" && uuid.test(value[key] as string))
    || value.channel_id !== channelId || !["title", "code", "starts_at", "timezone", "description", "updated_at"].every(key => typeof value[key] === "string")
    || !/^[a-z0-9-]{1,48}$/u.test(value.code as string) || !Number.isFinite(Date.parse(value.starts_at as string))
    || !["ends_at", "location", "cover_media_id"].every(key => value[key] === null || typeof value[key] === "string")
    || (value.events_enabled !== undefined && typeof value.events_enabled !== "boolean")
    || typeof value.cancelled !== "boolean" || typeof value.can_manage !== "boolean" || !Number.isSafeInteger(value.revision)
    || typeof value.needs_attention !== "boolean" || !Array.isArray(value.aliases) || value.aliases.some(alias => typeof alias !== "string" || !/^[a-z0-9-]{1,48}$/u.test(alias))
    || !eventResponses.includes(value.my_rsvp as EventResponse) || !Array.isArray(value.images)
    || value.images.some(image => !isRecord(image) || typeof image.media_id !== "string" || !uuid.test(image.media_id) || typeof image.contributor_id !== "string"))
    throw new Error("Ugyldig arrangement.");
  if (value.cover_media_id !== null && !uuid.test(value.cover_media_id as string)) throw new Error("Ugyldig forsidebilete.");
  return value as unknown as ChannelEvent;
}

/** Every operation is bound to the account generation, including mutations. */
export class ChannelEventsApi {
  constructor(private readonly http: HttpClient, private readonly owner: () => string | null, private readonly epoch: () => number) {}
  #path(channelId: string, suffix = "") { return `/api/v1/channels/${encodeURIComponent(channelId)}/events${suffix}`; }
  async #request<T>(owner: string, channelId: string, suffix: string, decode: (value: unknown) => T, options: RequestInit = {}) {
    const epoch = this.epoch();
    const admitted = () => this.owner() === owner && this.epoch() === epoch;
    const result = await this.http.json(this.#path(channelId, suffix), decode, { ...options,
      headers: { ...Object.fromEntries(new Headers(options.headers)), "x-sproyt-account": owner, ...(options.body ? { "content-type": "application/json" } : {}) } }, admitted);
    if (!admitted()) throw new Error("Kontoen vart endra.");
    return result;
  }
  list(owner: string, channelId: string, signal: AbortSignal, after: string | null = null) {
    return this.#request(owner, channelId, after ? `?after=${encodeURIComponent(after)}` : "", value => {
      if (!isRecord(value) || !Array.isArray(value.events)) throw new Error("Ugyldig arrangementsoversikt.");
      if (value.next_after !== null && typeof value.next_after !== "string") throw new Error("Ugyldig arrangementsmarkør.");
      if (value.attention_count !== undefined && (!Number.isSafeInteger(value.attention_count) || Number(value.attention_count) < 0)) throw new Error("Ugyldig arrangementsvarsel.");
      if (value.events_enabled !== undefined && typeof value.events_enabled !== "boolean") throw new Error("Ugyldig arrangementsinnstilling.");
      return { events: value.events.map(event => decodeChannelEvent(event, channelId)), next_after: value.next_after,
        attention_count: typeof value.attention_count === "number" ? value.attention_count : null,
        events_enabled: typeof value.events_enabled === "boolean" ? value.events_enabled : undefined,
        summary: value.summary === undefined ? null : decodeEventCollectionSummary(value.summary) };
    }, { signal });
  }
  references(owner: string, channelId: string, codes: readonly string[], signal: AbortSignal) {
    return this.#request(owner, channelId, "/references", value => {
      if (!isRecord(value) || !Array.isArray(value.events)) throw new Error("Ugyldige arrangementsreferansar.");
      return value.events.map(event => decodeChannelEvent(event, channelId));
    }, { method: "POST", body: JSON.stringify({ codes: codes.slice(0, 50) }), signal });
  }
  suggestions(owner: string, channelId: string, prefix: string, signal: AbortSignal) {
    return this.#request(owner, channelId, `?prefix=${encodeURIComponent(prefix)}`, value => {
      if (!isRecord(value) || !Array.isArray(value.events) || value.events.length > 12) throw new Error("Ugyldige arrangementsforslag.");
      return value.events.map(event => decodeChannelEvent(event, channelId));
    }, { signal });
  }
  search(owner: string, channelId: string, query: string, signal: AbortSignal) {
    return this.#request(owner, channelId, `?search=${encodeURIComponent(query)}`, value => {
      if (!isRecord(value) || !Array.isArray(value.events) || value.events.length > 12) throw new Error("Ugyldige arrangementstreff.");
      return value.events.map(event => decodeChannelEvent(event, channelId));
    }, { signal });
  }
  detail(owner: string, channelId: string, id: string, signal?: AbortSignal) {
    return this.#request(owner, channelId, `/${encodeURIComponent(id)}`, value => decodeChannelEvent(value, channelId), { signal });
  }
  save(owner: string, channelId: string, input: EventInput, id?: string, requestId?: string) {
    return this.#request(owner, channelId, id ? `/${encodeURIComponent(id)}` : "", value => decodeChannelEvent(value, channelId),
      { method: id ? "PUT" : "POST", body: JSON.stringify(input), ...(!id && requestId ? { headers: { "Idempotency-Key": requestId } } : {}) });
  }
  change(owner: string, channelId: string, id: string, action: "cancel" | "acknowledge" | "rsvp" | "images" | "cover", body?: unknown) {
    return this.#request(owner, channelId, `/${encodeURIComponent(id)}/${action}`, value => decodeChannelEvent(value, channelId),
      { method: action === "rsvp" || action === "cover" ? "PUT" : "POST", ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  }
  removeImage(owner: string, channelId: string, id: string, mediaId: string) {
    return this.#request(owner, channelId, `/${encodeURIComponent(id)}/images/${encodeURIComponent(mediaId)}`, value => decodeChannelEvent(value, channelId), { method: "DELETE" });
  }
  responses(owner: string, channelId: string, id: string, signal: AbortSignal, after: string | null = null) {
    return this.#request(owner, channelId, `/${encodeURIComponent(id)}/responses${after ? `?after=${encodeURIComponent(after)}` : ""}`, value => {
      if (!isRecord(value) || !Array.isArray(value.responses) || value.responses.some(person => !isRecord(person)
        || typeof person.user_id !== "string" || typeof person.display_name !== "string" || !eventResponses.includes(person.status as EventResponse))) throw new Error("Ugyldige deltakarsvar.");
      if (value.next_after !== null && typeof value.next_after !== "string") throw new Error("Ugyldig svarmarkør.");
      const counts = isRecord(value.counts) && eventResponses.every(status => Number.isSafeInteger(value.counts && (value.counts as Record<string, unknown>)[status]) && Number((value.counts as Record<string, unknown>)[status]) >= 0)
        ? value.counts as Record<EventResponse, number> : null;
      return { responses: value.responses as EventPerson[], next_after: value.next_after, counts };
    }, { signal });
  }
  images(owner: string, channelId: string, id: string, after: string, signal: AbortSignal) {
    return this.#request(owner, channelId, `/${encodeURIComponent(id)}/images?after=${encodeURIComponent(after)}`, value => {
      if (!isRecord(value) || !Array.isArray(value.images) || value.images.some(image => !isRecord(image) || typeof image.media_id !== "string" || !uuid.test(image.media_id) || typeof image.contributor_id !== "string")
        || (value.next_after !== null && typeof value.next_after !== "string")) throw new Error("Ugyldig biletbibliotek.");
      return { images: value.images as ChannelEvent["images"], next_after: value.next_after as string | null };
    }, { signal });
  }
  messages(owner: string, channelId: string, id: string, before: number | null, signal: AbortSignal) {
    return this.#request(owner, channelId, `/${encodeURIComponent(id)}/messages${before === null ? "" : `?before=${before}`}`, value => {
      if (!isRecord(value) || !Array.isArray(value.messages) || (value.next_before !== null && typeof value.next_before !== "number")) throw new Error("Ugyldige arrangementstreff.");
      const messages = value.messages.map(decodeChatMessage);
      if (messages.some(message => message.channel_id !== channelId || message.deleted_at)) throw new Error("Meldingane gjeld ein annan kanal.");
      return { messages, next_before: value.next_before } as EventMessages;
    }, { signal });
  }
}

/** One bounded, shared discovery queue for visible root and thread message bodies. */
export class EventReferenceDiscovery {
  readonly #cache = new Map<string, ChannelEvent | null>();
  readonly #pending = new Set<string>();
  readonly #controller = new AbortController();
  #running = false;
  constructor(private readonly api: ChannelEventsApi, private readonly owner: string, private readonly channelId: string, private readonly changed: () => void) {}
  events() { return [...new Map([...this.#cache.values()].filter((event): event is ChannelEvent => event !== null).map(event => [event.id, event])).values()]; }
  dispose() { this.#controller.abort(); this.#cache.clear(); this.#pending.clear(); }
  collect(codes: ReadonlySet<string>) {
    for (const code of codes) if (!this.#cache.has(code) && this.#pending.size < 512) this.#pending.add(code);
    if (this.#running || !this.#pending.size || this.#controller.signal.aborted) return;
    this.#running = true;
    queueMicrotask(async () => {
      try {
        while (this.#pending.size && !this.#controller.signal.aborted) {
          const batch = [...this.#pending].slice(0, 50);
          const events = await this.api.references(this.owner, this.channelId, batch, this.#controller.signal);
          if (this.#controller.signal.aborted) return;
          for (const code of batch) {
            this.#pending.delete(code);
            this.#cache.set(code, events.find(event => event.code === code || event.aliases.includes(code)) ?? null);
          }
          while (this.#cache.size > 512) this.#cache.delete(this.#cache.keys().next().value!);
          if (events.length) this.changed();
        }
      } catch {
        if (!this.#controller.signal.aborted) {
          for (const code of this.#pending) this.#cache.set(code, null);
          this.#pending.clear();
          while (this.#cache.size > 512) this.#cache.delete(this.#cache.keys().next().value!);
        }
      } finally { this.#running = false; }
    });
  }
}
