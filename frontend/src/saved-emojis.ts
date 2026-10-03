import { HttpClient } from "./api";

/** Per-item mutations preserve additions made by another device. */
export class SavedEmojiApi {
  constructor(private readonly http: HttpClient) {}
  list(): Promise<string[]> {
    return this.http.json("/api/v1/me/emojis", value => {
      if (!Array.isArray(value) || value.length > 50 || !value.every(item => typeof item === "string"))
        throw new Error("Ugyldig emoji-liste frå tenaren.");
      return value;
    });
  }
  save(emoji: string, saved = true): Promise<void> {
    return this.http.empty("/api/v1/me/emojis", { method: saved ? "POST" : "DELETE",
      headers: { "content-type": "application/json" }, body: JSON.stringify({ emoji }) });
  }
}

/** Only offer automatic saving for a whole pasted emoji, never ordinary text. The server validates. */
export function pastedEmoji(value: string): string | null {
  const emoji = value.trim();
  return emoji && new TextEncoder().encode(emoji).length <= 32
    && [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(emoji)].length === 1
    && /[\p{Extended_Pictographic}\p{Regional_Indicator}\u20e3]/u.test(emoji) ? emoji : null;
}
