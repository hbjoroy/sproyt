import assert from "node:assert/strict";
import test from "node:test";
import { createVisibleReadPolicy, readSequencePastDeleted } from "../src/application/visible-read";
import type { ChatMessage } from "../src/types";

test("visible endpoint passes only contiguous deleted sequences and stops at unknown or live replies", () => {
  const message = (sequence: number, deleted = true, parent: string | null = null): ChatMessage => ({
    id: String(sequence), sequence, channel_id: "a", parent_message_id: parent, sender_id: "u", sender_display_name: "U",
    body: "", sent_at: "2025-01-01T00:00:00Z", edited_at: null, deleted_at: deleted ? "2025-02-01T00:00:00Z" : null
  });
  assert.equal(readSequencePastDeleted(1, "a", [message(2), message(3)]), 3);
  assert.equal(readSequencePastDeleted(1, "a", [message(2), message(3, false, "root"), message(4)]), 2);
  assert.equal(readSequencePastDeleted(1, "a", [message(3)]), 1);
  assert.equal(readSequencePastDeleted(0, "a", [message(1), message(2)]), 2);
  assert.equal(readSequencePastDeleted(0, "a", [message(2)]), 0);
});

test("visible read progress retries failed/lost sends and never regresses confirmed progress", () => {
  const policy = createVisibleReadPolicy();
  const sent: number[] = [];
  const send = (sequence: number, id: string) => () => { sent.push(sequence); return id; };
  policy.confirm("channel:a", 40);
  policy.acknowledge("channel:a", 43, send(43, "first"));
  policy.acknowledge("channel:a", 43, send(43, "duplicate"));
  assert.deepEqual(sent, [43]);
  policy.fail("first");
  policy.acknowledge("channel:a", 43, send(43, "retry"));
  policy.disconnect();
  policy.acknowledge("channel:a", 43, send(43, "after-reconnect"));
  policy.complete("after-reconnect");
  policy.confirm("channel:a", 10);
  policy.acknowledge("channel:a", 42, send(42, "stale"));
  policy.acknowledge("channel:b", 42, send(42, "other-channel"));
  policy.acknowledge("thread:root", 45, () => null);
  policy.acknowledge("thread:root", 45, send(45, "available"));
  assert.deepEqual(sent, [43, 43, 43, 42, 45]);
});
