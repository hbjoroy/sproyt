import type { HttpClient } from "./api";
import { decodeChatMessage, isRecord, type ChatMessage } from "./types";

export interface PersonCardData {
  id: string;
  display_name: string;
  handle: string | null;
  created_at: string;
  signup_ordinal: number | null;
  status_text: string;
  status_emoji: string;
  messages: ChatMessage[];
}

export type LoadPersonCard = (owner: string, person: string, signal: AbortSignal) => Promise<PersonCardData>;

export function personCardLoader(http: HttpClient, owner: () => string | null, epoch: () => number): LoadPersonCard {
  return (viewer, person, signal) => {
    const generation = epoch();
    return http.json(`/api/v1/people/${encodeURIComponent(person)}/card`, value => {
      if (!isRecord(value) || value.id !== person || typeof value.display_name !== "string"
        || !(value.handle === null || typeof value.handle === "string") || typeof value.created_at !== "string"
        || typeof value.status_text !== "string" || typeof value.status_emoji !== "string"
        || !(value.signup_ordinal === null || (typeof value.signup_ordinal === "number" && Number.isSafeInteger(value.signup_ordinal) && value.signup_ordinal > 0))
        || !Array.isArray(value.messages) || value.messages.length > 5) throw new Error("Ugyldig personkort.");
      const messages = value.messages.map(decodeChatMessage);
      if (messages.some(m => m.sender_id !== person || m.deleted_at)) throw new Error("Ugyldig aktivitet på personkortet.");
      return { id: person, display_name: value.display_name, handle: value.handle, created_at: value.created_at,
        status_text: value.status_text, status_emoji: value.status_emoji, signup_ordinal: value.signup_ordinal, messages };
    }, { signal, cache: "no-store", headers: { "x-sproyt-account": viewer } }, () => viewer === owner() && epoch() === generation);
  };
}
