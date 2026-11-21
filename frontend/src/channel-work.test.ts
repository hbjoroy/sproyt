import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "./api";
import { ChannelWorkApi, decodeAssignedTasks, decodeChannelProcess } from "./channel-work";
import type { PilotTask } from "./process-pilot";
import type { WorkItemTask } from "./work-items";

const id = "c63ac052-a05a-4b5d-bfff-04429338df90";
const messageId = "15292fae-20de-4e9f-96b1-c3af5ae8596f";
const pilot: PilotTask = {
  id, message_id: messageId, instance_id: "instance-1", node_id: "first", status: "pending",
  assignee_id: "user-1", title: "Første oppgåve", can_complete: true, delivery_status: "ready", process_status: "waiting",
};
const workItem: WorkItemTask = {
  id, message_id: messageId, work_item_id: id, revision: 1, application_name: "Sprøyt", title: "Feilrapport",
  description: "Namn manglar", status: "pending", process_status: "waiting", delivery_status: "ready",
  category: null, priority: null, decision_status: null, assignee_name: "Test", can_decide: true, blocked: false,
  node_id: "review", can_request_information: true, information_request: null, information_response: null,
};
const origin = { channel_id: "channel-1", channel_name: "Planlegging", circle_id: "circle-1", circle_name: "Testkretsen" };
const process = {
  id: `pilot:${id}`, kind: "pilot", title: "Eksempel", channel_id: "channel-1", channel_name: "Planlegging",
  status: "waiting", definition_name: "Døme", runtime: "v2", projection: false,
  steps: [{ name: "Første oppgåve", status: "completed", node_id: "first" }],
  history: [], steps_truncated: false, history_truncated: false, remaining_steps: null,
  definition_namespace: "sproyt", definition_version: "2", active_node_ids: ["second"],
  definition_steps: [
    { id: "first", name: "Første oppgåve", kind: "user-task", next: ["second", "alternative"] },
    { id: "second", name: "Andre oppgåve", kind: "user-task", next: [] },
    { id: "alternative", name: "Alternativt steg", kind: "task", next: [] },
  ],
};

test("the queue preserves task kind and origin and validates the existing task contracts", () => {
  const tasks = [{ ...origin, kind: "process_pilot", task: pilot },
    { ...origin, channel_id: "channel-2", kind: "work_item", task: workItem }];
  const page = decodeAssignedTasks({ tasks, next_cursor: "opaque" });
  assert.equal(page.next_cursor, "opaque");
  assert.deepEqual(page.tasks.map(row => [row.kind, row.channel_id, row.task.message_id]), [
    ["process_pilot", "channel-1", messageId], ["work_item", "channel-2", messageId],
  ]);
  assert.throws(() => decodeAssignedTasks({ tasks: [{ ...tasks[0], kind: "unknown" }], next_cursor: null }));
  assert.throws(() => decodeAssignedTasks({ tasks: [{ ...tasks[1], task: { ...workItem, revision: 0 } }], next_cursor: null }));
});

test("queue requests bind the account and channel and allow pagination on personal scope", async () => {
  const requests: { url: URL; account: string | null; signal: AbortSignal | null | undefined }[] = [];
  const controller = new AbortController();
  const api = new ChannelWorkApi(new HttpClient({ participant: () => "session-1", fetch: async (url, init) => {
    requests.push({ url: new URL(String(url), "https://example.test"), account: new Headers(init?.headers).get("x-sproyt-account"), signal: init?.signal });
    return Response.json({ tasks: [{ ...origin, kind: "process_pilot", task: pilot }], next_cursor: null });
  } }));
  await api.tasks("owner", undefined, "cursor:2", controller.signal, () => true);
  assert.equal(requests[0]?.url.pathname, "/api/v1/me/process-tasks");
  assert.equal(requests[0]?.url.searchParams.get("participant"), "session-1");
  assert.equal(requests[0]?.url.searchParams.get("cursor"), "cursor:2");
  assert.equal(requests[0]?.url.searchParams.get("channel_id"), null);
  assert.equal(requests[0]?.account, "owner");
  assert.equal(requests[0]?.signal, controller.signal);
  await assert.rejects(() => api.tasks("owner", "other-channel", null, controller.signal, () => true));
  assert.equal(requests[1]?.url.searchParams.get("channel_id"), "other-channel");
  await assert.rejects(() => api.tasks("owner", undefined, null, controller.signal, () => false));
  assert.equal(requests.length, 2);
});

test("process decoding keeps possible branches distinct from registered and active steps", () => {
  const decoded = decodeChannelProcess(process);
  assert.deepEqual(decoded.definition_steps?.[0]?.next, ["second", "alternative"]);
  assert.deepEqual(decoded.active_node_ids, ["second"]);
  assert.equal(decoded.steps[0]?.node_id, "first");
  assert.equal(decoded.steps[0]?.status, "completed");
  assert.equal(decodeChannelProcess({ ...process, definition_steps: null, active_node_ids: null }).definition_steps, null);
  assert.throws(() => decodeChannelProcess({ ...process, definition_steps: [{ ...process.definition_steps[0], next: "second" }] }));
});

test("channel process detail binds the requested row and escapes the scoped identifiers", async () => {
  let requested = "";
  let response = process;
  const api = new ChannelWorkApi(new HttpClient({ fetch: async url => {
    requested = String(url);
    return Response.json(response);
  } }));
  const { signal } = new AbortController();
  await api.process("owner", origin.channel_id, process.id, signal, () => true);
  assert.equal(requested, `/api/v1/channels/channel-1/processes/pilot%3A${id}`);
  response = { ...process, channel_id: "other-channel" };
  await assert.rejects(() => api.process("owner", origin.channel_id, process.id, signal, () => true));
});
