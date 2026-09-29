import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { HttpClient } from "./api";
import { decodePilotTask, ProcessPilotApi, processTaskId, type PilotTask } from "./process-pilot";
import { ProcessTaskDetails, ProcessTaskMessage } from "./ui/react/process-pilot";

const id = "c63ac052-a05a-4b5d-bfff-04429338df90";
const task: PilotTask = { id, message_id: "message-1", instance_id: "instance-1", node_id: "first", status: "pending",
  assignee_id: "user-1", assignee_name: "Harald", title: "Første oppgåve", can_complete: true, delivery_status: "ready" };

test("only exact task message bodies create task controls", () => {
  assert.equal(processTaskId(`[[process-task:${id}]]`), id);
  for (const body of [`Look [[process-task:${id}]]`, `\n[[process-task:${id}]]`, "[[process-task:not-a-uuid]]", `[[process-task:${id}]]\n`, `\`[[process-task:${id}]]\``]) {
    assert.equal(processTaskId(body), null);
  }
});

test("task decoding requires explicit server permission and message binding", () => {
  assert.deepEqual(decodePilotTask(task), task);
  for (const invalid of [{ ...task, can_complete: undefined }, { ...task, can_complete: "true" }, { ...task, message_id: null }, { ...task, status: "done" }, { ...task, delivery_status: "invented" }]) {
    assert.throws(() => decodePilotTask(invalid));
  }
});

test("task disclosure starts collapsed and read-only viewers never get completion controls", () => {
  const api = new ProcessPilotApi(new HttpClient({ fetch: async () => { throw new Error("render must not fetch"); } }), () => "user-1");
  const collapsed = renderToStaticMarkup(createElement(ProcessTaskMessage, { api, taskId: id, messageId: task.message_id }));
  assert.match(collapsed, /<details/);
  assert.ok(!collapsed.includes(" open="));
  assert.ok(!collapsed.includes("[[process-task:"));
  const render = (value: PilotTask) => renderToStaticMarkup(createElement(ProcessTaskDetails, { task: value, busy: false, onComplete() {} }));
  assert.match(render(task), /Fullfør oppgåva/);
  assert.ok(!render({ ...task, can_complete: false }).includes("Fullfør oppgåva"));
  assert.ok(!render({ ...task, status: "completed" }).includes("Fullfør oppgåva"));
  const delivering = render({ ...task, delivery_status: "pending" });
  assert.match(delivering, /Ventar på stadfesting/);
  assert.match(delivering, /disabled/);
  assert.ok(!delivering.includes("Oppgåva er fullført"));
});

test("task API carries authenticated identity and binds each read and completion to its message", async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const api = new ProcessPilotApi(new HttpClient({ participant: () => "participant-1", fetch: async (url, init) => {
    requests.push({ url: String(url), init });
    return Response.json(task);
  } }), () => "user-1");
  await api.task(id, task.message_id);
  await api.complete(id, task.message_id);
  assert.match(requests[0]!.url, /message_id=message-1&participant=participant-1$/);
  assert.equal(requests[0]!.init?.credentials, "same-origin");
  assert.equal(JSON.parse(String(requests[1]!.init?.body)).message_id, task.message_id);
  const copied = new ProcessPilotApi(new HttpClient({ fetch: async () => Response.json(task) }), () => "user-1");
  await assert.rejects(() => copied.task(id, "copied-message"), /anna oppgåve/);
});

test("a lost start response reuses admission id and a confirmed new start gets a new id", async () => {
  const ids: string[] = [];
  let fail = true;
  const api = new ProcessPilotApi(new HttpClient({ fetch: async (_url, init) => {
    ids.push(JSON.parse(String(init?.body)).request_id);
    if (fail) { fail = false; throw new Error("network unavailable"); }
    return Response.json({ id: "instance-1", status: "running" });
  } }), () => "user-1");
  await assert.rejects(() => api.start("channel-1"));
  await api.start("channel-1");
  await api.start("channel-1");
  assert.equal(ids[0], ids[1]);
  assert.notEqual(ids[1], ids[2]);
});
