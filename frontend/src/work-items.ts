import { HttpClient } from "./api";
import { isRecord } from "./types";

export type WorkApplication = Readonly<{ id: string; key: string; name: string }>;
export type WorkItemDraft = Readonly<{ source_body: string; title: string; suggested_by_model: boolean }>;
export type WorkItemReceipt = Readonly<{ id: string; title: string; status: string; start_status: string }>;
export type WorkItemTask = Readonly<{
  id: string; message_id: string; work_item_id: string; revision: number;
  application_name: string; title: string; description: string; status: "pending" | "completed" | "cancelled";
  process_status: string; delivery_status: "ready" | "pending" | "failed";
  category: string | null; priority: string | null; decision_status: string | null;
  assignee_name: string; can_decide: boolean;
  blocked: boolean;
  node_id: "review" | "provide-information" | "followup-review";
  can_request_information: boolean;
  information_request: string | null; information_response: string | null;
}>;

export function workItemTaskId(body: string): string | null {
  return /^\[\[work-item-task:([0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12})\]\]$/iu.exec(body)?.[1] ?? null;
}

const uuid = (value: unknown): value is string => typeof value === "string" && /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/iu.test(value);

export function decodeWorkItemTask(value: unknown): WorkItemTask {
  if (!isRecord(value) || !uuid(value.id) || !uuid(value.message_id) || !uuid(value.work_item_id)
    || typeof value.title !== "string" || typeof value.description !== "string" || typeof value.application_name !== "string"
    || typeof value.revision !== "number" || !Number.isSafeInteger(value.revision) || value.revision < 1
    || !["pending", "completed", "cancelled"].includes(String(value.status))
    || !["starting", "waiting", "completed", "cancelled", "failed"].includes(String(value.process_status))
    || !["ready", "pending", "failed"].includes(String(value.delivery_status)) || typeof value.can_decide !== "boolean"
    || typeof value.assignee_name !== "string" || typeof value.blocked !== "boolean"
    || !["review", "provide-information", "followup-review"].includes(String(value.node_id))
    || typeof value.can_request_information !== "boolean"
    || (value.can_request_information && value.node_id !== "review")
    || !(value.information_request === null || typeof value.information_request === "string")
    || !(value.information_response === null || typeof value.information_response === "string")
    || !["bug", "change", "question", null].includes(value.category as string | null)
    || !["untriaged", "low", "normal", "high", "critical", null].includes(value.priority as string | null)
    || !["reviewing", "needs_information", "planned", "resolved", "rejected", null].includes(value.decision_status as string | null)) {
    throw new Error("Kunne ikkje lese behandlaroppgåva.");
  }
  return value as WorkItemTask;
}

export class WorkItemApi {
  private readonly applicationsCache = new Map<string, Promise<readonly WorkApplication[]>>();
  private readonly admissions = new Map<string, { payload: string; id: string; revision?: number }>();
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

  async task(id: string, messageId: string, signal?: AbortSignal): Promise<WorkItemTask> {
    const task = await this.http.json(`/api/v1/work-item-tasks/${encodeURIComponent(id)}?message_id=${encodeURIComponent(messageId)}`, decodeWorkItemTask, { signal });
    if (task.id !== id || task.message_id !== messageId) throw new Error("Svaret gjeld ei anna oppgåve.");
    return task;
  }

  async decide(task: WorkItemTask, category: string, priority: string, status: string, note = ""): Promise<WorkItemTask> {
    const payload = JSON.stringify({ task: task.id, message: task.message_id, category, priority, status, note });
    const key = `sproyt-work-item-decision:${this.identity()}:${task.id}`;
    let admission = this.admissions.get(key);
    if (!admission) {
      try {
        const saved = JSON.parse(sessionStorage.getItem(key) || "null") as unknown;
        if (isRecord(saved) && saved.payload === payload && uuid(saved.id) && Number.isSafeInteger(saved.revision) && Number(saved.revision) > 0) admission = { payload, id: saved.id, revision: Number(saved.revision) };
      } catch { /* optional storage */ }
    }
    if (!admission || admission.payload !== payload) {
      admission = { payload, id: crypto.randomUUID(), revision: task.revision };
      this.admissions.set(key, admission);
      try { sessionStorage.setItem(key, JSON.stringify(admission)); } catch { /* optional storage */ }
    }
    const result = await this.http.json(`/api/v1/work-item-tasks/${encodeURIComponent(task.id)}/decide`, decodeWorkItemTask,
      { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({
      message_id: task.message_id, request_id: admission.id, expected_revision: admission.revision ?? task.revision, category, priority, status, note
    }) });
    if (result.id !== task.id || result.message_id !== task.message_id) throw new Error("Avgjerda gjeld ei anna oppgåve.");
    this.admissions.delete(key);
    try { sessionStorage.removeItem(key); } catch { /* optional storage */ }
    return result;
  }
}
