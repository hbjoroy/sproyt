import assert from "node:assert/strict";
import { test } from "node:test";
import { HttpClient } from "../src/api";
import { AgentGeoapifyRollApi } from "../src/agent-geoapify-roll";

const binding = "a".repeat(64);
const preview = { provider: "geoapify", preview_binding: binding, origin_mode: "named", suggested_origin: "Nørreport", radius_m: 1500,
  categories: ["catering.bar", "catering.pub"], state: null };
const state = { id: "roll-1", status: "choose_origin", choices: [{ index: 0, label: "Nørreport, København" }], error_code: null, result_message_id: null };
const json = (body: unknown) => new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } });

test("Geoapify consent is explicit, scoped and bound; origin selection is separate", async () => {
  const calls: Array<{ url: string; init: RequestInit }> = [];
  const api = new AgentGeoapifyRollApi(new HttpClient({ fetch: async (url, init) => {
    calls.push({ url: String(url), init: init ?? {} }); return json(init?.method === "GET" ? preview : state);
  } }));
  const p = await api.preview("alice", "channel", "message", () => true);
  assert.equal(p.suggestedOrigin, "Nørreport");
  const roll = await api.start("alice", "channel", "message", p, " Nørreport ", () => true);
  assert.equal(roll.choices[0]?.label, "Nørreport, København");
  await api.choose("alice", "channel", "message", roll.id, 0, () => true);
  assert.equal(calls[0]?.url, "/api/v1/channels/channel/messages/message/geoapify-roll");
  assert.equal(calls[2]?.url, "/api/v1/channels/channel/messages/message/geoapify-roll/origin");
  assert.deepEqual(JSON.parse(String(calls[1]?.init.body)), { preview_binding: binding, allow_shared_location: false, origin_text: "Nørreport" });
  assert.deepEqual(JSON.parse(String(calls[2]?.init.body)), { roll_id: "roll-1", index: 0 });
  for (const call of calls) { assert.equal(new Headers(call.init.headers).get("x-sproyt-account"), "alice"); assert.equal(call.init.cache, "no-store"); }
});

test("shared position is separately consented and malformed preview is rejected", async () => {
  let body: unknown = { ...preview, origin_mode: "shared", suggested_origin: null };
  let posted: unknown;
  const api = new AgentGeoapifyRollApi(new HttpClient({ fetch: async (_url, init) => {
    if (init?.method === "POST") { posted = JSON.parse(String(init.body)); return json(state); }
    return json(body);
  } }));
  const p = await api.preview("alice", "channel", "message", () => true);
  await api.start("alice", "channel", "message", p, "ignored", () => true);
  assert.deepEqual(posted, { preview_binding: binding, allow_shared_location: true });
  body = { ...preview, radius_m: 50000 };
  await assert.rejects(api.preview("alice", "channel", "message", () => true));
  body = { ...preview, state: { ...state, choices: [] } };
  await assert.rejects(api.preview("alice", "channel", "message", () => true));
});

test("late preview cannot cross an account or channel fence", async () => {
  let release: ((response: Response) => void) | undefined;
  let admitted = true;
  const api = new AgentGeoapifyRollApi(new HttpClient({ fetch: () => new Promise(resolve => { release = resolve; }) }));
  const pending = api.preview("alice", "channel", "message", () => admitted);
  admitted = false; release!(json(preview));
  await assert.rejects(pending, /endra/);
});
