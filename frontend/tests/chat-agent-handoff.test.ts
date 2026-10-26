import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "../src/api";
import { CircleChatAgentApi, validAgentHandoff } from "../src/chat-agents";

const source = "00000000-0000-7000-8000-000000000001";
const target = "00000000-0000-7000-8000-000000000002";
const initial = { available: true, revision: 0, target_agent_ids: [], guidance: "", agent_revision: 1 };

const agentInput = { displayName: "Agent", triggerWords: [], responsePhrases: [], enabled: false, revision: 1 };
const operations: ((api: CircleChatAgentApi) => Promise<unknown>)[] = [
  api => api.list("circle"), api => api.create("circle", agentInput), api => api.update("circle", source, agentInput),
  api => api.listChannel("channel"), api => api.selectChannel("channel", source, true, 1),
  api => api.handoff("circle", source), api => api.updateHandoff("circle", source, { revision: 0, targetAgentIds: [], guidance: "" })
];
const reply = () => Response.json({ ...initial, agents: [], worker_available: false,
  agent_id: source, circle_id: "circle", display_name: "Agent", trigger_words: [], response_phrases: [], enabled: false,
  access_revision: 1, selection_available: true });

test("scoped agent requests bound unknown outcomes with a timeout; legacy requests retain their signal behavior", async () => {
  const timeout = AbortSignal.timeout;
  const controller = new AbortController();
  let requests = 0;
  AbortSignal.timeout = milliseconds => { assert.equal(milliseconds, 20_000); return controller.signal; };
  try {
    const http = new HttpClient({ fetch: async (_url, options) => {
      requests++;
      if (!options?.signal) return reply();
      assert.equal(options.signal, controller.signal);
      return new Promise<Response>((_resolve, reject) => options.signal!.addEventListener("abort", () => reject(options.signal!.reason), { once: true }));
    } });
    const api = new CircleChatAgentApi(http);
    await api.list("circle");
    const pending = api.forOwner("alice", () => true).update("circle", source, agentInput);
    controller.abort(new DOMException("Request timed out", "TimeoutError"));
    await assert.rejects(pending, { name: "TimeoutError" });
    assert.equal(requests, 2); assert.equal(http.pendingWrites, 0);
  } finally { AbortSignal.timeout = timeout; }
});

test("all seven scoped agent operations send the account fence; legacy clients omit it", async () => {
  for (const operation of operations) {
    const owners: (string | null)[] = [];
    const api = new CircleChatAgentApi(new HttpClient({ fetch: async (_url, options) => {
      owners.push(new Headers(options?.headers).get("x-sproyt-account")); return reply();
    } }));
    await operation(api.forOwner("alice", () => true));
    await operation(api);
    assert.deepEqual(owners, ["alice", null]);
    await assert.rejects(operation(api.forOwner("alice", () => false)), /Innlogginga vart endra/);
    assert.equal(owners.length, 2);
  }
});

test("all scoped operations reject results completed after an account switch", async () => {
  for (const operation of operations) {
    let admitted = true, complete!: (response: Response) => void;
    const api = new CircleChatAgentApi(new HttpClient({ fetch: () => new Promise(resolve => { complete = resolve; }) }))
      .forOwner("alice", () => admitted);
    const pending = operation(api);
    admitted = false; complete(reply());
    await assert.rejects(pending, /Innlogginga vart endra/);
  }
});

test("401 renewal cannot resend any agent operation under a changed account or an old A-B-A generation", async () => {
  for (const operation of operations) for (const returnToAlice of [false, true]) {
    let owner = "alice", generation = 1, requests = 0;
    const originalGeneration = generation;
    const http = new HttpClient({ fetch: async (_url, options) => {
      requests++; assert.equal(new Headers(options?.headers).get("x-sproyt-account"), "alice");
      return new Response("", { status: 401 });
    }, refreshSession: async () => {
      owner = "bob"; generation++;
      if (returnToAlice) { owner = "alice"; generation++; }
      return true;
    } });
    const api = new CircleChatAgentApi(http).forOwner("alice", () => owner === "alice" && generation === originalGeneration);
    await assert.rejects(operation(api), /Innlogginga vart endra/);
    assert.equal(requests, 1); assert.equal(http.pendingWrites, 0);
  }
});

test("handoff uses separate scoped revisioned configuration and preserves original agent fields", async () => {
  const requests: { url: string; method: string; body: unknown }[] = [];
  let wire: unknown = initial;
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async (url, options) => {
    requests.push({ url: String(url), method: options?.method ?? "GET", body: options?.body ? JSON.parse(String(options.body)) : null });
    return Response.json(wire);
  } }));
  assert.deepEqual(await api.handoff("circle/with space", source), {
    available: true, revision: 0, targetAgentIds: [], guidance: "", agentRevision: 1
  });
  assert.match(requests[0]!.url, /circle%2Fwith%20space/);
  wire = { available: true, revision: 1, target_agent_ids: [target], guidance: "Spør om lokale tips", agent_revision: 2 };
  const saved = await api.updateHandoff("circle", source, { revision: 0, targetAgentIds: [target], guidance: "Spør om lokale tips" });
  assert.equal(saved.agentRevision, 2);
  assert.equal(requests[1]!.method, "PUT");
  assert.deepEqual(requests[1]!.body, { revision: 0, target_agent_ids: [target], guidance: "Spør om lokale tips" });
  assert.ok(!("response_phrases" in (requests[1]!.body as object)));
  wire = { ...initial, available: false };
  assert.equal((await api.handoff("circle", source)).available, false);
  for (const invalid of [null, { ...initial, revision: -1 }, { ...initial, agent_revision: 0 },
    { ...initial, target_agent_ids: [target, target] }, { ...initial, target_agent_ids: [7] },
    { ...initial, guidance: "x".repeat(1001) }, { ...initial, available: "yes" }]) {
    wire = invalid; await assert.rejects(api.handoff("circle", source), /Ugyldig/);
  }
});

test("handoff rejects self, duplicates, excess recipients and malformed policy without a request", async () => {
  let requests = 0;
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async () => { requests++; return Response.json(initial); } }));
  assert.ok(validAgentHandoff({ revision: 0, targetAgentIds: [], guidance: "" }));
  for (const input of [
    { revision: -1, targetAgentIds: [] }, { revision: 0.5, targetAgentIds: [] },
    { revision: 0, targetAgentIds: [source] }, { revision: 0, targetAgentIds: [target, target] },
    { revision: 0, targetAgentIds: ["not-an-agent"] },
    { revision: 0, targetAgentIds: Array(5).fill(target) }
  ]) await assert.rejects(api.updateHandoff("circle", source, { ...input, guidance: "" }));
  assert.equal(requests, 0);
});
