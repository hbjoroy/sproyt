import assert from "node:assert/strict";
import test from "node:test";
import { approximateCount, inboxCounts } from "../src/ui/react/inbox-counts";

test("inbox mention and task counts stay independent", () => {
  assert.deepEqual(inboxCounts([{ read: false }, { read: true }, { read: false }], [{ status: "open" }, { status: "done" }]), { mentionCount: 2, taskCount: 1 });
  assert.deepEqual(inboxCounts([], [{ status: "open" }]), { mentionCount: 0, taskCount: 1 });
  assert.equal(approximateCount(24), "24");
  assert.equal(approximateCount(25), "25+");
  assert.equal(approximateCount(50), "50+");
  assert.equal(approximateCount(100), "100+");
});
