import assert from "node:assert/strict";
import test from "node:test";
import { createDeviceInstall } from "../src/device-install";

function environment(ios = false, standalone = false) {
  const events = new EventTarget();
  const queries = new Map<string, EventTarget & { matches: boolean }>();
  const fake = Object.assign(events, {
    navigator: { userAgent: ios ? "iPhone" : "Chrome", platform: ios ? "iPhone" : "Win32", maxTouchPoints: 0, standalone },
    matchMedia(query: string) {
      let media = queries.get(query);
      if (!media) { media = Object.assign(new EventTarget(), { matches: false }); queries.set(query, media); }
      return media;
    }
  });
  const store = createDeviceInstall(fake as unknown as NonNullable<Parameters<typeof createDeviceInstall>[0]>);
  return { events, queries, store };
}

test("native installation is advertised only after the browser offers it and requires a user action", async () => {
  const { events, store } = environment();
  const changes: (string | null)[] = [];
  store.subscribe(() => changes.push(store.getSnapshot()));
  assert.equal(store.getSnapshot(), null);
  let invoked = 0;
  const event = Object.assign(new Event("beforeinstallprompt", { cancelable: true }), { prompt: async () => { invoked++; } });
  events.dispatchEvent(event);
  assert.equal(event.defaultPrevented, true);
  assert.equal(store.getSnapshot(), "native");
  assert.equal(invoked, 0);
  await Promise.all([store.install(), store.install()]);
  assert.equal(invoked, 1);
  assert.equal(store.getSnapshot(), null);
  assert.deepEqual(changes, ["native", null]);
  store.dispose();
});

test("installation and display-mode changes suppress prompts; iOS gives manual guidance only in a browser", () => {
  const { events, queries, store } = environment(true);
  assert.equal(store.getSnapshot(), "ios");
  const standalone = queries.get("(display-mode: standalone)")!;
  standalone.matches = true;
  standalone.dispatchEvent(new Event("change"));
  assert.equal(store.getSnapshot(), null);
  standalone.matches = false;
  standalone.dispatchEvent(new Event("change"));
  assert.equal(store.getSnapshot(), "ios");
  events.dispatchEvent(new Event("appinstalled"));
  assert.equal(store.getSnapshot(), null);
  store.dispose();
});

test("install events that do not contain an install prompt do not create a false option", () => {
  const { events, store } = environment();
  events.dispatchEvent(new Event("beforeinstallprompt", { cancelable: true }));
  assert.equal(store.getSnapshot(), null);
  store.dispose();
});

test("an iOS home-screen app is not offered installation again", () => {
  const { store } = environment(true, true);
  assert.equal(store.getSnapshot(), null);
  store.dispose();
});
