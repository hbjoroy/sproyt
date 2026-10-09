import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "../src/api";
import { SavedStatusApi } from "../src/saved-statuses";

test("saved status API reads only the actor and deletes one combination without a profile write", async () => {
  const status = { text: "På tur", emoji: "🥾", save_count: 2, last_used_at: "2026-10-09T10:00:00Z" };
  const calls: unknown[] = [];
  const api = new SavedStatusApi(new HttpClient({ participant: () => "owner", fetch: async (url, init) => {
    calls.push([String(url), init?.method ?? "GET", init?.body ? JSON.parse(String(init.body)) : null]);
    return init?.method ? new Response(null, { status: 204 }) : Response.json([status]);
  } }));
  assert.deepEqual(await api.list(), [status]); await api.remove(status);
  assert.deepEqual(calls, [["/api/v1/me/statuses?participant=owner", "GET", null],
    ["/api/v1/me/statuses?participant=owner", "DELETE", { text: "På tur", emoji: "🥾" }]]);
  for (const value of [[{ ...status, save_count: 0 }], [{ ...status, last_used_at: "bad" }],
    [{ ...status, text: "", emoji: "" }], Array(21).fill(status), [{ ...status, text: "x".repeat(101) }]]) {
    const invalid = new SavedStatusApi(new HttpClient({ fetch: async () => Response.json(value) }));
    await assert.rejects(invalid.list(), /Ugyldig liste/);
  }
});
