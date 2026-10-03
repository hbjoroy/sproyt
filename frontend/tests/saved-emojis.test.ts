import assert from "node:assert/strict";
import test from "node:test";
import { HttpClient } from "../src/api";
import { SavedEmojiApi, pastedEmoji } from "../src/saved-emojis";

test("personal emoji API uses actor-aware per-item operations and rejects malformed collection replies", async () => {
  const calls: { url: string; method: string; body: unknown }[] = [];
  const api = new SavedEmojiApi(new HttpClient({ participant: () => "owner", fetch: async (url, init) => {
    calls.push({ url: String(url), method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : null });
    return init?.method ? new Response(null, { status: 204 }) : Response.json(["🧑🏽‍🚀", "🇬🇷"]);
  } }));
  assert.deepEqual(await api.list(), ["🧑🏽‍🚀", "🇬🇷"]);
  await api.save("🧑🏽‍🚀"); await api.save("🇬🇷", false);
  assert.deepEqual(calls, [
    { url: "/api/v1/me/emojis?participant=owner", method: "GET", body: null },
    { url: "/api/v1/me/emojis?participant=owner", method: "POST", body: { emoji: "🧑🏽‍🚀" } },
    { url: "/api/v1/me/emojis?participant=owner", method: "DELETE", body: { emoji: "🇬🇷" } }
  ]);
  const invalid = new SavedEmojiApi(new HttpClient({ fetch: async () => Response.json(["😀", 2]) }));
  await assert.rejects(invalid.list(), /Ugyldig emoji-liste/);
});

test("automatic paste saving recognizes whole complex emoji without collecting ordinary pasted text", () => {
  for (const value of ["🧑🏽‍🚀", "🇬🇷", "1️⃣", " 🫶 "]) assert.equal(pastedEmoji(value), value.trim());
  for (const value of ["", "Hello 😀", "😀😀", "secret", "é"]) assert.equal(pastedEmoji(value), null);
});
