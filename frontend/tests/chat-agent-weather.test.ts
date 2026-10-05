import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "../src/api";
import { CircleChatAgentApi, validAgentWeather } from "../src/chat-agents";

test("weather settings roundtrip while old agent replies and omitted inputs remain compatible", async () => {
  const weather = { location: "Parikia", latitude: 37.085, longitude: 25.148 };
  const wire = { agent_id: "agent", circle_id: "circle", display_name: "Vêrven", trigger_words: ["vêr"], response_phrases: ["Kort svar"], enabled: false, revision: 1, worker_available: false };
  const bodies: unknown[] = [];
  let reply: unknown = wire;
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async (_, init) => {
    const body = JSON.parse(String(init?.body)); bodies.push(body);
    return Response.json(reply);
  } }));
  const input = { displayName: "Vêrven", triggerWords: ["vêr"], responsePhrases: ["Kort svar"], enabled: false };
  assert.equal((await api.create("circle", input)).weather, null);
  assert.ok(!("weather" in (bodies[0] as object)));
  reply = { ...wire, weather };
  assert.deepEqual((await api.update("circle", "agent", { ...input, weather, revision: 1 })).weather, weather);
  assert.deepEqual((bodies[1] as { weather: unknown }).weather, weather);
  reply = { ...wire, weather: null };
  assert.equal((await api.update("circle", "agent", { ...input, weather: null })).weather, null);
  assert.equal((bodies[2] as { weather: unknown }).weather, null);
  reply = { ...wire, weather: { ...weather, latitude: 91 } };
  await assert.rejects(api.create("circle", input), /Ugyldig vêroppsett/);
});

test("weather validation bounds named fixed locations and finite coordinates", () => {
  const weather = { location: "Parikia", latitude: 37.085, longitude: 25.148 };
  assert.ok(validAgentWeather(weather));
  assert.ok(validAgentWeather({ location: "x".repeat(80), latitude: -90, longitude: 180 }));
  for (const invalid of [{ location: " " }, { location: "x".repeat(81) }, { latitude: 90.01 }, { latitude: NaN }, { longitude: -180.01 }, { longitude: Infinity }])
    assert.equal(validAgentWeather({ ...weather, ...invalid }), false);
});

test("weather availability defaults closed for old lists and requires a boolean when present", async () => {
  let reply: unknown = { agents: [], worker_available: true };
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async () => Response.json(reply) }));
  assert.equal((await api.list("circle")).weatherAvailable, false);
  reply = { agents: [], worker_available: false, weather_available: true };
  assert.equal((await api.list("circle")).weatherAvailable, true);
  reply = { agents: [], worker_available: true, weather_available: "yes" };
  await assert.rejects(api.list("circle"), /Ugyldig agentliste/);
});

test("ferry configuration preserves omitted updates, carries opt-in and rejects unknown ports", async () => {
  const wire = { agent_id: "agent", circle_id: "circle", display_name: "Maria", trigger_words: ["hei"], response_phrases: ["Kort svar"], enabled: false, revision: 1, worker_available: false };
  let reply: unknown = wire;
  const bodies: Record<string, unknown>[] = [];
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async (_, init) => {
    if (init?.body) bodies.push(JSON.parse(String(init.body)));
    return Response.json(reply);
  } }));
  const input = { displayName: "Maria", triggerWords: ["hei"], responsePhrases: ["Kort svar"], enabled: false };
  assert.equal((await api.update("circle", "agent", input)).ferryPort, null);
  assert.ok(!("ferry_port" in bodies[0]!));
  reply = { ...wire, ferry_port: "paros" };
  assert.equal((await api.update("circle", "agent", { ...input, ferryPort: "paros" })).ferryPort, "paros");
  assert.equal(bodies[1]!.ferry_port, "paros");
  reply = { ...wire, ferry_port: null };
  await api.update("circle", "agent", { ...input, ferryPort: null });
  assert.equal(bodies[2]!.ferry_port, null);
  reply = { ...wire, ferry_port: "naxos" };
  await assert.rejects(api.create("circle", input), /Ugyldig fergehamn/);
  reply = { agents: [], worker_available: true };
  assert.equal((await api.list("circle")).ferryAvailable, false);
  reply = { agents: [], worker_available: true, ferry_available: true };
  assert.equal((await api.list("circle")).ferryAvailable, true);
  reply = { agents: [], worker_available: true, ferry_available: "true" };
  await assert.rejects(api.list("circle"), /Ugyldig agentliste/);
});

test("vision opt-in roundtrips for any agent while omitted PATCH fields preserve existing choices", async () => {
  const wire = { agent_id: "agent", circle_id: "circle", display_name: "Biletven", trigger_words: ["sjå"], response_phrases: ["Svar kort"],
    enabled: false, revision: 3, worker_available: true, vision_enabled: true, vision_available: true };
  const bodies: Record<string, unknown>[] = [];
  let reply: unknown = wire;
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async (_, init) => {
    if (init?.body) bodies.push(JSON.parse(String(init.body))); return Response.json(reply);
  } }));
  const input = { displayName: "Biletven", triggerWords: ["sjå"], responsePhrases: ["Svar kort"], enabled: false, revision: 3 };
  const preserved = await api.update("circle", "agent", input);
  assert.equal(preserved.visionEnabled, true); assert.equal(preserved.visionAvailable, true);
  assert.ok(!("vision_enabled" in bodies[0]!));
  assert.ok(!("weather" in bodies[0]!)); assert.ok(!("ferry_port" in bodies[0]!));
  reply = { ...wire, vision_enabled: false };
  assert.equal((await api.update("circle", "agent", { ...input, visionEnabled: false })).visionEnabled, false);
  assert.equal(bodies[1]!.vision_enabled, false);
  reply = wire; await api.create("circle", { ...input, visionEnabled: true }); assert.equal(bodies[2]!.vision_enabled, true);
  for (const invalid of [{ vision_enabled: "true" }, { vision_available: 1 }, { vision_enabled: null }]) {
    reply = { ...wire, ...invalid }; await assert.rejects(api.update("circle", "agent", input), /Ugyldig agentsvar/);
  }
});

test("old vision replies default safely off and unavailable; list availability is explicit", async () => {
  let reply: unknown = { agents: [{ agent_id: "agent", circle_id: "circle", display_name: "Vanleg agent", trigger_words: [], response_phrases: [], enabled: false, revision: 1, worker_available: true }], worker_available: true };
  const api = new CircleChatAgentApi(new HttpClient({ fetch: async () => Response.json(reply) }));
  const old = await api.list("circle");
  assert.equal(old.agents[0]!.visionEnabled, false); assert.equal(old.agents[0]!.visionAvailable, false); assert.equal(old.visionAvailable, false);
  reply = { agents: [], worker_available: true, vision_available: true }; assert.equal((await api.list("circle")).visionAvailable, true);
  reply = { agents: [], worker_available: true, vision_available: "true" }; await assert.rejects(api.list("circle"), /Ugyldig agentliste/);
});
