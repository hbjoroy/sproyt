import assert from "node:assert/strict";
import test from "node:test";
import { devicePushStatus } from "../src/device-push";
import type { NotificationSettings } from "../src/api";

const active: NotificationSettings = { enabled: true, publicKey: "key", subscriptions: 1, channelIds: [],
  preferences: { mode: "instant", directMessages: true, mentions: true } };
function context(overrides: Partial<Parameters<typeof devicePushStatus>[0]> = {}): Parameters<typeof devicePushStatus>[0] {
  return { supported: true, permission: "granted", settings: async () => active,
    subscription: async () => ({ endpoint: "https://push.example/current" }), registered: async () => true,
    admitted: () => true, ...overrides };
}

test("device push reminders distinguish browser permission, actual subscription and current account", async () => {
  assert.equal(await devicePushStatus(context({ permission: "default" })), "needs-setup");
  assert.equal(await devicePushStatus(context({ permission: "denied" })), "blocked");
  assert.equal(await devicePushStatus(context({ subscription: async () => null })), "needs-setup");
  assert.equal(await devicePushStatus(context({ registered: async () => false })), "needs-setup");
  assert.equal(await devicePushStatus(context()), "none");
});

test("device push reminders stay quiet when unsupported, muted or status cannot be determined", async () => {
  assert.equal(await devicePushStatus(context({ supported: false })), "none");
  assert.equal(await devicePushStatus(context({ settings: async () => ({ ...active, enabled: false }) })), "none");
  assert.equal(await devicePushStatus(context({ settings: async () => ({ ...active, preferences: { ...active.preferences, mode: "muted" } }) })), "none");
  assert.equal(await devicePushStatus(context({ subscription: async () => { throw new Error("offline"); } })), "none");
  assert.equal(await devicePushStatus(context({ registered: async () => { throw new Error("offline"); } })), "none");
});

test("device push checks stop after an account change and do not query another account", async () => {
  let admitted = true, checked = false;
  assert.equal(await devicePushStatus(context({ admitted: () => admitted,
    subscription: async () => { admitted = false; return { endpoint: "https://push.example/current" }; },
    registered: async () => { checked = true; return false; }
  })), "none");
  assert.equal(checked, false);
});
