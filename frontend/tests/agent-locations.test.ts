import assert from "node:assert/strict";
import test from "node:test";
import { AgentLocationApi, decodeAgentLocation, decodeLocationAgents } from "../src/agent-locations";
import { HttpClient } from "../src/api";

const location = {
  latitude: 60.3913,
  longitude: 5.3221,
  accuracy_m: 18.4,
  observed_at: "2026-10-10T10:00:00.000Z",
  expires_at: "2026-10-10T10:30:00.000Z"
};

test("agent location contract rejects malformed coordinates, dates and duplicate agents", () => {
  assert.deepEqual(decodeAgentLocation(location), {
    latitude: 60.3913, longitude: 5.3221, accuracyM: 18.4,
    observedAt: location.observed_at, expiresAt: location.expires_at
  });
  for (const patch of [
    { latitude: 91 }, { longitude: -181 }, { accuracy_m: -1 }, { accuracy_m: 100_001 },
    { observed_at: "not-a-date" }, { expires_at: null }, { expires_at: "2026-10-10T09:59:59.000Z" }
  ]) assert.throws(() => decodeAgentLocation({ ...location, ...patch }), /Ugyldig posisjonssvar/);
  assert.throws(() => decodeLocationAgents({ agents: [
    { id: "agent-1", name: "Vegvisar", location: null },
    { id: "agent-1", name: "Duplikat", location }
  ] }), /Ugyldig posisjonssvar/);
});

test("agent location API scopes reads, shares and idempotent removal to channel and agent", async () => {
  const calls: Array<{ path: string; method: string; body?: unknown }> = [];
  const api = new AgentLocationApi(new HttpClient({ fetch: async (input, init) => {
    calls.push({ path: String(input), method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : undefined });
    if (init?.method === "PUT") return Response.json(location);
    if (init?.method === "DELETE") return new Response(null, { status: 204 });
    return Response.json({ agents: [{ id: "agent/1", name: "Vegvisar", location: null }] });
  } }));
  const agents = await api.list("kanal/1");
  assert.equal(agents[0]?.name, "Vegvisar");
  await api.share("kanal/1", "agent/1", {
    latitude: 60.39131, longitude: 5.32211, accuracyM: 18.4, observedAt: location.observed_at
  });
  await api.remove("kanal/1", "agent/1");
  assert.deepEqual(calls, [
    { path: "/api/v1/channels/kanal%2F1/agent-locations", method: "GET", body: undefined },
    { path: "/api/v1/channels/kanal%2F1/agent-locations/agent%2F1", method: "PUT",
      body: { latitude: 60.39131, longitude: 5.32211, accuracy_m: 18.4, observed_at: location.observed_at } },
    { path: "/api/v1/channels/kanal%2F1/agent-locations/agent%2F1", method: "DELETE", body: undefined }
  ]);
});
