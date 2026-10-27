import assert from "node:assert/strict";
import test from "node:test";
import { createThreadSummaryState } from "../src/application/thread-summary-state";
import type { ChatMessage, ThreadSummary } from "../src/types";

const summary = (count: number, unread: number, latest: number, root = "root"): ThreadSummary =>
  ({ root_message_id: root, reply_count: count, unread_count: unread, latest_sequence: latest });
const reply = (sequence: number, sender: string, root = "root"): ChatMessage => ({ id: `reply-${sequence}`,
  channel_id: "channel", parent_message_id: root, sender_id: sender, sender_display_name: sender,
  body: "svar", sequence, sent_at: "2026-10-27T00:00:00Z", edited_at: null, deleted_at: null });

test("live own and other replies are rebased on a delayed channel list without duplication", () => {
  const state = createThreadSummaryState();
  const own = reply(20, "me"), other = reply(21, "peer");
  state.beginList("list"); state.addReply(own, "me"); state.addReply(other, "me");
  assert.deepEqual(state.summaries.get("root"), summary(2, 1, 21));
  assert.equal(state.receiveList("list", [summary(5, 2, 19)], () => [own, other], "me"), "applied");
  assert.deepEqual(state.summaries.get("root"), summary(7, 3, 21));
  state.beginList("again");
  assert.equal(state.receiveList("again", [summary(7, 3, 21)], () => [own, other], "me"), "applied");
  state.addReply(other, "me"); // A late duplicate live event was already in the snapshot.
  assert.deepEqual(state.summaries.get("root"), summary(7, 3, 21));
});

test("first reply arriving during a list is retained even if the snapshot has no root", () => {
  const state = createThreadSummaryState();
  state.beginList("list"); const message = reply(4, "peer", "new-root"); state.addReply(message, "me");
  assert.equal(state.receiveList("list", [], () => [], "me"), "applied");
  assert.deepEqual(state.summaries.get("new-root"), summary(1, 1, 4, "new-root"));
  state.beginList("next");
  assert.equal(state.receiveList("next", [], () => [], "me"), "applied");
  assert.equal(state.summaries.size, 0);
});

test("a delayed read confirmation preserves newer unread replies and ignores older confirmations", () => {
  const state = createThreadSummaryState();
  state.beginList("initial"); state.receiveList("initial", [summary(5, 2, 19)], () => [], "me");
  state.trackRead("old-read", "root", 19);
  const other = reply(21, "peer"); state.addReply(other, "me");
  assert.equal(state.receiveRead("old-read", summary(5, 0, 19), [other], "me"), "applied");
  assert.deepEqual(state.summaries.get("root"), summary(6, 1, 21));
  state.trackRead("newer", "root", 21); state.trackRead("older", "root", 20);
  assert.equal(state.receiveRead("newer", summary(6, 0, 21), [other], "me"), "applied");
  assert.equal(state.receiveRead("older", summary(5, 2, 19), [other], "me"), "stale");
  assert.deepEqual(state.summaries.get("root"), summary(6, 0, 21));
});

test("a list started before a read confirmation must be retried, preserving the confirmed state", () => {
  const state = createThreadSummaryState();
  state.beginList("initial"); state.receiveList("initial", [summary(5, 2, 19)], () => [], "me");
  state.beginList("stale"); state.trackRead("read", "root", 19);
  assert.equal(state.receiveRead("read", summary(5, 0, 19), [], "me"), "applied");
  assert.equal(state.receiveList("stale", [summary(5, 2, 19)], () => [], "me"), "retry");
  assert.deepEqual(state.summaries.get("root"), summary(5, 0, 19));
  state.beginList("retry");
  assert.equal(state.receiveList("retry", [summary(5, 0, 19)], () => [], "me"), "applied");
});

test("scope reset, disconnect, failed requests and mismatched roots fence late responses", () => {
  const state = createThreadSummaryState();
  state.beginList("account-a"); state.trackRead("read-a", "root", 9);
  state.reset(); state.beginList("account-b");
  assert.equal(state.receiveList("account-a", [summary(2, 2, 9)], () => [], "other"), "ignored");
  assert.equal(state.receiveRead("read-a", summary(2, 0, 9), [], "other"), "ignored");
  assert.equal(state.receiveList("account-b", [], () => [], "other"), "applied");
  state.trackRead("bad-root", "root", 9);
  assert.equal(state.receiveRead("bad-root", summary(2, 0, 9, "another-root"), [], "other"), "ignored");
  state.beginList("disconnected"); state.trackRead("lost", "root", 9); state.disconnect();
  assert.equal(state.receiveList("disconnected", [summary(2, 2, 9)], () => [], "other"), "ignored");
  assert.equal(state.receiveRead("lost", summary(2, 0, 9), [], "other"), "ignored");
  state.beginList("failed"); state.fail("failed");
  assert.equal(state.receiveList("failed", [summary(2, 2, 9)], () => [], "other"), "ignored");
});


test("out-of-order live replies are counted once and survive a stale thread load", () => {
  const state = createThreadSummaryState();
  state.beginList("initial"); state.receiveList("initial", [summary(2, 1, 10)], () => [], "me");
  state.addReply(reply(14, "peer"), "me"); state.addReply(reply(12, "me"), "me"); state.addReply(reply(14, "peer"), "me");
  assert.deepEqual(state.summaries.get("root"), summary(4, 2, 14));
  state.beginList("delayed"); state.receiveList("delayed", [summary(2, 1, 10)], () => [], "me");
  assert.deepEqual(state.summaries.get("root"), summary(4, 2, 14));
});

test("deletion fences an older list and read response without resurrecting the reply", () => {
  const state = createThreadSummaryState();
  state.beginList("initial"); state.receiveList("initial", [summary(1, 1, 10)], () => [], "me");
  state.beginList("old-list"); state.trackRead("old-read", "root", 10);
  state.deleteReply({ ...reply(10, "peer"), deleted_at: "2026-10-27T00:10:00Z" });
  assert.equal(state.receiveList("old-list", [summary(1, 1, 10)], () => [], "me"), "retry");
  assert.equal(state.receiveRead("old-read", summary(1, 0, 10), [], "me"), "retry");
  state.beginList("new"); state.receiveList("new", [summary(0, 0, 10)], () => [reply(10, "peer")], "me");
  state.addReply(reply(10, "peer"), "me");
  assert.deepEqual(state.summaries.get("root"), summary(0, 0, 10));
});

test("an older acknowledgement after a newer list is confirmed but requires a refresh", () => {
  const state = createThreadSummaryState();
  state.beginList("first"); state.receiveList("first", [summary(1, 1, 10)], () => [], "me");
  state.trackRead("ack", "root", 10);
  state.beginList("newer"); state.receiveList("newer", [summary(3, 3, 20)], () => [], "me");
  assert.equal(state.receiveRead("ack", summary(1, 0, 10), [], "me"), "retry");
  assert.deepEqual(state.summaries.get("root"), summary(3, 3, 20));
});
