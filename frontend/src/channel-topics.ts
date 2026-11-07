import { HttpClient } from "./api";
import { decodeChatMessage, isRecord, type ChatMessage } from "./types";

export type ChannelTopic = Readonly<{ key: string; label: string; count: number }>;
export type ChannelTopicSummary = Readonly<{ enabled: boolean; can_manage: boolean; topics: readonly ChannelTopic[] }>;
export type ChannelTopicMatch = Readonly<{
  message_id: string; parent_message_id: string | null; sequence: number;
  message: ChatMessage;
}>;
export type ChannelTopicPage = Readonly<{ matches: readonly ChannelTopicMatch[]; next_before: number | null }>;

function decodeSummary(value: unknown): ChannelTopicSummary {
  if (!isRecord(value) || typeof value.enabled !== "boolean" || typeof value.can_manage !== "boolean" || !Array.isArray(value.topics))
    throw new Error("Ugyldig emneoversikt");
  const topics = value.topics.map((topic: unknown) => {
    if (!isRecord(topic) || typeof topic.key !== "string" || typeof topic.label !== "string" || typeof topic.count !== "number")
      throw new Error("Ugyldig emne");
    return { key: topic.key, label: topic.label, count: topic.count };
  });
  return { enabled: value.enabled, can_manage: value.can_manage, topics: topics.slice(0, 6) };
}

function decodePage(value: unknown, channelId: string): ChannelTopicPage {
  if (!isRecord(value) || !Array.isArray(value.matches) || (value.next_before !== null && typeof value.next_before !== "number"))
    throw new Error("Ugyldige emnetreff");
  const matches = value.matches.map((match: unknown) => {
    if (!isRecord(match) || typeof match.message_id !== "string"
      || (match.parent_message_id !== null && typeof match.parent_message_id !== "string")
      || typeof match.sequence !== "number")
      throw new Error("Ugyldig emnetreff");
    const message = decodeChatMessage(match.message);
    if (message.channel_id !== channelId || message.id !== match.message_id || message.sequence !== match.sequence
      || message.parent_message_id !== match.parent_message_id || message.deleted_at)
      throw new Error("Svaret gjeld ei anna melding.");
    return { message_id: match.message_id, parent_message_id: match.parent_message_id, sequence: match.sequence, message };
  });
  return { matches, next_before: value.next_before };
}

export class ChannelTopicsApi {
  readonly #changes = new EventTarget();
  constructor(private readonly http: HttpClient, private readonly owner: () => string | null, private readonly epoch: () => number) {}

  #path(channelId: string) { return `/api/v1/channels/${encodeURIComponent(channelId)}/topics`; }
  #admission(owner: string) {
    const epoch = this.epoch();
    return () => this.owner() === owner && this.epoch() === epoch;
  }

  async #request<T>(owner: string, path: string, decode: (value: unknown) => T, options: RequestInit) {
    const admitted = this.#admission(owner);
    const result = await this.http.json(path, decode, {
      ...options, headers: { "x-sproyt-account": owner, ...(options.body ? { "content-type": "application/json" } : {}) }
    }, admitted);
    if (!admitted()) throw new Error("Kontoen vart endra.");
    return result;
  }

  subscribe(channelId: string, listener: EventListener) {
    this.#changes.addEventListener(channelId, listener);
    return () => this.#changes.removeEventListener(channelId, listener);
  }

  summary(owner: string, channelId: string, signal: AbortSignal) {
    return this.#request(owner, this.#path(channelId), decodeSummary, { signal });
  }

  async save(owner: string, channelId: string, enabled: boolean) {
    const summary = await this.#request(owner, this.#path(channelId), decodeSummary, {
      method: "PUT", body: JSON.stringify({ enabled })
    });
    this.#changes.dispatchEvent(new Event(channelId));
    return summary;
  }

  matches(owner: string, channelId: string, topic: string, before: number | null, signal: AbortSignal) {
    const query = new URLSearchParams({ topic });
    if (before !== null) query.set("before", String(before));
    return this.#request(owner, `${this.#path(channelId)}/matches?${query}`, value => decodePage(value, channelId), { signal });
  }

  context(owner: string, channelId: string, match: ChannelTopicMatch, signal: AbortSignal) {
    const query = new URLSearchParams({ sequence: String(match.sequence), message_id: match.message_id });
    return this.#request(owner, `${this.#path(channelId)}/context?${query}`, value => {
      if (!isRecord(value) || !Array.isArray(value.messages)) throw new Error("Ugyldig meldingshistorikk");
      const messages = value.messages.map(decodeChatMessage);
      if (messages.some(message => message.channel_id !== channelId || message.parent_message_id !== null || message.deleted_at)
        || !messages.some(message => message.id === match.message_id && message.sequence === match.sequence))
        throw new Error("Svaret gjeld ei anna melding.");
      return messages;
    }, { signal });
  }
}
