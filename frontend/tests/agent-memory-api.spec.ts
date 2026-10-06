import { expect, test } from "@playwright/test";
import { randomUUID } from "node:crypto";

test("own memory API keeps consent inactive, enforces revisions and exports its snapshot", async ({ request }) => {
  const suffix = randomUUID();
  const owner = `memory-${suffix}`;
  const outsider = `outside-${suffix}`;
  const query = `?participant=${owner}`;
  // Materialize the development identity through the real account endpoint.
  expect((await request.get(`/api/v1/me/emojis${query}`)).ok()).toBeTruthy();
  const created = await request.post(`/api/v1/commands${query}`, { data: {
    protocol: "sproyt.chat.v1", request_id: suffix, type: "create_circle",
    payload: { slug: `memory-${suffix}`, name: "Memory API contract" }
  } });
  const circleReply = await created.json();
  expect(circleReply.type).toBe("circle_created");
  const circle = circleReply.payload.circle.id;
  const configuration = { display_name: "Memory agent", trigger_words: ["hello"],
    response_phrases: ["Warm"], enabled: false };
  const agentResponse = await request.post(`/api/v1/circles/${circle}/chat-agents${query}`,
    { data: { ...configuration, memory_enabled: true } });
  expect(agentResponse.status()).toBe(201);
  const agent = (await agentResponse.json()).agent_id;
  const memory = `/api/v1/me/circles/${circle}/chat-agents/${agent}/memory`;
  const initial = await request.get(`${memory}${query}`);
  expect(initial.headers()["cache-control"]).toBe("no-store");
  expect(await initial.json()).toMatchObject({ revision: 0, enabled: false,
    agent_enabled: true, collection_available: false, collection_started_at: null, notes: [] });
  const consent = await request.patch(`${memory}${query}`, { data: { revision: 0, enabled: true } });
  expect(consent.status()).toBe(200);
  expect(await consent.json()).toMatchObject({ revision: 1, memory_epoch: 2,
    enabled: true, collection_available: false, collection_started_at: null });
  const denied = await request.get(`${memory}?participant=${outsider}`);
  expect(denied.status()).toBe(403);
  expect(denied.headers()["cache-control"]).toBe("no-store");
  const forged = await request.patch(`${memory}${query}`, { data: { revision: 1, enabled: true, user_id: suffix } });
  expect(forged.status()).toBe(422);
  const crossOrigin = await request.post(`${memory}/actions${query}`, {
    headers: { origin: "https://example.invalid" }, data: { revision: 1, action: "reset" }
  });
  expect(crossOrigin.status()).toBe(403);
  const reset = await request.post(`${memory}/actions${query}`, { data: { revision: 1, action: "reset" } });
  expect(reset.status()).toBe(200);
  expect(await reset.json()).toMatchObject({ revision: 2, memory_epoch: 3, notes: [] });
  const stale = await request.patch(`${memory}${query}`, { data: { revision: 1, enabled: false } });
  expect(stale.status()).toBe(409);
  expect(stale.headers()["cache-control"]).toBe("no-store");
  const exported = await request.get(`/api/v1/me/export${query}`);
  expect(exported.status()).toBe(200);
  expect((await exported.json()).agent_memories).toEqual([expect.objectContaining({
    agent_id: agent, circle_id: circle, revision: 2, memory_epoch: 3, enabled: true, notes: []
  })]);
  // An older client omitting the optional configuration keeps the choice.
  const update = await request.patch(`/api/v1/circles/${circle}/chat-agents/${agent}${query}`,
    { data: { ...configuration, revision: 1 } });
  expect(update.status()).toBe(200);
  expect((await update.json()).memory_enabled).toBe(true);
});
