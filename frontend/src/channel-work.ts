import type { HttpClient } from "./api";
import { decodeCircleProcesses, type CircleProcess, type CircleProcessPage } from "./circle-processes";
import { decodePilotTask, type PilotTask } from "./process-pilot";
import { isRecord } from "./types";
import { decodeWorkItemTask, type WorkItemTask } from "./work-items";

type TaskOrigin = Readonly<{
  channel_id: string;
  channel_name: string;
  circle_id: string | null;
  circle_name: string | null;
}>;

export type AssignedProcessTask = TaskOrigin & (
  Readonly<{ kind: "work_item"; task: WorkItemTask }>
  | Readonly<{ kind: "process_pilot"; task: PilotTask }>
  | Readonly<{ kind: "feedback"; task: Readonly<{ id: string; message_id: string; title: string; source_excerpt?: string }> }>
);

type TaskPage = Readonly<{ tasks: readonly AssignedProcessTask[]; next_cursor: string | null }>;
type DefinitionStep = Readonly<{ id: string; name: string; kind: string; next: readonly string[] }>;
export type ChannelProcess = CircleProcess & Readonly<{
  definition_namespace: string;
  definition_version: string | null;
  definition_steps: readonly DefinitionStep[] | null;
  active_node_ids: readonly string[] | null;
}>;

const hasText = (value: unknown): value is string => typeof value === "string" && value.length > 0;
const nullableText = (value: unknown): value is string | null => value === null || hasText(value);
const stringList = (value: unknown): value is string[] => Array.isArray(value) && value.every(hasText);
const invalidTasks = () => new Error("Kunne ikkje lese oppgåvelista.");
const invalidProcess = () => new Error("Kunne ikkje lese prosessdetaljane.");

export function assignedTaskKey(item: AssignedProcessTask): string {
  return `${item.kind}:${item.task.id}`;
}

export function decodeAssignedTasks(value: unknown): TaskPage {
  if (!isRecord(value) || !Array.isArray(value.tasks) || value.tasks.length > 100 || !nullableText(value.next_cursor)) throw invalidTasks();
  const tasks = value.tasks.map((row): AssignedProcessTask => {
    if (!isRecord(row) || !hasText(row.channel_id) || !hasText(row.channel_name)
      || !nullableText(row.circle_id) || !nullableText(row.circle_name)) throw invalidTasks();
    const origin = { channel_id: row.channel_id, channel_name: row.channel_name,
      circle_id: row.circle_id, circle_name: row.circle_name };
    if (row.kind === "work_item") return { ...origin, kind: row.kind, task: decodeWorkItemTask(row.task) };
    if (row.kind === "process_pilot") return { ...origin, kind: row.kind, task: decodePilotTask(row.task) };
    if (row.kind === "feedback" && isRecord(row.task) && hasText(row.task.id) && hasText(row.task.message_id) && hasText(row.task.title)) {
      const excerpt = row.task.source_excerpt;
      if (excerpt != null && typeof excerpt !== "string") throw invalidTasks();
      return { ...origin, kind: "feedback", task: { id: row.task.id, message_id: row.task.message_id, title: row.task.title, source_excerpt: excerpt ?? undefined } };
    }
    throw invalidTasks();
  });
  return { tasks, next_cursor: value.next_cursor };
}

export function decodeChannelProcess(value: unknown): ChannelProcess {
  if (!isRecord(value) || !hasText(value.definition_namespace) || !nullableText(value.definition_version)
    || !(value.active_node_ids === null || stringList(value.active_node_ids))
    || !(value.definition_steps === null || Array.isArray(value.definition_steps))) throw invalidProcess();
  const { processes: [process] } = decodeCircleProcesses({ processes: [value], next_cursor: null });
  if (!process) throw invalidProcess();
  const definitionSteps = value.definition_steps?.map((node): DefinitionStep => {
    if (!isRecord(node) || !hasText(node.id) || !hasText(node.name) || !hasText(node.kind) || !stringList(node.next)) throw invalidProcess();
    return { id: node.id, name: node.name, kind: node.kind, next: node.next };
  }) ?? null;
  return { ...process, definition_namespace: value.definition_namespace, definition_version: value.definition_version,
    active_node_ids: value.active_node_ids, definition_steps: definitionSteps };
}

export class ChannelWorkApi {
  constructor(private readonly http: HttpClient) {}

  tasks(owner: string, channelId: string | undefined, after: string | null, signal: AbortSignal, admitted: () => boolean): Promise<TaskPage> {
    const query = new URLSearchParams({ limit: "25" });
    if (channelId) query.set("channel_id", channelId);
    if (after) query.set("cursor", after);
    return this.http.json(`/api/v1/me/process-tasks?${query}`, value => {
      const page = decodeAssignedTasks(value);
      if (channelId && page.tasks.some(item => item.channel_id !== channelId)) throw invalidTasks();
      return page;
    }, { signal, headers: { "x-sproyt-account": owner } }, admitted);
  }

  processes(owner: string, channelId: string, state: "open" | "all", after: string | null, signal: AbortSignal, admitted: () => boolean): Promise<CircleProcessPage> {
    const query = new URLSearchParams({ limit: "20", state });
    if (after) query.set("after", after);
    return this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/processes?${query}`, value => {
      const page = decodeCircleProcesses(value);
      if (page.processes.some(process => process.channel_id !== channelId)) throw invalidProcess();
      return page;
    }, { signal, headers: { "x-sproyt-account": owner } }, admitted);
  }

  process(owner: string, channelId: string, processId: string, signal: AbortSignal, admitted: () => boolean): Promise<ChannelProcess> {
    return this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/processes/${encodeURIComponent(processId)}`, value => {
      const process = decodeChannelProcess(value);
      if (process.channel_id !== channelId || process.id !== processId) throw invalidProcess();
      return process;
    }, { signal, headers: { "x-sproyt-account": owner } }, admitted);
  }
}
