import { HttpClient } from "./api";

export interface SavedStatus {
  readonly text: string;
  readonly emoji: string;
  readonly save_count: number;
  readonly last_used_at: string;
}

export class SavedStatusApi {
  constructor(private readonly http: HttpClient) {}
  list(): Promise<SavedStatus[]> {
    return this.http.json("/api/v1/me/statuses", value => {
      if (!Array.isArray(value) || value.length > 20 || !value.every(item =>
        item && typeof item === "object" && typeof item.text === "string" && [...item.text].length <= 100
        && typeof item.emoji === "string" && [...item.emoji].length <= 16 && Boolean(item.text || item.emoji)
        && Number.isSafeInteger(item.save_count) && item.save_count > 0
        && typeof item.last_used_at === "string" && Number.isFinite(Date.parse(item.last_used_at)))) {
        throw new Error("Ugyldig liste over lagra statusar.");
      }
      return value as SavedStatus[];
    });
  }
  remove(status: Pick<SavedStatus, "text" | "emoji">): Promise<void> {
    return this.http.empty("/api/v1/me/statuses", { method: "DELETE", headers: { "content-type": "application/json" },
      body: JSON.stringify({ text: status.text, emoji: status.emoji }) });
  }
}
