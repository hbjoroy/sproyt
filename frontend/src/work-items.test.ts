import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "./api";
import { WorkItemApi, decodePublicWorkItemStatus, decodeWorkItemTask, decodeSourceWorkItem, githubWorkItemDraft, validSupplementBody, workItemStatusId, workItemTaskId, type WorkItemTask } from "./work-items";

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

test("public status marker and decoder never expose internal notes", () => {
  assert.equal(workItemStatusId(`[[work-item-status:${id}]]`), id);
  assert.equal(workItemStatusId(`Text [[work-item-status:${id}]]`), null);
  assert.equal(workItemStatusId(`[[work-item-status:${id}]]\n`), null);
  assert.deepEqual(decodePublicWorkItemStatus({ visible: false }), { visible: false });
  assert.throws(() => decodePublicWorkItemStatus({ visible: false, title: "Secret" }));
  const visible = { visible: true, title: "Mobile bug", application_name: "Sprøyt", status: "planned",
    public_feedback: "Queued", history: [{ from_status: "new", to_status: "planned", created_at: 123, public_feedback: "Queued" }] };
  assert.deepEqual(decodePublicWorkItemStatus(visible), visible);
  assert.throws(() => decodePublicWorkItemStatus({ ...visible, internal_note: null }));
  assert.throws(() => decodePublicWorkItemStatus({ ...visible, history: [{ ...visible.history[0], internal_note: "Private" }] }));
});

test("public status reads carry the exact message proof", async () => {
  const requests: string[] = [];
  const api = new WorkItemApi(new HttpClient({ fetch: async url => {
    requests.push(String(url)); return Response.json({ visible: false });
  } }), () => "status-reader");
  assert.deepEqual(await api.publicStatus(id, message), { visible: false });
  assert.match(requests[0]!, new RegExp(`/work-items/${id}/status\\?message_id=${message}$`));
});

test("status task decoder requires explicit lifecycle and valid history", () => {
  const lifecycle = { case_status: "planned", can_start: false, allowed_statuses: ["in_development", "resolved", "rejected"],
    internal_note: null, public_feedback: null, history: [{ from_status: "new", to_status: "planned", actor_name: "Harald",
      created_at: 123, internal_note: "Private", public_feedback: "Queued" }] };
  const statusTask = { ...task, node_id: "change-status", can_request_information: false, lifecycle };
  assert.deepEqual(decodeWorkItemTask(statusTask), statusTask);
  assert.throws(() => decodeWorkItemTask({ ...statusTask, lifecycle: null }));
  assert.throws(() => decodeWorkItemTask({ ...statusTask, lifecycle: { ...lifecycle, can_start: "yes" } }));
  assert.throws(() => decodeWorkItemTask({ ...statusTask, lifecycle: { ...lifecycle, history: [{ ...lifecycle.history[0], internal_note: {} }] } }));
});

test("status start and decision retries preserve exact payload and revision", async () => {
  const statusTask: WorkItemTask = { ...task, node_id: "change-status", can_request_information: false,
    lifecycle: { case_status: "planned", can_start: true, allowed_statuses: ["in_development", "resolved", "rejected"],
      internal_note: null, public_feedback: null, history: [] } };
  const saved = new Map<string, string>();
  const previous = globalThis.sessionStorage;
  Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: {
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: (key: string, value: string) => { saved.set(key, value); },
    removeItem: (key: string) => { saved.delete(key); }
  } });
  try {
    const requests: Record<string, unknown>[] = [];
    let fail = true;
    const http = new HttpClient({ fetch: async (url, init) => {
      requests.push(JSON.parse(String(init?.body)) as Record<string, unknown>);
      if (fail) { fail = false; throw new Error("response lost"); }
      return Response.json(String(url).endsWith("/status-change")
        ? { id, work_item_id: app, channel_name: "# Review", start_status: "pending" }
        : { ...statusTask, revision: 2, can_decide: false, delivery_status: "pending" });
    } });
    const first = new WorkItemApi(http, () => "status-user");
    await assert.rejects(() => first.startStatusChange(statusTask));
    const second = new WorkItemApi(http, () => "status-user");
    await second.startStatusChange({ ...statusTask, revision: 2 });
    assert.deepEqual(requests[0], requests[1]);
    assert.equal(requests[1]!.expected_revision, 1);
    fail = true;
    await assert.rejects(() => second.changeStatus(statusTask, "in_development", "Private", "Public", false));
    assert.deepEqual(second.pendingStatusChange(statusTask), { status: "in_development", internal_note: "Private",
      public_feedback: "Public", no_change: false });
    const third = new WorkItemApi(http, () => "status-user");
    await assert.rejects(() => third.changeStatus(statusTask, "resolved", "Private", "Public", false), /same val/);
    await third.changeStatus({ ...statusTask, revision: 2 }, "in_development", "Private", "Public", false);
    assert.deepEqual(requests[2], requests[3]);
    assert.equal(requests[3]!.expected_revision, 1);
  } finally {
    if (previous === undefined) delete (globalThis as { sessionStorage?: Storage }).sessionStorage;
    else Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: previous });
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

const sourceItem = { id: app, source_message_id: message, revision: 1, title: "Feil på mobil", description: "Skrivefeltet forsvinn",
  application_name: "Sprøyt", status: "reviewing", can_supplement: true, supplements: [] };
const extra = { id, actor_name: "Innmeldar", body: "Også på iPhone", created_at: "2026-10-04 20:30:00+00" };

test("source case DTO rejects internal fields and requires source binding, explicit rights and public supplement history", async () => {
  assert.deepEqual(decodeSourceWorkItem({ ...sourceItem, supplements: [extra] }).supplements, [extra]);
  for (const value of [{ ...sourceItem, internal_note: "secret" }, { ...sourceItem, can_supplement: "yes" },
    { ...sourceItem, revision: 0 }, { ...sourceItem, supplements: [{ ...extra, actor_id: "private" }] },
    { ...sourceItem, supplements: [{ ...extra, created_at: 123 }] }]) assert.throws(() => decodeSourceWorkItem(value));
  const api = new WorkItemApi(new HttpClient({ fetch: async () => Response.json([sourceItem]) }), () => "user");
  await assert.rejects(api.sourceItems("channel", id), /anna melding/);
  assert.ok(validSupplementBody("ø".repeat(4000))); assert.equal(validSupplementBody("ø".repeat(4001)), false);
  assert.equal(validSupplementBody(" "), false);
});

test("supplement retry journal survives reload with exact original revision and clears only after acceptance or confirmed conflict", async () => {
  const previousStorage = globalThis.sessionStorage;
  const stored = new Map<string, string>();
  Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: {
    getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => stored.set(key, value), removeItem: (key: string) => stored.delete(key)
  } });
  try {
    let result = "lost";
    const bodies: Record<string, unknown>[] = [];
    const http = new HttpClient({ fetch: async (_, init) => {
      bodies.push(JSON.parse(String(init?.body)));
      if (result === "lost") throw new Error("accepted response lost");
      if (result === "conflict") return new Response("Ny informasjon", { status: 409 });
      return Response.json({ ...sourceItem, revision: 2, supplements: [extra] });
    } });
    const first = new WorkItemApi(http, () => "requester");
    await assert.rejects(first.supplement(sourceItem, "Også på iPhone"));
    const next = new WorkItemApi(http, () => "requester");
    assert.equal(next.pendingSupplement(sourceItem), "Også på iPhone");
    await assert.rejects(next.supplement({ ...sourceItem, revision: 2 }, "Endra tekst"), /same val/);
    result = "accepted"; await next.supplement({ ...sourceItem, revision: 2 }, "Også på iPhone");
    assert.deepEqual(bodies[0], bodies[1]); assert.equal(bodies[1]!.expected_revision, 1);
    assert.equal(next.pendingSupplement(sourceItem), null);
    result = "conflict"; await assert.rejects(next.supplement(sourceItem, "Ny tekst"));
    const conflicted = bodies.at(-1)!; assert.equal(next.pendingSupplement(sourceItem), null);
    result = "accepted"; await next.supplement({ ...sourceItem, revision: 3 }, "Ny tekst");
    assert.equal(bodies.at(-1)!.expected_revision, 3); assert.notEqual(bodies.at(-1)!.request_id, conflicted.request_id);
  } finally {
    if (previousStorage === undefined) delete (globalThis as { sessionStorage?: Storage }).sessionStorage;
    else Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: previousStorage });
  }
});

test("review retries reject changed choices and GitHub initial draft includes supplements without rewriting server text", async () => {
  const bodies: any[] = [];
  const api = new WorkItemApi(new HttpClient({ fetch: async (_, init) => {
    bodies.push(JSON.parse(String(init?.body))); if (bodies.length === 1) throw new Error("lost"); return Response.json(task);
  } }), () => "reviewer");
  await assert.rejects(api.decide(task, "bug", "high", "planned"));
  await assert.rejects(api.decide({ ...task, revision: 2 }, "bug", "low", "planned"), /same val/);
  await api.decide({ ...task, revision: 2 }, "bug", "high", "planned");
  assert.deepEqual(bodies[0], bodies[1]); assert.equal(bodies[1].expected_revision, 1);
  assert.match(githubWorkItemDraft({ ...task, supplements: [extra] }), /Skrivefeltet forsvinn[\s\S]*Innmeldar[\s\S]*Også på iPhone/);
  assert.throws(() => decodeWorkItemTask({ ...task, supplements: [{ ...extra, internal_note: "private" }] }));
});

test("review admission freezes the last supplement marker across a new client and newly polled information", async () => {
  const previousStorage = globalThis.sessionStorage;
  const stored = new Map<string, string>();
  Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: {
    getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => stored.set(key, value), removeItem: (key: string) => stored.delete(key)
  } });
  try {
    const bodies: any[] = [];
    const http = new HttpClient({ fetch: async (_, init) => {
      bodies.push(JSON.parse(String(init?.body))); if (bodies.length === 1) throw new Error("lost"); return Response.json(task);
    } });
    const first = new WorkItemApi(http, () => "marker-reviewer");
    await assert.rejects(first.decide({ ...task, supplements: [extra] }, "bug", "high", "planned"));
    const next = new WorkItemApi(http, () => "marker-reviewer");
    const refreshed = { ...task, revision: 2, supplements: [extra, { ...extra, id: message, body: "Nyare informasjon" }] };
    assert.equal(next.pendingDecision(refreshed)?.expected_supplement_id, extra.id);
    await next.decide(refreshed, "bug", "high", "planned");
    assert.deepEqual(bodies[0], bodies[1]); assert.equal(bodies[1].expected_supplement_id, extra.id); assert.equal(bodies[1].expected_revision, 1);
  } finally {
    if (previousStorage === undefined) delete (globalThis as { sessionStorage?: Storage }).sessionStorage;
    else Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: previousStorage });
  }
});
