import { HttpClient } from "./api";
import { isRecord } from "./types";

export type WorkApplication = Readonly<{ id: string; key: string; name: string }>;
export type WorkItemDraft = Readonly<{ source_body: string; title: string; suggested_by_model: boolean }>;
export type WorkItemReceipt = Readonly<{ id: string; title: string; status: string; start_status: string }>;

const uuid = (value: unknown): value is string => typeof value === "string" && /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/iu.test(value);

export class WorkItemApi {
  private readonly applicationsCache = new Map<string, Promise<readonly WorkApplication[]>>();
  private readonly admissions = new Map<string, { payload: string; id: string }>();
  constructor(private readonly http: HttpClient, private readonly identity: () => string) {}

  applications(channelId: string): Promise<readonly WorkApplication[]> {
    const cached = this.applicationsCache.get(channelId);
    if (cached) return cached;
    const request = this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/work-items/applications`, value => {
      if (!Array.isArray(value) || !value.every(item => isRecord(item) && uuid(item.id) && typeof item.key === "string" && typeof item.name === "string")) throw new Error("Kunne ikkje lese applikasjonane.");
      return value as WorkApplication[];
    }).catch(error => { this.applicationsCache.delete(channelId); throw error; });
    this.applicationsCache.set(channelId, request);
    return request;
  }

  draft(channelId: string, messageId: string): Promise<WorkItemDraft> {
    return this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/work-items/draft/${encodeURIComponent(messageId)}`, value => {
      if (!isRecord(value) || typeof value.source_body !== "string" || typeof value.title !== "string" || typeof value.suggested_by_model !== "boolean") throw new Error("Kunne ikkje lage saksutkast.");
      return value as WorkItemDraft;
    });
  }

  async register(channelId: string, messageId: string, applicationId: string, title: string, description: string, sourceBody: string): Promise<WorkItemReceipt> {
    const payload = JSON.stringify({ channelId, messageId, applicationId, title, description, sourceBody });
    const key = `sproyt-work-item:${this.identity()}:${messageId}`;
    let admission = this.admissions.get(key);
    if (!admission) {
      try {
        const saved = JSON.parse(sessionStorage.getItem(key) || "null") as unknown;
        if (isRecord(saved) && saved.payload === payload && uuid(saved.id)) admission = { payload, id: saved.id };
      } catch { /* optional storage */ }
    }
    if (!admission || admission.payload !== payload) {
      admission = { payload, id: crypto.randomUUID() };
      this.admissions.set(key, admission);
      try { sessionStorage.setItem(key, JSON.stringify(admission)); } catch { /* optional storage */ }
    }
    const receipt = await this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/work-items`, value => {
      if (!isRecord(value) || !uuid(value.id) || typeof value.title !== "string" || typeof value.status !== "string" || typeof value.start_status !== "string") throw new Error("Kunne ikkje lese sakskvitteringa.");
      return value as WorkItemReceipt;
    }, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({
      source_message_id: messageId, application_id: applicationId, title, description,
      expected_source_body: sourceBody, request_id: admission.id
    }) });
    this.admissions.delete(key);
    try { sessionStorage.removeItem(key); } catch { /* optional storage */ }
    return receipt;
  }
}
