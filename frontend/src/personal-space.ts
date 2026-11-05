import type { HttpClient } from "./api";
import { decodeChatMessage, isChannelBase, isRecord, type ChannelBase, type ChatMessage } from "./types";
import { readViewTarget } from "./view-history";

const path = "/api/v1/me/personal-space";

export type PersonalNote = Readonly<{ markdown: string; revision: number }>;

export type PersonalMessageReference = Readonly<{
  channelId: string;
  messageId: string;
  sequence: number;
  threadId?: string;
}>;

const uuid = /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/iu;

export function personalMessageReference(body: string, origin: string): PersonalMessageReference | null {
  // Old saved links have the same target and can be rendered without rewriting history.
  const text = body.trim();
  const macro = /^\[\[(.+)\]\]$/u.exec(text)?.[1];
  const link = /^\[Lagra melding\]\((\/\?[^\s()]+)\)$/u.exec(macro ?? text)?.[1] ?? macro;
  if (!link || !/^(?:\/\?|https?:\/\/)/u.test(link)) return null;
  let url: URL;
  try { url = new URL(link, origin); } catch { return null; }
  if (url.origin !== origin || url.pathname !== "/" || url.hash || url.username || url.password
    || [...url.searchParams.keys()].some(key => !["channel", "message", "sequence", "thread"].includes(key)
      || url.searchParams.getAll(key).length !== 1)) return null;
  const target = readViewTarget(url.href);
  if (target?.kind !== "conversation" || !uuid.test(target.channelId) || !target.messageId || !uuid.test(target.messageId)
    || !target.sequence || (target.threadId !== undefined && !uuid.test(target.threadId))) return null;
  return { channelId: target.channelId.toLowerCase(), messageId: target.messageId.toLowerCase(),
    sequence: target.sequence, threadId: target.threadId?.toLowerCase() };
}

function decodeNote(value: unknown): PersonalNote {
  if (!isRecord(value) || typeof value.markdown !== "string" || typeof value.revision !== "number"
    || !Number.isSafeInteger(value.revision) || value.revision < 0) {
    throw new Error("Ugyldig svar for notatet.");
  }
  return { markdown: value.markdown, revision: value.revision };
}

export class PersonalSpaceApi {
  constructor(private readonly http: HttpClient, private readonly currentOwner: () => string | null,
    private readonly accountEpoch: () => number) {}

  open(owner: string): Promise<ChannelBase> {
    return this.request(owner, path, value => {
      if (!isChannelBase(value) || value.created_by !== owner || value.kind !== "private" || value.circle_id !== null) {
        throw new Error("Ugyldig svar for området.");
      }
      return value;
    }, { method: "POST" });
  }

  note(owner: string): Promise<PersonalNote> {
    return this.request(owner, `${path}/note`, decodeNote);
  }

  saveNote(owner: string, note: PersonalNote): Promise<PersonalNote> {
    return this.request(owner, `${path}/note`, decodeNote, { method: "PUT", body: JSON.stringify(note) });
  }

  saveLink(owner: string, messageId: string): Promise<boolean> {
    return this.request(owner, `${path}/saved-links/${encodeURIComponent(messageId)}`, value => {
      if (!isRecord(value) || typeof value.created !== "boolean") throw new Error("Ugyldig svar ved lagring.");
      return value.created;
    }, { method: "POST" });
  }

  sourceMessage(owner: string, reference: PersonalMessageReference, signal: AbortSignal): Promise<ChatMessage> {
    const { channelId, messageId, sequence } = reference;
    return this.request(owner, `${path}/sources/${encodeURIComponent(channelId)}/${encodeURIComponent(messageId)}/${sequence}`, value => {
      const message = decodeChatMessage(value);
      if (message.id !== messageId || message.channel_id !== channelId
        || message.sequence !== sequence || message.deleted_at) throw new Error("Svaret gjeld ei anna melding.");
      return message;
    }, { signal });
  }

  private async request<T>(owner: string, url: string, decode: (value: unknown) => T,
    options: RequestInit = {}): Promise<T> {
    const epoch = this.accountEpoch();
    const admitted = () => this.currentOwner() === owner && this.accountEpoch() === epoch;
    const result = await this.http.json(url, decode, {
      ...options,
      headers: { "x-sproyt-account": owner, ...(options.body ? { "content-type": "application/json" } : {}) }
    }, admitted);
    if (!admitted()) throw new Error("Kontoen vart endra.");
    return result;
  }
}
