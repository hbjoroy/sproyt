import assert from "node:assert/strict";
import test from "node:test";
import { reactionChoices, rememberReaction } from "../src/reaction-choices";
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

test("pinned order, regular use across days and a recent choice share one personal row", () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  const data = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
  } });
  try {
    const day = 86_400_000;
    for (let n = 0; n < 4; n++) rememberReaction("A", "🚲", n * day);
    for (let n = 0; n < 40; n++) rememberReaction("A", "⚽", 5 * day + n);
    const row = (owner: string) => reactionChoices(owner, ["🛶", "🍻"], 6 * day).slice(0, 6).map(([emoji]) => emoji);
    assert.deepEqual(row("A").slice(0, 3), ["🛶", "🍻", "🚲"]);
    assert.equal(row("A")[5], "⚽");
    assert.ok(!row("B").includes("🚲") && !row("B").includes("⚽"));
  } finally {
    if (original) Object.defineProperty(globalThis, "localStorage", original);
    else Reflect.deleteProperty(globalThis, "localStorage");
  }
});
