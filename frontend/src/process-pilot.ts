import { HttpClient } from "./api";
import { isRecord } from "./types";

export type PilotConfiguration = Readonly<{
  configured: boolean; can_configure: boolean; can_start: boolean; assignee_name?: string;
  /** v1 is retained when reading configuration returned by older servers. */
  runtime_model: "v1" | "v2";
}>;
export type PilotTask = Readonly<{
  id: string; message_id: string; instance_id: string; node_id: string; status: "pending" | "completed" | "cancelled";
  assignee_id: string; assignee_name?: string; title: string; can_complete: boolean;
  delivery_status: "ready" | "pending" | "failed";
  /** Old pilot responses predate this run-level state and were always waiting for the next step. */
  process_status: "starting" | "waiting" | "completed" | "cancelled" | "failed";
}>;

// Only a complete server message is a task reference. Quoted, embedded and
// malformed markers remain ordinary text and never create task controls.
export function processTaskId(body: string): string | null {
  return /^\[\[process-task:([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\]\]$/u.exec(body)?.[1] ?? null;
}

export function decodePilotConfiguration(value: unknown): PilotConfiguration {
  if (!isRecord(value) || typeof value.configured !== "boolean" || typeof value.can_configure !== "boolean"
    || typeof value.can_start !== "boolean" || (value.assignee_name != null && typeof value.assignee_name !== "string")) {
    throw new Error("Kunne ikkje lese prosessoppsettet.");
  }
  if (value.runtime_model != null && value.runtime_model !== "v1" && value.runtime_model !== "v2") throw new Error("Kunne ikkje lese prosessoppsettet.");
  return { configured: value.configured, can_configure: value.can_configure, can_start: value.can_start,
    assignee_name: typeof value.assignee_name === "string" ? value.assignee_name : undefined,
    runtime_model: value.runtime_model === "v2" ? "v2" : "v1" };
}

export function decodePilotTask(value: unknown): PilotTask {
  if (!isRecord(value) || !["id", "message_id", "instance_id", "node_id", "assignee_id", "title"].every(key => typeof value[key] === "string" && Boolean(value[key]))
    || !["pending", "completed", "cancelled"].includes(String(value.status)) || typeof value.can_complete !== "boolean"
    || !["ready", "pending", "failed"].includes(String(value.delivery_status))
    || (value.process_status !== undefined && !["starting", "waiting", "completed", "cancelled", "failed"].includes(String(value.process_status)))
    || (value.assignee_name != null && typeof value.assignee_name !== "string")) throw new Error("Kunne ikkje lese oppgåva.");
  return { id: value.id as string, message_id: value.message_id as string, instance_id: value.instance_id as string, node_id: value.node_id as string,
    assignee_id: value.assignee_id as string, title: value.title as string, status: value.status as PilotTask["status"],
    can_complete: value.can_complete, delivery_status: value.delivery_status as PilotTask["delivery_status"],
    assignee_name: typeof value.assignee_name === "string" ? value.assignee_name : undefined,
    process_status: value.process_status === undefined ? "waiting" : value.process_status as PilotTask["process_status"] };
}

const post = (body: unknown): RequestInit => ({ method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });

export class ProcessPilotApi {
  // Retain the admission key across a lost response, component unmount or retry.
  private readonly admissions = new Map<string, string>();
  constructor(private readonly http: HttpClient, private readonly identity: () => string) {}
  canReload(): boolean {
    try { return [...this.admissions].every(([key, value]) => sessionStorage.getItem(key) === value); }
    catch { return this.admissions.size === 0; }
  }
  private admissionKey(operation: string): string { return `sproyt-process-pilot:${this.identity()}:${operation}`; }
  private requestId(operation: string): string {
    const key = this.admissionKey(operation);
    const existing = this.admissions.get(key);
    if (existing) return existing;
    try {
      const persisted = sessionStorage.getItem(key);
      if (persisted && processTaskId(`[[process-task:${persisted}]]`)) {
        this.admissions.set(key, persisted);
        return persisted;
      }
    } catch { /* Storage is optional; in-memory retries still share an id. */ }
    const id = crypto.randomUUID();
    this.admissions.set(key, id);
    try { sessionStorage.setItem(key, id); } catch { /* optional */ }
    return id;
  }
  configuration(channelId: string, signal?: AbortSignal): Promise<PilotConfiguration> {
    return this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/process-pilot`, decodePilotConfiguration, { signal });
  }
  async configure(channelId: string): Promise<void> {
    await this.http.empty(`/api/v1/channels/${encodeURIComponent(channelId)}/process-pilot`, post({ enabled: true }));
  }
  async start(channelId: string): Promise<void> {
    const key = this.admissionKey(`start:${channelId}`);
    await this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/process-pilot/start`, value => {
      if (!isRecord(value) || typeof value.id !== "string" || typeof value.status !== "string") throw new Error("Kunne ikkje stadfeste at prosessen starta. Prøv igjen.");
      return value;
    }, post({ request_id: this.requestId(`start:${channelId}`) }));
    this.admissions.delete(key);
    try { sessionStorage.removeItem(key); } catch { /* optional */ }
  }
  async task(id: string, messageId: string, signal?: AbortSignal): Promise<PilotTask> {
    const task = await this.http.json(`/api/v1/process-pilot/tasks/${encodeURIComponent(id)}?message_id=${encodeURIComponent(messageId)}`, decodePilotTask, { signal });
    if (task.id !== id || task.message_id !== messageId) throw new Error("Svaret gjeld ei anna oppgåve.");
    return task;
  }
  async complete(id: string, messageId: string): Promise<PilotTask> {
    const task = await this.http.json(`/api/v1/process-pilot/tasks/${encodeURIComponent(id)}/complete`, decodePilotTask,
      post({ request_id: this.requestId(`complete:${id}`), message_id: messageId }));
    if (task.id !== id || task.message_id !== messageId) throw new Error("Svaret gjeld ei anna oppgåve.");
    if (task.status !== "pending" || ["completed", "cancelled", "failed"].includes(task.process_status)) {
      const key = this.admissionKey(`complete:${id}`);
      this.admissions.delete(key);
      try { sessionStorage.removeItem(key); } catch { /* the terminal receipt is confirmed */ }
    }
    return task;
  }
}
