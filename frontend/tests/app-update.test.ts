import assert from "node:assert/strict";
import test from "node:test";
import { createAppUpdate, updateAppWorker, type UpdatePosition } from "../src/app-update";

class MemoryStorage implements Storage {
  values = new Map<string, string>();
  get length() { return this.values.size; }
  clear() { this.values.clear(); }
  getItem(key: string) { return this.values.get(key) ?? null; }
  key(index: number) { return [...this.values.keys()][index] ?? null; }
  removeItem(key: string) { this.values.delete(key); }
  setItem(key: string, value: string) { this.values.set(key, value); }
}
const anchor: UpdatePosition = { key: "channel:one", position: { anchorId: "message", anchorOffset: -12, distanceFromBottom: 500, sequence: 7 } };
const version = (name: string) => `/assets/app/${name}/app.js`;

test("manual update reloads once and verifies code on return without clearing drafts or journals", async () => {
  const storage = new MemoryStorage();
  storage.setItem("draft", "Utkast"); storage.setItem("protocol-journal", "original-request-id");
  let reloads = 0, checks = 0, worker = 0;
  const dependencies = { storage, latestVersion: async () => version("new"), updateWorker: async () => { worker++; },
    prepare: async () => { checks++; return { owner: "user", positions: [] }; }, reload: () => { reloads++; } };
  const old = createAppUpdate({ ...dependencies, currentVersion: version("old") });
  await old.run(() => [anchor]); await old.run();
  assert.equal(reloads, 1); assert.equal(checks, 2); assert.equal(worker, 1);
  const next = createAppUpdate({ ...dependencies, currentVersion: version("new") });
  assert.equal(next.getSnapshot().error, false); assert.match(next.getSnapshot().message, /oppdatert/);
  assert.deepEqual(next.resumePosition("user", "channel:one"), anchor.position);
  assert.equal(next.resumePosition("other-user", "channel:one"), null);
  assert.equal(reloads, 1); assert.equal(storage.getItem("draft"), "Utkast"); assert.equal(storage.getItem("protocol-journal"), "original-request-id");
});

test("same code refreshes data deliberately, mismatched code reports failure without a reload loop", async () => {
  const storage = new MemoryStorage(); let reloads = 0;
  const options = { storage, latestVersion: async () => version("same"), updateWorker: async () => {},
    prepare: async () => ({ owner: "user", positions: [] }), reload: () => { reloads++; } };
  await createAppUpdate({ ...options, currentVersion: version("same") }).run();
  assert.equal(reloads, 1);
  const wrong = createAppUpdate({ ...options, currentVersion: version("wrong") });
  assert.equal(wrong.getSnapshot().error, true); assert.equal(reloads, 1);
  const again = createAppUpdate({ ...options, currentVersion: version("wrong") });
  assert.equal(again.getSnapshot().message, ""); assert.equal(reloads, 1);
});

test("network failure retries explicitly and newly unsafe drafts stop navigation after the worker check", async () => {
  const storage = new MemoryStorage(); let attempts = 0, checks = 0, reloads = 0;
  const update = createAppUpdate({ storage, currentVersion: version("old"), latestVersion: async () => {
    if (++attempts === 1) throw new Error("Offline"); return version("new");
  }, updateWorker: async () => {}, prepare: async () => {
    if (++checks === 3) throw new Error("Vedlegg må sendast først"); return { owner: "user", positions: [] };
  }, reload: () => { reloads++; } });
  await update.run(); assert.equal(update.getSnapshot().busy, false); assert.equal(attempts, 1);
  await update.run(); assert.match(update.getSnapshot().message, /Vedlegg/); assert.equal(reloads, 0);
  await update.run(); assert.equal(reloads, 1);
});

test("unavailable resume storage blocks refresh and leaves existing storage untouched", async () => {
  const storage = new MemoryStorage(); let reloads = 0;
  storage.setItem("draft", "Keep"); storage.setItem = () => { throw new Error("Storage unavailable"); };
  const update = createAppUpdate({ storage, currentVersion: version("old"), latestVersion: async () => version("new"),
    updateWorker: async () => {}, prepare: async () => ({ owner: "user", positions: [] }), reload: () => { reloads++; } });
  await update.run(); assert.equal(reloads, 0); assert.equal(storage.getItem("draft"), "Keep"); assert.equal(update.getSnapshot().error, true);
});

test("worker activation timeout removes its listener before an explicit retry", async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  const originalTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  let expire: (() => void) | undefined;
  const listeners = new Set<EventListenerOrEventListenerObject>();
  const worker = { state: "installing", addEventListener: (_: string, listener: EventListenerOrEventListenerObject) => listeners.add(listener),
    removeEventListener: (_: string, listener: EventListenerOrEventListenerObject) => listeners.delete(listener) };
  const ready = Promise.resolve({ installing: worker, waiting: null, update: async () => {} } as unknown as ServiceWorkerRegistration);
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { serviceWorker: {} } });
  globalThis.setTimeout = ((callback: () => void) => { expire = callback; return 1; }) as typeof setTimeout;
  globalThis.clearTimeout = (() => { expire = undefined; }) as typeof clearTimeout;
  try {
    for (let retry = 0; retry < 2; retry++) {
      const update = updateAppWorker(ready);
      const rejection = assert.rejects(update, /tok for lang tid/);
      await Promise.resolve(); await Promise.resolve();
      assert.equal(listeners.size, 1);
      assert.ok(expire); expire();
      await rejection;
      assert.equal(listeners.size, 0);
    }
  } finally {
    globalThis.setTimeout = originalTimeout;
    globalThis.clearTimeout = originalClearTimeout;
    if (original) Object.defineProperty(globalThis, "navigator", original);
    else delete (globalThis as { navigator?: Navigator }).navigator;
  }
});
