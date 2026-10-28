import assert from "node:assert/strict";
import test from "node:test";
import { DIRECT_ACTIVE_WINDOW, rankDirectConversations, retainDirectOrder } from "./direct-activity";
import { asWireEvent, protocolId } from "./types";

const now = Date.parse("2026-10-28T12:00:00Z");
const day = 86_400_000;
const item = (id: string, age: number, score = 0, calculated = now) => ({ id,
  directActivity: { last_message_at: new Date(now - age).toISOString(), score, calculated_at: new Date(calculated).toISOString() } });
const ids = (items: readonly { id: string }[]) => items.map(item => item.id);

test("active DMs use last message before any frequency, old DMs use frequency then last message", () => {
  const input = [item("old-low", 2 * day, 1), item("frequent", 3 * day, 50), item("active", day, 99),
    item("newest", 1, 0), item("tie-new", 4 * day, 10), item("tie-old", 5 * day, 10), { id: "empty" }];
  assert.deepEqual(ids(rankDirectConversations(input, now)), ["newest", "active", "frequent", "tie-new", "tie-old", "old-low", "empty"]);
  assert.equal(input[0]!.id, "old-low", "does not mutate host data");
});

test("36-hour cutoff and 30-day decay use a shared clock, independent of device visits", () => {
  assert.deepEqual(ids(rankDirectConversations([item("outside", DIRECT_ACTIVE_WINDOW + 1, 100), item("edge", DIRECT_ACTIVE_WINDOW)], now)), ["edge", "outside"]);
  const input = [item("stale-score", 2 * day, 10, now - 30 * day), item("fresh-score", 3 * day, 6)];
  assert.deepEqual(ids(rankDirectConversations(input, now)), ["fresh-score", "stale-score"]);
  assert.deepEqual(ids(rankDirectConversations([...input].reverse(), now)), ["fresh-score", "stale-score"]);
});

test("empty histories have deterministic IDs and retaining interaction order drops removed rows", () => {
  const ranked = rankDirectConversations([{ id: "z" }, { id: "a" }], now);
  assert.deepEqual(ids(ranked), ["a", "z"]);
  assert.deepEqual(ids(retainDirectOrder(ranked, ["gone", "z"])), ["z", "a"]);
});

test("activity wire boundary accepts older pods and rejects invalid score or timestamps", () => {
  const channel = { id: "dm", slug: "dm", name: "Person", kind: "private", circle_id: null,
    direct_user_id: "person", is_direct: true, description: "", role: "member", last_read_sequence: 0, latest_sequence: 0 };
  const decode = (fields: object) => asWireEvent({ protocol: protocolId, type: "channels_listed", payload: { channels: [{ ...channel, ...fields }] } });
  assert.ok(decode({}));
  assert.ok(decode({ direct_activity: null }));
  const direct_activity = item("test", day, 1).directActivity;
  assert.ok(decode({ direct_activity }));
  for (const invalid of [{ score: -1 }, { score: Infinity }, { last_message_at: "invalid" }, { calculated_at: "invalid" }]) {
    assert.equal(decode({ direct_activity: { ...direct_activity, ...invalid } }), null);
  }
});

