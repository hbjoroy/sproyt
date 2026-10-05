import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "../src/api";
import { CircleChatAgentApi } from "../src/chat-agents";

const wire = { agent_id: "agent", circle_id: "circle", display_name: "Test", trigger_words: ["hei"],
  response_phrases: ["Kort"], enabled: false, revision: 1, worker_available: true };
const input = { displayName: "Test", triggerWords: ["hei"], responsePhrases: ["Kort"], enabled: false };

test("image configuration distinguishes omission, explicit disable and removal; old APIs stay off", async () => {
  let reply: unknown = wire;
  const sent: Record<string, unknown>[] = [];
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async (_, init) => {
    sent.push(JSON.parse(String(init?.body))); return Response.json(reply);
  } }));
  const old = await api.update("circle", "agent", input);
  assert.equal(old.imageGeneration, null); assert.equal(old.imageGenerationAvailable, false);
  assert.ok(!("image_generation" in sent[0]!));
  const config = { identity_id: "maria-v1", enabled: true, occasional: true };
  reply = { ...wire, image_generation: config, image_generation_available: true };
  const saved = await api.update("circle", "agent", { ...input, imageGeneration: { identityId: "maria-v1", enabled: true, occasional: true } });
  assert.deepEqual(sent[1]!.image_generation, config);
  assert.deepEqual(saved.imageGeneration, { identityId: "maria-v1", enabled: true, occasional: true });
  await api.update("circle", "agent", { ...input, imageGeneration: { identityId: "maria-v1", enabled: false, occasional: false } });
  assert.deepEqual(sent[2]!.image_generation, { identity_id: "maria-v1", enabled: false, occasional: false });
  reply = { ...wire, image_generation: null };
  await api.update("circle", "agent", { ...input, imageGeneration: null });
  assert.equal(sent[3]!.image_generation, null);
  for (const invalid of [{ ...config, enabled: "true" }, { ...config, identity_id: "" }, { ...config, occasional: 1 }]) {
    reply = { ...wire, image_generation: invalid }; await assert.rejects(api.create("circle", input), /Ugyldig biletoppsett/);
  }
});

test("image identities and capability availability come from the server with bounded strict decoding", async () => {
  let reply: unknown = { agents: [wire], worker_available: true };
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async () => Response.json(reply) }));
  const old = await api.list("circle"); assert.equal(old.imageGenerationAvailable, false); assert.deepEqual(old.imageIdentities, []);
  const identity = { id: "other-v2", label: "Another identity" };
  reply = { agents: [wire], worker_available: true, image_generation_available: true, image_identities: [identity] };
  assert.deepEqual((await api.list("circle")).imageIdentities, [identity]);
  for (const invalid of ["wrong", [{ id: 4, label: "x" }], [identity, identity], Array.from({ length: 51 }, (_, i) => ({ id: String(i), label: "x" }))]) {
    reply = { agents: [], worker_available: true, image_identities: invalid }; await assert.rejects(api.list("circle"), /Ugyldig biletidentitet/);
  }
  reply = { agents: [], worker_available: true, image_generation_available: "yes" };
  await assert.rejects(api.list("circle"), /Ugyldig agentliste/);
});
