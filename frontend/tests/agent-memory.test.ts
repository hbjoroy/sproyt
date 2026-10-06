import assert from "node:assert/strict";
import test from "node:test";
import { AgentMemoryApi, decodeAgentMemory, decodeMemoryAgents, validMemoryText } from "../src/agent-memory";
import { HttpClient } from "../src/api";
const circle = "00000000-0000-7000-8000-000000003001";
const agent = "00000000-0000-7000-8000-000000003002";
const view = { circle_id: circle, agent_id: agent, enabled: false, agent_enabled: true,
  collection_available: false, collection_started_at: null, revision: 0, memory_epoch: 1,
  history_compactions: 0, notes: [], unavailable_notes: 0 };
test("memory contract rejects unsafe revisions, malformed notes and duplicate agents", () => {
  assert.equal(decodeAgentMemory(view).collectionAvailable, false);
  for (const patch of [{ revision: -1 }, { revision: 0.5 }, { revision: Number.MAX_SAFE_INTEGER + 1 },
    { memory_epoch: 0 }, { enabled: "true" }, { collection_started_at: 1e15 }, { unavailable_notes: -1 },
    { notes: [{}] }, { agent_id: "bad-id" }]) assert.throws(() => decodeAgentMemory({ ...view, ...patch }));
  assert.throws(() => decodeMemoryAgents({ agents: [{ agent_id: agent, display_name: "Maria" }, { agent_id: agent, display_name: "Maria" }] }));
  assert.equal(validMemoryText("🌴".repeat(256)), true);
  assert.equal(validMemoryText("🌴".repeat(257)), false);
  assert.equal(validMemoryText("\t\n"), false);
  assert.equal(validMemoryText("abc\u0000"), false);
  const note = { id: agent, channel_id: circle, kind: "preference", content: { text: "Nynorsk", participant_ids: [] },
    origin: "automatic", evidence: "user_stated", revision: 1, created_at: 1, updated_at: 1, expires_at: null, source_message_ids: [circle] };
  assert.equal(decodeAgentMemory({ ...view, notes: [note] }).notes[0]?.kind, "preference");
  for (const field of ["kind", "origin", "evidence"] as const) {
    assert.throws(() => decodeAgentMemory({ ...view, notes: [{ ...note, [field]: [note[field]] }] }));
  }
});
test("memory API verifies response scope and sends only revision and own action", async () => {
  const requests: Array<{ path: string; body: unknown }> = [];
  let response = view;
  const api = new AgentMemoryApi(new HttpClient({ fetch: async (input, options) => {
    requests.push({ path: String(input), body: options?.body ? JSON.parse(String(options.body)) : null });
    return Response.json(response);
  } }));
  await api.action(circle, agent, 3, { action: "correct", note_id: agent, text: "Nynorsk" });
  assert.deepEqual(requests[0], { path: `/api/v1/me/circles/${circle}/chat-agents/${agent}/memory/actions`,
    body: { revision: 3, action: "correct", note_id: agent, text: "Nynorsk" } });
  response = { ...view, circle_id: agent };
  await assert.rejects(api.get(circle, agent), /Ugyldig/);
});
test("agent memory configuration preserves older omitted updates", async () => {
  const { CircleChatAgentApi } = await import("../src/chat-agents");
  let sent: any;
  let enabled: unknown = true;
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async (_input, options) => {
    sent = JSON.parse(String(options?.body));
    return Response.json({ agent_id: agent, circle_id: circle, display_name: "Maria", trigger_words: ["hi"],
      response_phrases: ["warm"], enabled: false, memory_enabled: enabled, revision: 1, worker_available: false });
  } }));
  const input = { displayName: "Maria", triggerWords: ["hi"], responsePhrases: ["warm"], enabled: false };
  assert.equal((await api.update(circle, agent, input)).memoryEnabled, true);
  assert.equal("memory_enabled" in sent, false);
  await api.update(circle, agent, { ...input, memoryEnabled: false });
  assert.equal(sent.memory_enabled, false);
  enabled = "true";
  await assert.rejects(api.update(circle, agent, input), /Ugyldig/);
});
