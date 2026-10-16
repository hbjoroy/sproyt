import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "../src/api";
import { CircleChatAgentApi, validAgentHandoff } from "../src/chat-agents";

const source = "00000000-0000-7000-8000-000000000001";
const target = "00000000-0000-7000-8000-000000000002";
const initial = { available: true, revision: 0, target_agent_ids: [], guidance: "", agent_revision: 1 };

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
