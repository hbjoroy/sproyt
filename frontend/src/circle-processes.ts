import type { HttpClient } from "./api";
import { isRecord } from "./types";

export type ProcessStep = Readonly<{ name: string; status: string }>;
export type ProcessHistory = ProcessStep & Readonly<{ occurred_at: string | null }>;
export type CircleProcess = Readonly<{
  id: string; kind: string; title: string; channel_id: string; channel_name: string;
  status: string; definition_name: string; runtime: string; projection: boolean;
  steps: readonly ProcessStep[]; history: readonly ProcessHistory[];
  steps_truncated: boolean; history_truncated: boolean; remaining_steps: readonly ProcessStep[] | null;
}>;
export type CircleProcessPage = Readonly<{ processes: readonly CircleProcess[]; next_cursor: string | null }>;

const invalid = () => new Error("Kunne ikkje lese prosessoversikta.");
function text(value: unknown): string {
  if (typeof value !== "string" || !value || value.length > 4096) throw invalid();
  return value;
}
function steps(value: unknown): ProcessStep[] {
  if (!Array.isArray(value) || value.length > 100) throw invalid();
  return value.map(step => {
    if (!isRecord(step)) throw invalid();
    return { name: text(step.name), status: text(step.status) };
  });
}
export function decodeCircleProcesses(value: unknown): CircleProcessPage {
  if (!isRecord(value) || !Array.isArray(value.processes) || value.processes.length > 50
    || (value.next_cursor !== null && typeof value.next_cursor !== "string")) throw invalid();
  return { next_cursor: value.next_cursor === null ? null : text(value.next_cursor), processes: value.processes.map(row => {
    if (!isRecord(row) || typeof row.projection !== "boolean" || typeof row.steps_truncated !== "boolean"
      || typeof row.history_truncated !== "boolean" || !Array.isArray(row.history) || row.history.length > 100) throw invalid();
    return { id: text(row.id), kind: text(row.kind), title: text(row.title), channel_id: text(row.channel_id),
      channel_name: text(row.channel_name), status: text(row.status), definition_name: text(row.definition_name),
      runtime: text(row.runtime), projection: row.projection, steps: steps(row.steps),
      remaining_steps: row.remaining_steps === null ? null : steps(row.remaining_steps),
      steps_truncated: row.steps_truncated, history_truncated: row.history_truncated,
      history: row.history.map(event => {
        if (!isRecord(event) || (event.occurred_at !== null && typeof event.occurred_at !== "string")) throw invalid();
        return { name: text(event.name), status: text(event.status), occurred_at: event.occurred_at as string | null };
      }) };
  }) };
}

export class CircleProcessApi {
  constructor(private readonly http: HttpClient) {}
  list(circleId: string, owner: string, state: "open" | "all", after: string | null, signal: AbortSignal, admitted: () => boolean) {
    const query = new URLSearchParams({ limit: "20", state });
    if (after) query.set("after", after);
    return this.http.json(`/api/v1/circles/${encodeURIComponent(circleId)}/processes?${query}`, decodeCircleProcesses,
      { signal, headers: { "x-sproyt-account": owner } }, admitted);
  }
}
