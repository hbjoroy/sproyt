import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "./api";
import { CircleFavorites, ProfileCircleFavoritesApi, type CircleFavoritesApi } from "./circle-favorites";
const circle = "00000000-0000-4000-8000-000000000001";
const other = "00000000-0000-4000-8000-000000000002";
const deferred = <T>() => { let resolve!: (value: T) => void; const promise = new Promise<T>(done => { resolve = done; }); return { promise, resolve }; };
async function ready(store: CircleFavorites) {
  for (let n = 0; n < 30 && (!store.getSnapshot().loaded || store.getSnapshot().loading || store.getSnapshot().pending.length); n++) await new Promise(done => setTimeout(done, 0));
  assert.equal(store.getSnapshot().loaded, true); assert.equal(store.getSnapshot().loading, false); assert.equal(store.getSnapshot().pending.length, 0);
}
function server() {
  const data = new Map<string, Set<string>>();
  const calls: string[] = [];
  const api: CircleFavoritesApi = {
    async list(owner) { return [...(data.get(owner) ?? [])]; },
    async set(owner, id, pinned, admitted) { assert.ok(admitted()); calls.push(`${owner}:${id}:${pinned}`); const ids = data.get(owner) ?? new Set(); pinned ? ids.add(id) : ids.delete(id); data.set(owner, ids); }
  };
  return { api, data, calls };
}
test("profile favorites survive a fresh client and preserve changes from another device", async () => {
  const { api } = server();
  const first = new CircleFavorites(api, () => "alice"); first.bindOwner("alice"); await ready(first);
  const second = new CircleFavorites(api, () => "alice"); second.bindOwner("alice"); await ready(second);
  await first.set(circle, true); await second.set(other, true); await first.refresh();
  assert.deepEqual(new Set(first.getSnapshot().ids), new Set([circle, other]));
  const fresh = new CircleFavorites(api, () => "alice"); fresh.bindOwner("alice"); await ready(fresh);
  assert.deepEqual(new Set(fresh.getSnapshot().ids), new Set([circle, other]));
  await first.set(circle, false); await second.refresh(); assert.deepEqual(second.getSnapshot().ids, [other]);
});
test("a GET started before a mutation cannot erase its confirmed result", async () => {
  const { api } = server(); const store = new CircleFavorites(api, () => "alice"); store.bindOwner("alice"); await ready(store);
  const old = deferred<readonly string[]>(); const originalList = api.list;
  let stale = true; api.list = (owner, admitted) => stale ? (stale = false, old.promise) : originalList(owner, admitted);
  const refresh = store.refresh(); await store.set(circle, true);
  assert.deepEqual(store.getSnapshot().ids, [circle]); old.resolve([]); await refresh;
  assert.deepEqual(store.getSnapshot().ids, [circle]);
});
test("account changes fence late results and queued writes without blocking the new account", async () => {
  const { api, calls, data } = server(); let owner = "alice";
  const store = new CircleFavorites(api, () => owner); store.bindOwner(owner); await ready(store);
  const hold = deferred<void>(); const original = api.set; let entered = false;
  api.set = async (...args) => { if (args[0] === "alice") { entered = true; await hold.promise; } if (args[3]()) await original(...args); };
  const first = store.set(circle, true), queued = store.set(other, true);
  for (let n = 0; n < 5 && !entered; n++) await Promise.resolve(); assert.ok(entered);
  owner = "bob"; data.set(owner, new Set([other])); store.bindOwner(owner); await ready(store);
  await store.set(circle, true); hold.resolve(); await Promise.all([first, queued]);
  assert.deepEqual(new Set(store.getSnapshot().ids), new Set([circle, other]));
  assert.ok(calls.every(call => call.startsWith("bob:"))); assert.equal(store.getSnapshot().ownerId, "bob");
});
test("failed saves stay unconfirmed, preserve other favorites and expose an explicit retry", async () => {
  const { api, data } = server(); data.set("alice", new Set([other])); let fail = true; const set = api.set;
  api.set = async (...args) => { if (fail) { fail = false; throw new Error("Offline"); } await set(...args); };
  const store = new CircleFavorites(api, () => "alice"); store.bindOwner("alice"); await ready(store);
  await store.set(circle, true); assert.deepEqual(store.getSnapshot().ids, [other]); assert.equal(store.getSnapshot().error, "Offline");
  store.retry(); await ready(store); assert.deepEqual(new Set(store.getSnapshot().ids), new Set([circle, other])); assert.equal(store.getSnapshot().error, "");
});
test("session renewal cannot retry a favorite write under another account", async () => {
  let owner = "alice", requests = 0;
  const http = new HttpClient({ fetch: async (_url, init) => { requests++; assert.equal(new Headers(init?.headers).get("x-sproyt-account"), "alice"); return new Response("", { status: 401 }); }, refreshSession: async () => { owner = "bob"; return true; } });
  await assert.rejects(new ProfileCircleFavoritesApi(http).set("alice", circle, true, () => owner === "alice"), /Innlogginga vart endra/);
  assert.equal(requests, 1); assert.equal(http.pendingWrites, 0);
});

test("a legacy transfer remains bound to its original account for the entire batch", async () => {
  const { api, calls } = server(); let owner = "alice";
  const store = new CircleFavorites(api, () => owner); store.bindOwner(owner); await ready(store);
  const hold = deferred<void>(); let entered = false; const original = api.set;
  api.set = async (...args) => { if (args[0] === "alice") { entered = true; await hold.promise; } if (args[3]()) await original(...args); };
  const transfer = store.transfer("alice", [circle, other]);
  for (let n = 0; n < 5 && !entered; n++) await Promise.resolve(); assert.ok(entered);
  owner = "bob"; store.bindOwner(owner); await ready(store);
  hold.resolve(); await transfer;
  assert.deepEqual(store.getSnapshot().ids, []); assert.deepEqual(calls, []);
  await store.set(circle, true, "alice"); assert.deepEqual(calls, []);
});
