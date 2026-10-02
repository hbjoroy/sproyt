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
  assignee_name: "Harald", can_decide: true, blocked: false, node_id: "review", can_request_information: true,
  information_request: null, information_response: null };

test("only a complete work-item marker becomes a task card", () => {
  assert.equal(workItemTaskId(`[[work-item-task:${id}]]`), id);
  for (const body of [`Look [[work-item-task:${id}]]`, `[[work-item-task:${id}]]\n`, "[[work-item-task:wrong]]"]) {
    assert.equal(workItemTaskId(body), null);
  }
});

test("task decoding requires explicit server permission and known state", () => {
  assert.deepEqual(decodeWorkItemTask(task),task);
  for (const invalid of [{ ...task, can_decide: undefined },{ ...task, blocked: "false" },{ ...task, status: "invented" },{ ...task, message_id: null },{ ...task, node_id: "invented" },{ ...task, node_id: "followup-review", can_request_information: true },{ ...task, information_request: {} }]) {
    assert.throws(() => decodeWorkItemTask(invalid));
  }
});

test("GitHub export decoding requires a complete, explicit server capability", () => {
  const github = { repository: "owner/public-repo", repository_id: 42, binding_revision: 3, can_publish: true, status: "ready", issue_url: null,
    title: "Feil på mobil", body: "Skrivefeltet forsvinn" };
  const exportTask = { ...task, node_id: "publish-github", can_request_information: false, github_export: github };
  assert.deepEqual(decodeWorkItemTask(exportTask), exportTask);
  for (const invalid of [
    { ...exportTask, github_export: null },
    { ...exportTask, github_export: { ...github, can_publish: "true" } },
    { ...exportTask, github_export: { ...github, repository_id: "42" } },
    { ...exportTask, github_export: { ...github, status: "unknown" } },
    { ...exportTask, github_export: { ...github, issue_url: 3 } }
  ]) assert.throws(() => decodeWorkItemTask(invalid));
});

test("GitHub retry reuses exact command and accepted revision, even after a new API instance", async () => {
  const githubTask: WorkItemTask = { ...task, node_id: "publish-github", can_request_information: false, github_export: {
    repository: "owner/public-repo", repository_id: 42, binding_revision: 3, can_publish: true, status: "ready", issue_url: null,
    title: task.title, body: task.description } };
  const storage = new Map<string, string>();
  const previousStorage = globalThis.sessionStorage;
  Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => { storage.set(key, value); },
    removeItem: (key: string) => { storage.delete(key); }
  } });
  try {
    const bodies: Record<string, unknown>[] = [];
    let fail = true;
    const http = new HttpClient({ fetch: async (url, init) => {
      assert.match(String(url), /\/work-item-tasks\/.*\/github$/);
      bodies.push(JSON.parse(String(init?.body)) as Record<string, unknown>);
      if (fail) { fail = false; throw new Error("accepted response lost"); }
      return Response.json({ ...githubTask, revision: 2, can_decide: false, github_export: {
        ...githubTask.github_export, status: "pending" } });
    } });
    const first = new WorkItemApi(http, () => "github-user");
    await assert.rejects(() => first.exportGithub(githubTask, "Edited title", "Edited body", true));
    assert.deepEqual(first.pendingGithubExport(githubTask), { title: "Edited title", body: "Edited body", send: true,
      expected_repository_id: 42, expected_binding_revision: 3 });
    const second = new WorkItemApi(http, () => "github-user");
    await assert.rejects(() => second.exportGithub({ ...githubTask, revision: 2 }, "Different", "Edited body", true), /same innhald/);
    await second.exportGithub({ ...githubTask, revision: 2 }, "Edited title", "Edited body", true);
    assert.deepEqual(bodies[0], bodies[1]);
    assert.equal(bodies[1]!.expected_revision, 1);
    assert.equal(bodies[1]!.send, true);
    assert.equal(bodies[1]!.expected_repository_id, 42);
    assert.equal(bodies[1]!.expected_binding_revision, 3);
    assert.equal(second.pendingGithubExport(githubTask), null);
  } finally {
    if (previousStorage === undefined) delete (globalThis as { sessionStorage?: Storage }).sessionStorage;
    else Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: previousStorage });
  }
});

test("GitHub export refuses a changed repository binding before sending", async () => {
  let calls = 0;
  const taskWithGithub: WorkItemTask = { ...task, node_id: "publish-github", can_request_information: false,
    github_export: { repository: "owner/repo", repository_id: 42, binding_revision: 4, can_publish: true,
      status: "ready", issue_url: null, title: null, body: null } };
  const api = new WorkItemApi(new HttpClient({ fetch: async () => { calls++; return Response.json(taskWithGithub); } }), () => "binding-user");
  await assert.rejects(() => api.exportGithub(taskWithGithub, "Reviewed bug", "Approved body", true,
    { repository_id: 42, binding_revision: 3 }), /målet er endra/);
  assert.equal(calls, 0);
});

test("information retries keep the accepted revision and exact text after refresh", async () => {
  const bodies: Record<string, unknown>[] = [];
  const api = new WorkItemApi(new HttpClient({ fetch: async (_url, init) => {
    bodies.push(JSON.parse(String(init?.body)) as Record<string, unknown>);
    if (bodies.length === 1) throw new Error("accepted response lost");
    return Response.json(task);
  } }), () => "user-1");
  await assert.rejects(() => api.decide(task, "bug", "high", "needs_information", "Which browser?"));
  await api.decide({ ...task, revision: 2 }, "bug", "high", "needs_information", "Which browser?");
  assert.deepEqual(bodies[0], bodies[1]);
  assert.equal(bodies[1]!.expected_revision, 1);
  assert.equal(bodies[1]!.note, "Which browser?");
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
