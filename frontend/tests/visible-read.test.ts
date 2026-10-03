import assert from "node:assert/strict";
import test from "node:test";
import { createVisibleReadPolicy } from "../src/application/visible-read";

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
