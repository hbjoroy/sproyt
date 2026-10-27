import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "../src/api";
import { AgentPlaceLookupApi, decodeAgentPlaceLookup } from "../src/agent-place-lookup";

const place = { id: "ChIJ-test-1", attribution: { title: "Original source title", url: "https://www.google.com/search?q=bar" },
  googleMapsLinks: { placeUrl: "https://www.google.com/maps/place/test" } };
const good = () => ({ search: { summary: "Original text [0]\nAnother line", places: [place] }, walking: null, route_tokens: [] });
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });

test("preview reveals exactly what would be sent and rejects inconsistent location disclosure", async () => {
  let body: unknown = { query: "Bar nær Nørreport", origin_mode: "named", named_origin: "Nørreport", uses_shared_location: false, preview_binding: "a".repeat(64) };
  const api = new AgentPlaceLookupApi(new HttpClient({ fetch: async () => json(body) }));
  assert.deepEqual(await api.preview("alice", "channel", "message", () => true), {
    query: "Bar nær Nørreport", originMode: "named", namedOrigin: "Nørreport", usesSharedLocation: false, binding: "a".repeat(64)
  });
  body = { query: "Bars", origin_mode: "shared", named_origin: null, uses_shared_location: false, preview_binding: "a".repeat(64) };
  await assert.rejects(api.preview("alice", "channel", "message", () => true));
});

test("walking route is user requested, preserves attribution and warning, and rejects unsafe sources", async () => {
  let url = "", request: RequestInit = {};
  let attribution = place.attribution;
  const api = new AgentPlaceLookupApi(new HttpClient({ fetch: async (target, init) => {
    url = String(target); request = init ?? {};
    return json({ route: { routes: [{ distanceMeters: 1450, duration: "601s", attribution }] }, warning: "Gangruter er i beta." });
  } }));
  const walking = await api.route("alice", "channel", "message", "opaque", () => true);
  assert.equal(url, "/api/v1/channels/channel/messages/message/place-route");
  assert.deepEqual(JSON.parse(String(request.body)), { token: "opaque", allow_shared_location: true });
  assert.equal(walking.distanceText, "1,5 km");
  assert.equal(walking.durationText, "11 min");
  assert.deepEqual(walking.sources, [place.attribution]);
  assert.equal(walking.warning, "Gangruter er i beta.");
  attribution = { title: "bad", url: "https://evil.test" };
  await assert.rejects(api.route("alice", "channel", "message", "opaque", () => true));
});

test("decoder preserves Google text and attribution, keeps the place URL separate", () => {
  const result = decodeAgentPlaceLookup(good());
  assert.equal(result.state.kind, "ready");
  if (result.state.kind !== "ready") throw new Error("expected ready");
  assert.equal(result.state.summary, "Original text [0]\nAnother line");
  assert.deepEqual(result.state.sources, [{ title: "Original source title", url: "https://www.google.com/search?q=bar" }]);
  assert.deepEqual(result.places, [{ title: "Original source title", attributionUrl: "https://www.google.com/search?q=bar", placeUrl: "https://www.google.com/maps/place/test" }]);
  assert.equal(result.walkingAvailable, false);
  assert.equal(decodeAgentPlaceLookup({ search: { summary: "", places: [] }, walking: null, route_tokens: [] }).state.kind, "empty");
});

test("decoder fails closed on incomplete, inconsistent or unsafe provider data", () => {
  const invalid = [
    {}, { search: { summary: "", places: [] } }, { search: { summary: "No bars", places: [] }, walking: null, route_tokens: [] },
    { search: { summary: "", places: [place] }, walking: null, route_tokens: [] },
    { search: { summary: "Somewhere", places: [{ ...place, attribution: { title: "Unknown", url: "https://evil.test/" } }] }, walking: null, route_tokens: [] },
    { search: { summary: "Somewhere", places: [{ ...place, googleMapsLinks: { placeUrl: "javascript:alert(1)" } }] }, walking: null, route_tokens: [] },
    { ...good(), walking: {} },
    { ...good(), walking: { routes: [{ distanceMeters: -10, duration: "10s", attribution: place.attribution }] } }
  ];
  for (const value of invalid) assert.throws(() => decodeAgentPlaceLookup(value));
  const result = decodeAgentPlaceLookup({ ...good(), walking: { routes: [{ distanceMeters: 1230, duration: "600s", attribution: place.attribution }] } });
  assert.equal(result.walkingAvailable, true);
  assert.equal(result.state.kind, "ready");
  if (result.state.kind === "ready") assert.equal(result.state.walking, undefined, "route must not be guessed or displayed yet");
});

test("HTTP requests are account fenced, bounded, encoded and use no-store", async () => {
  const seen: Array<{ url: string; init: RequestInit }> = [];
  const http = new HttpClient({ fetch: async (url, init) => {
    seen.push({ url: String(url), init: init ?? {} });
    return String(url).endsWith("/place-lookups") ? json({ message_ids: ["message/1"] }) : json(good());
  } });
  const api = new AgentPlaceLookupApi(http);
  const signal = new AbortController().signal;
  assert.deepEqual(await api.list("alice", "channel/1", ["message/1", "message-2"], () => true, signal), { messageIds: ["message/1"], googleMessageIds: ["message/1"], geoapifyMessageIds: [] });
  const result = await api.lookup("alice", "channel/1", "message/1", true, "a".repeat(64), () => true, signal);
  assert.equal(result.state.kind, "ready");
  assert.equal(seen[0]?.url, "/api/v1/channels/channel%2F1/place-lookups");
  assert.equal(seen[1]?.url, "/api/v1/channels/channel%2F1/messages/message%2F1/place-lookup");
  assert.deepEqual(JSON.parse(String(seen[0]?.init.body)), { message_ids: ["message/1", "message-2"] });
  assert.deepEqual(JSON.parse(String(seen[1]?.init.body)), { allow_shared_location: true, preview_binding: "a".repeat(64) });
  for (const call of seen) {
    assert.equal(call.init.method, "POST");
    assert.equal(call.init.cache, "no-store");
    assert.equal(new Headers(call.init.headers).get("x-sproyt-account"), "alice");
    assert.equal(call.init.signal, signal);
  }
  assert.throws(() => api.list("alice", "channel", Array.from({ length: 31 }, (_, n) => `${n}`), () => true));
  assert.throws(() => api.list("alice", "channel", ["same", "same"], () => true));
});

test("discovery rejects unsolicited or duplicate message IDs", async () => {
  for (const ids of [["unknown"], ["known", "known"]]) {
    const api = new AgentPlaceLookupApi(new HttpClient({ fetch: async () => json({ message_ids: ids }) }));
    await assert.rejects(api.list("alice", "channel", ["known", "other"], () => true));
  }
  const api = new AgentPlaceLookupApi(new HttpClient({ fetch: async () => json({ message_ids: ["known"], geoapify_message_ids: ["other"] }) }));
  await assert.rejects(api.list("alice", "channel", ["known", "other"], () => true));
  const googleApi = new AgentPlaceLookupApi(new HttpClient({ fetch: async () => json({ message_ids: ["known"], google_message_ids: ["other"] }) }));
  await assert.rejects(googleApi.list("alice", "channel", ["known", "other"], () => true));
});

test("late result and retries cannot cross account/channel or abort boundaries", async () => {
  let release: ((response: Response) => void) | undefined;
  let permitted = true;
  const api = new AgentPlaceLookupApi(new HttpClient({ fetch: () => new Promise(resolve => { release = resolve; }) }));
  const pending = api.lookup("alice", "channel", "message", false, "a".repeat(64), () => permitted);
  permitted = false;
  release!(json(good()));
  await assert.rejects(pending, /endra/);
  permitted = true;
  const controller = new AbortController();
  const aborted = api.lookup("alice", "channel", "message", false, "a".repeat(64), () => permitted, controller.signal);
  controller.abort();
  release!(json(good()));
  await assert.rejects(aborted, { name: "AbortError" });

  let calls = 0;
  const fenced = new AgentPlaceLookupApi(new HttpClient({ fetch: async () => { calls++; return new Response("", { status: 401 }); },
    refreshSession: async () => { permitted = false; return true; } }));
  await assert.rejects(fenced.lookup("alice", "channel", "message", false, "a".repeat(64), () => permitted), /endra/);
  assert.equal(calls, 1, "must not resend after the account/channel fence closes");
});
