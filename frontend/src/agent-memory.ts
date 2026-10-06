import { HttpClient } from "./api";
import { isRecord } from "./types";

export type MemoryAgent = { agentId: string; displayName: string };
export type MemoryNote = {
  id: string; channelId: string; kind: "preference" | "temporary_context" | "interaction";
  text: string; participantIds: string[]; origin: "automatic" | "user";
  evidence: "user_stated" | "conversation_event" | "user_confirmed";
  revision: number; createdAt: number; updatedAt: number; expiresAt: number | null; sourceMessageIds: string[];
};
export type AgentMemory = {
  circleId: string; agentId: string; enabled: boolean; agentEnabled: boolean;
  collectionAvailable: boolean; collectionStartedAt: number | null;
  revision: number; memoryEpoch: number; historyCompactions: number; notes: MemoryNote[]; unavailableNotes: number;
};
export type MemoryAction = { action: "correct"; note_id: string; text: string }
  | { action: "confirm" | "forget"; note_id: string } | { action: "reset" };
const invalid = () => new Error("Ugyldig svar frå minnetenesta.");
const uuid = (v: unknown): v is string => typeof v === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu.test(v);
const integer = (v: unknown, minimum = 0): v is number => typeof v === "number" && Number.isSafeInteger(v) && v >= minimum;
const timestamp = (v: unknown): v is number | null => v === null || (integer(v) && v <= 8640000000000);
const ids = (v: unknown, limit: number): v is string[] => Array.isArray(v) && v.length <= limit && v.every(uuid) && new Set(v).size === v.length;
export const memoryTextBytes = (text: string) => new TextEncoder().encode(text.trim()).length;
export const validMemoryText = (text: string) => !!text.trim() && memoryTextBytes(text) <= 1024 && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(text.trim());

export function decodeMemoryAgents(value: unknown): MemoryAgent[] {
  if (!isRecord(value) || !Array.isArray(value.agents) || value.agents.length > 10) throw invalid();
  const agents = value.agents.map(item => {
    if (!isRecord(item) || !uuid(item.agent_id) || typeof item.display_name !== "string" || !item.display_name.trim()) throw invalid();
    return { agentId: item.agent_id, displayName: item.display_name };
  });
  if (new Set(agents.map(agent => agent.agentId)).size !== agents.length) throw invalid();
  return agents;
}
export function decodeAgentMemory(value: unknown): AgentMemory {
  if (!isRecord(value) || !uuid(value.circle_id) || !uuid(value.agent_id)
    || typeof value.enabled !== "boolean" || typeof value.agent_enabled !== "boolean" || typeof value.collection_available !== "boolean"
    || !timestamp(value.collection_started_at) || !integer(value.revision) || !integer(value.memory_epoch, 1)
    || !integer(value.history_compactions) || !integer(value.unavailable_notes) || !Array.isArray(value.notes) || value.notes.length > 24) throw invalid();
  const notes: MemoryNote[] = value.notes.map(note => {
    if (!isRecord(note) || !uuid(note.id) || !uuid(note.channel_id) || !isRecord(note.content)
      || typeof note.content.text !== "string" || !validMemoryText(note.content.text) || !ids(note.content.participant_ids, 30)
      || !["preference", "temporary_context", "interaction"].includes(String(note.kind))
      || !["automatic", "user"].includes(String(note.origin)) || !["user_stated", "conversation_event", "user_confirmed"].includes(String(note.evidence))
      || !integer(note.revision, 1) || !integer(note.created_at) || !timestamp(note.created_at) || !integer(note.updated_at) || !timestamp(note.updated_at) || !timestamp(note.expires_at)
      || !ids(note.source_message_ids, 30)) throw invalid();
    return { id: note.id, channelId: note.channel_id, kind: note.kind as MemoryNote["kind"], text: note.content.text,
      participantIds: note.content.participant_ids, origin: note.origin as MemoryNote["origin"], evidence: note.evidence as MemoryNote["evidence"],
      revision: note.revision, createdAt: note.created_at, updatedAt: note.updated_at, expiresAt: note.expires_at, sourceMessageIds: note.source_message_ids };
  });
  if (new Set(notes.map(note => note.id)).size !== notes.length || notes.reduce((sum, note) => sum + memoryTextBytes(note.text), 0) > 16384) throw invalid();
  return { circleId: value.circle_id, agentId: value.agent_id, enabled: value.enabled, agentEnabled: value.agent_enabled,
    collectionAvailable: value.collection_available, collectionStartedAt: value.collection_started_at,
    revision: value.revision, memoryEpoch: value.memory_epoch, historyCompactions: value.history_compactions, notes, unavailableNotes: value.unavailable_notes };
}
export class AgentMemoryApi {
  constructor(private readonly http: HttpClient) {}
  private path(circleId: string, agentId?: string) {
    return `/api/v1/me/circles/${encodeURIComponent(circleId)}/chat-agents${agentId ? `/${encodeURIComponent(agentId)}/memory` : ""}`;
  }
  list(circleId: string, signal?: AbortSignal) { return this.http.json(this.path(circleId), decodeMemoryAgents, { signal }); }
  private async view(circleId: string, agentId: string, options: RequestInit = {}) {
    const view = await this.http.json(this.path(circleId, agentId) + (options.method === "POST" ? "/actions" : ""), decodeAgentMemory, options);
    if (view.circleId !== circleId || view.agentId !== agentId) throw invalid();
    return view;
  }
  get(circleId: string, agentId: string, signal?: AbortSignal) { return this.view(circleId, agentId, { signal }); }
  choice(circleId: string, agentId: string, revision: number, enabled: boolean) {
    return this.view(circleId, agentId, { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify({ revision, enabled }) });
  }
  action(circleId: string, agentId: string, revision: number, action: MemoryAction) {
    return this.view(circleId, agentId, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ revision, ...action }) });
  }
}
