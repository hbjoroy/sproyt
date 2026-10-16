import assert from "node:assert/strict";
import test from "node:test";
import type { HttpClient } from "../src/api";
import { createImageGeneration, fallbackImageDraft } from "../src/imagegen";

class ElementStub {
  hidden = false;
  textContent = "";
  className = "";
  append(..._children: unknown[]) {}
  before(..._children: unknown[]) {}
  setAttribute(_name: string, _value: string) {}
  addEventListener(_name: string, _listener: unknown) {}
  replaceChildren() {}
  querySelectorAll() { return []; }
}

test("imagegen decline restores original literal draft and thread only after successful review, including reload", async () => {
  const descriptors = new Map(["document", "window", "location", "sessionStorage"].map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  const storage = new Map<string, string>();
  const restoreCalls: unknown[][] = [];
  let job: Record<string, unknown> | null = null;
  let failReview = false;
  let identity = "person-1";
  let allowRestore = true;
  let reviewDelay: Promise<void> | null = null;
  const globals = {
    document: { createElement: () => new ElementStub() },
    window: { setInterval: () => 0 },
    location: { search: "" },
    sessionStorage: { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value), removeItem: (key: string) => storage.delete(key) }
  };
  for (const [key, value] of Object.entries(globals)) Object.defineProperty(globalThis, key, { configurable: true, value });
  const http = { request: async (path: string, init?: RequestInit) => {
    if (path.endsWith("/review")) {
      await reviewDelay;
      if (failReview) throw new Error("Offline");
      job = null;
      return Response.json({});
    }
    if (init?.method === "POST") {
      const body = JSON.parse(String(init.body));
      job = { id: "image-1", channel_id: body.channel_id, state: "ready", prompt: body.prompt, mode: body.mode };
      return Response.json({ job });
    }
    return Response.json({ jobs: job ? [job] : [] });
  } } as unknown as HttpClient;
  const owner = () => createImageGeneration({ http, before: new ElementStub() as unknown as HTMLElement,
    toolbar: new ElementStub() as unknown as HTMLElement, connected: () => true, channel: () => "channel-1", identity: () => identity,
    channelName: id => id, attach: () => { throw new Error("Decline must never attach"); },
    restorePrompt: (...args) => { restoreCalls.push(args); return allowRestore; } });
  try {
    const original = '/imagegen literal "Keep the sky. Change the sea."';
    await owner().submit(original, "channel-1", [], "root-1");
    const reloaded = owner();
    await reloaded.refresh();
    failReview = true;
    await reloaded.review("image-1", "decline");
    assert.equal(restoreCalls.length, 0);
    failReview = false;
    await reloaded.review("image-1", "decline");
    assert.deepEqual(restoreCalls, [[original, "channel-1", "root-1"]]);
    assert.match(reloaded.getSnapshot().status, /klar til å endrast/);
    const conflict = owner();
    await conflict.submit(original, "channel-1");
    allowRestore = false;
    await conflict.review("image-1", "decline");
    assert.equal(conflict.getSnapshot().rejectedDraft, original);
    conflict.editRejectedDraft("/imagegen A calmer sea");
    assert.equal(conflict.getSnapshot().rejectedDraft, "/imagegen A calmer sea");

    const accountChange = owner();
    await accountChange.submit(original, "channel-1");
    let finishReview!: () => void;
    reviewDelay = new Promise<void>(resolve => { finishReview = resolve; });
    const pendingReview = accountChange.review("image-1", "decline");
    identity = "person-2";
    finishReview();
    await pendingReview;
    assert.equal(restoreCalls.length, 2, "Old account's asynchronous decline must not restore into another account");
    assert.equal(accountChange.getSnapshot().rejectedDraft, null);
  } finally {
    for (const [key, descriptor] of descriptors) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});

test("older image jobs reconstruct an editable command without losing literal mode", () => {
  assert.equal(fallbackImageDraft({ prompt: "Sea", mode: "literal" }), '/imagegen literal "Sea"');
  assert.equal(fallbackImageDraft({ prompt: "literal scenery", mode: "expanded" }), '/imagegen "literal scenery"');
});
