import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "./api";
import { WorkItemApi, decodeWorkItemTask, workItemTaskId, type WorkItemTask } from "./work-items";

const id = "c63ac052-a05a-4b5d-bfff-04429338df90";
const message = "28f01db0-20f1-42f0-953a-176bc76ce0d1";
const app = "d45a5746-6aba-46a5-9658-4ef56b9bf353";
const task: WorkItemTask = { id, message_id: message, work_item_id: app, revision: 1,
  application_name: "Sprøyt", title: "Feil på mobil", description: "Skrivefeltet forsvinn", status: "pending",
  process_status: "waiting", delivery_status: "ready", category: null, priority: null, decision_status: null,
  assignee_name: "Harald", can_decide: true, blocked: false };

test("only a complete work-item marker becomes a task card", () => {
  assert.equal(workItemTaskId(`[[work-item-task:${id}]]`), id);
  for (const body of [`Look [[work-item-task:${id}]]`, `[[work-item-task:${id}]]\n`, "[[work-item-task:wrong]]"]) {
    assert.equal(workItemTaskId(body), null);
  }
});

test("task decoding requires explicit server permission and known state", () => {
  assert.deepEqual(decodeWorkItemTask(task),task);
  for (const invalid of [{ ...task, can_decide: undefined },{ ...task, blocked: "false" },{ ...task, status: "invented" },{ ...task, message_id: null }]) {
    assert.throws(() => decodeWorkItemTask(invalid));
  }
});

test("registration and decision retries reuse their admission key after an uncertain response", async () => {
  const ids: string[] = [];
  let fail = true;
  const api = new WorkItemApi(new HttpClient({ fetch: async (_url, init) => {
    const body = JSON.parse(String(init?.body)) as { request_id: string };
    ids.push(body.request_id);
    if (fail) { fail = false; throw new Error("connection lost"); }
    return Response.json(String(_url).includes("/decide") ? task : { id, title: "Feil på mobil", status: "new", start_status: "pending" });
  } }), () => "user-1");
  await assert.rejects(() => api.register("channel", message, app, "Feil på mobil", "Skrivefeltet forsvinn", "source"));
  await api.register("channel", message, app, "Feil på mobil", "Skrivefeltet forsvinn", "source");
  assert.equal(ids[0], ids[1]);
  fail = true;
  await assert.rejects(() => api.decide(task, "bug", "high", "planned"));
  await api.decide(task, "bug", "high", "planned");
  assert.equal(ids[2], ids[3]);
  assert.notEqual(ids[0], ids[2]);
});

test("task reads are bound to the actual message and current identity", async () => {
  const requests: string[] = [];
  const api = new WorkItemApi(new HttpClient({ participant: () => "participant-1", fetch: async url => {
    requests.push(String(url)); return Response.json(task);
  } }), () => "user-1");
  assert.equal((await api.task(id, message)).message_id, message);
  assert.match(requests[0]!, /message_id=.*&participant=participant-1$/);
  await assert.rejects(() => api.task(id, "wrong-message"), /anna oppgåve/);
});
