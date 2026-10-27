import { HttpClient } from "./api";
import { isRecord } from "./types";

export type ThreadRootPreview = Readonly<{
  id: string; author: string | null; preview: string; sentAt: string | null; deleted: boolean;
}>;

/** The endpoint is read-only and returns only roots in the authorised channel. */
export class ThreadPreviewApi {
  constructor(private readonly http: HttpClient) {}
  list(owner: string, channelId: string, rootIds: readonly string[], admitted: () => boolean, signal?: AbortSignal): Promise<readonly ThreadRootPreview[]> {
    if (!owner || !channelId || rootIds.length === 0 || rootIds.length > 30 || new Set(rootIds).size !== rootIds.length) throw new Error("Ugyldig trådutval.");
    const requested = new Set(rootIds);
    return this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/thread-root-previews`, value => {
      if (!isRecord(value) || !Array.isArray(value.roots) || value.roots.length > rootIds.length) throw new Error("Ugyldig trådoversikt.");
      const seen = new Set<string>();
      return value.roots.map(row => {
        if (!isRecord(row) || typeof row.id !== "string" || !requested.has(row.id) || seen.has(row.id)
          || !(row.author === null || typeof row.author === "string") || typeof row.preview !== "string"
          || !(row.sent_at === null || typeof row.sent_at === "string") || typeof row.deleted !== "boolean")
          throw new Error("Ugyldig trådoversikt.");
        seen.add(row.id);
        return { id: row.id, author: row.author, preview: row.preview, sentAt: row.sent_at, deleted: row.deleted };
      });
    }, { method: "POST", headers: { "content-type": "application/json", "x-sproyt-account": owner },
      body: JSON.stringify({ root_ids: rootIds }), signal }, admitted);
  }
}
