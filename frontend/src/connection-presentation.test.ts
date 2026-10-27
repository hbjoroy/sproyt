import assert from "node:assert/strict";
import test from "node:test";
import { connectionPresentation, createManualConnectionRecovery } from "./connection-presentation";
import type { ConnectionSnapshot } from "./connection";

const connection = (patch: Partial<ConnectionSnapshot> = {}): ConnectionSnapshot => ({ connected: true, closing: false,
  handoffActive: false, subscribedChannelId: "channel", transport: "websocket", ...patch });
const options = { online: true, authenticationRequired: false, retrying: false, detail: "Fornyar økta …" };

test("a healthy SSE session stays connected during background WS handoff and session status changes", () => {
  assert.equal(connectionPresentation(connection({ transport: "sse", handoffActive: true }), options).phase, "connected");
  assert.equal(connectionPresentation(connection(), { ...options, detail: "Kunne ikkje sende meldinga" }).phase, "connected");
});
test("offline, actual connect work and authentication have distinct actions regardless of status wording", () => {
  assert.equal(connectionPresentation(connection(), { ...options, online: false }).phase, "offline");
  assert.equal(connectionPresentation(connection(), { ...options, authenticationRequired: true }).phase, "authentication");
  assert.equal(connectionPresentation(connection({ connected: false, connecting: true }), { ...options, detail: "Tilkopla" }).phase, "connecting");
  assert.equal(connectionPresentation(connection({ connected: false }), options).phase, "offline");
});
test("repeated manual attempts share one existing recovery and stop at the login boundary", async () => {
  let calls = 0, finish!: () => void, login = false;
  const pending: boolean[] = [];
  const retry = createManualConnectionRecovery({ authenticationRequired: () => login,
    recover: () => { calls++; return new Promise<void>(resolve => { finish = resolve; }); }, onPending: value => pending.push(value) });
  const first = retry(), second = retry();
  assert.equal(first, second);
  await Promise.resolve(); assert.equal(calls, 1); finish(); await first;
  assert.deepEqual(pending, [true, false]);
  login = true; await retry(); assert.equal(calls, 1);
});
