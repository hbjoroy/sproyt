import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "../src/api";
import { CircleChatAgentApi, validAgentSeaTemperature } from "../src/chat-agents";

const wire = { agent_id: "agent", circle_id: "circle", display_name: "Test", trigger_words: ["hei"],
  response_phrases: ["Kort"], enabled: false, revision: 1, worker_available: true };
const input = { displayName: "Test", triggerWords: ["hei"], responsePhrases: ["Kort"], enabled: false };
const sea = { default_location: "parikia", locations: ["parikia", "aliki"] };
const config = { defaultLocation: "parikia", locations: ["parikia", "aliki"] };

test("sea temperature config roundtrips, preserves omitted updates and supports removal without affecting weather", async () => {
  let reply: unknown = wire;
  const sent: Record<string, unknown>[] = [];
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async (_, init) => {
    sent.push(JSON.parse(String(init?.body))); return Response.json(reply);
  } }));
  assert.equal((await api.update("circle", "agent", input)).seaTemperature, null);
  assert.ok(!("sea_temperature" in sent[0]!));
  reply = { ...wire, sea_temperature: sea };
  assert.deepEqual((await api.create("circle", { ...input, seaTemperature: config })).seaTemperature, config);
  assert.deepEqual(sent[1]!.sea_temperature, sea);
  assert.ok(!("weather" in sent[1]!));
  await api.update("circle", "agent", { ...input, revision: 1 });
  assert.ok(!("sea_temperature" in sent[2]!));
  reply = { ...wire, sea_temperature: null };
  assert.equal((await api.update("circle", "agent", { ...input, seaTemperature: null })).seaTemperature, null);
  assert.equal(sent[3]!.sea_temperature, null);
  for (const invalid of [true, { ...sea, default_location: 3 }, { ...sea, locations: "parikia" },
    { ...sea, locations: [1] }, { ...sea, default_location: "nordnes" }, { ...sea, locations: [] },
    { ...sea, locations: ["parikia", "parikia"] }, { ...sea, locations: Array(9).fill("parikia") }]) {
    reply = { ...wire, sea_temperature: invalid };
    await assert.rejects(api.update("circle", "agent", input), /Ugyldig sjøtemperaturoppsett/);
  }
});

test("sea temperature selection bounds count, duplicates and default membership", () => {
  assert.ok(validAgentSeaTemperature(config));
  assert.ok(validAgentSeaTemperature({ defaultLocation: "0", locations: Array.from({ length: 8 }, (_, i) => String(i)) }));
  for (const invalid of [{ locations: [] }, { locations: ["parikia", "parikia"] }, { defaultLocation: "bjoroy" },
    { locations: ["parikia", ""] }, { locations: ["parikia", " x"] }, { locations: ["parikia", "x".repeat(81)] },
    { locations: Array.from({ length: 9 }, (_, i) => String(i)), defaultLocation: "0" }]) {
    assert.equal(validAgentSeaTemperature({ ...config, ...invalid }), false);
  }
});

test("sea temperature capability and admitted locations come from server and default safely for old APIs", async () => {
  let reply: unknown = { agents: [wire], worker_available: true };
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async () => Response.json(reply) }));
  const old = await api.list("circle");
  assert.equal(old.seaTemperatureAvailable, false); assert.deepEqual(old.seaTemperatureLocations, []);
  assert.equal(old.agents[0]!.seaTemperature, null);
  const catalogue = [{ id: "parikia", name: "Parikia" }, { id: "bjoroy", name: "Bjorøy" }];
  reply = { agents: [{ ...wire, sea_temperature: sea }], worker_available: true,
    sea_temperature_available: true, sea_temperature_locations: catalogue };
  const listed = await api.list("circle");
  assert.equal(listed.seaTemperatureAvailable, true); assert.deepEqual(listed.seaTemperatureLocations, catalogue);
  assert.deepEqual(listed.agents[0]!.seaTemperature, config);
  for (const invalid of [null, "wrong", [{ id: 4, name: "x" }], [{ id: "x", name: " " }],
    [catalogue[0], catalogue[0]], Array.from({ length: 51 }, (_, i) => ({ id: String(i), name: "x" }))]) {
    reply = { agents: [], worker_available: true, sea_temperature_locations: invalid };
    await assert.rejects(api.list("circle"), /Ugyldig sjøtemperaturstad/);
  }
  reply = { agents: [], worker_available: true, sea_temperature_available: "yes" };
  await assert.rejects(api.list("circle"), /Ugyldig agentliste/);
});
