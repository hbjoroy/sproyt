import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { UserProfile } from "../../types";
import { MessageProfileStatus, PreviewProfile, type PreviewSettingsHost } from "./preview-settings";

const profile = (earlyAdopter: boolean): UserProfile => ({
  id: "user-1", kind: "human", display_name: "Ada", handle: "ada",
  external_provider: null, external_subject: null, created_at: "2026-09-18T08:00:00Z",
  status_text: "", status_emoji: "", status_expires_at: null, early_adopter: earlyAdopter
});

function host(user: UserProfile): PreviewSettingsHost {
  return {
    profile: () => user,
    profileFor: () => user,
    async saveName() {}, async saveStatus() {}, async saveNotifications() {}, async enablePush() {},
    async loadNotifications() { throw new Error("unused"); }
  };
}

test("profile restores the first-50 badge only for eligible members", () => {
  const marked = renderToStaticMarkup(createElement(PreviewProfile, { host: host(profile(true)) }));
  const ordinary = renderToStaticMarkup(createElement(PreviewProfile, { host: host(profile(false)) }));
  assert.ok(marked.includes("✨"));
  assert.ok(marked.includes("Første 50 på Sprøyt"));
  assert.ok(!ordinary.includes("Første 50 på Sprøyt"));
});

test("message status offers editing only for its owner, including members without a badge", () => {
  const user = { ...profile(false), status_text: "På tur", status_emoji: "🥾" };
  const own = renderToStaticMarkup(createElement(MessageProfileStatus, { host: host(user), userId: user.id, own: true }));
  const other = renderToStaticMarkup(createElement(MessageProfileStatus, { host: host(user), userId: user.id, own: false }));
  assert.ok(own.includes('aria-label="Endre status: 🥾 På tur"'));
  assert.ok(own.includes("<button"));
  assert.ok(other.includes("🥾 På tur"));
  assert.ok(!other.includes("<button"));
});
