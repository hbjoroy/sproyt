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


test("image acceptance hides only after confirmed attachment and preserves recovery across failures and scope changes", async () => {
  const descriptors = new Map(["document", "window", "location", "sessionStorage"].map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  const storage = new Map<string, string>([["sproyt-imagegen-scope:person-1:image-1", "root-1"]]);
  let identity = "person-1";
  let channel = "channel-1";
  let attached = false;
  let failReview = false;
  let mediaChannel = "channel-1";
  let reviewGate: Promise<void> = Promise.resolve();
  const calls: unknown[][] = [];
  const job = { id: "image-1", channel_id: "channel-1", state: "ready", prompt: "Sea", mode: "literal" };
  const globals = {
    document: { createElement: () => new ElementStub() }, window: { setInterval: () => 0 }, location: { search: "" },
    sessionStorage: { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value), removeItem: (key: string) => storage.delete(key) }
  };
  for (const [key, value] of Object.entries(globals)) Object.defineProperty(globalThis, key, { configurable: true, value });
  const http = { request: async (path: string) => {
    if (path.endsWith("/review")) {
      await reviewGate;
      if (failReview) throw new Error("Review unavailable");
      job.state = "accepted";
      return Response.json({ media: { id: "media-1", channel_id: mediaChannel, original_filename: "generated.png", content_type: "image/png" } });
    }
    return Response.json({ jobs: [job] });
  } } as unknown as HttpClient;
  const makeOwner = () => createImageGeneration({ http, before: new ElementStub() as unknown as HTMLElement,
    toolbar: new ElementStub() as unknown as HTMLElement, connected: () => true, channel: () => channel, identity: () => identity,
    channelName: id => id, attach: (...args) => { calls.push(args); return attached; }, restorePrompt: () => false });
  try {
    const owner = makeOwner();
    await owner.refresh();
    await owner.review(job.id, "accept");
    assert.equal(owner.getSnapshot().visible, true, "A successful HTTP review is insufficient if the target draft did not accept the media");
    assert.match(owner.getSnapshot().status, /kunne ikkje leggjast i utkastet/);
    assert.equal(calls[0]?.[1], "root-1");
    attached = true;
    await owner.review(job.id, "accept");
    assert.equal(owner.getSnapshot().visible, false);
    assert.match(owner.getSnapshot().status, /lagt i utkastet/);
    await owner.refresh();
    assert.equal(owner.getSnapshot().visible, false, "Polling must not reopen the accepted workshop");
    owner.open();
    assert.equal(owner.getSnapshot().visible, true);
    assert.equal(owner.getSnapshot().jobs[0]?.state, "accepted", "Hiding keeps the job available to reopen");
    await Promise.resolve();

    const failure = makeOwner();
    await failure.refresh();
    const attachedCount = calls.length;
    failReview = true;
    await failure.review(job.id, "accept");
    assert.equal(failure.getSnapshot().visible, true);
    assert.match(failure.getSnapshot().status, /Review unavailable/);
    failReview = false;
    mediaChannel = "foreign-channel";
    await failure.review(job.id, "accept");
    assert.equal(failure.getSnapshot().visible, true);
    assert.match(failure.getSnapshot().status, /Ugyldig biletvedlegg/);
    assert.equal(calls.length, attachedCount);
    mediaChannel = "channel-1";

    let release!: () => void;
    reviewGate = new Promise<void>(resolve => { release = resolve; });
    const delayed = failure.review(job.id, "accept");
    assert.equal(failure.getSnapshot().visible, true);
    assert.equal(failure.getSnapshot().busy, true);
    channel = "channel-2";
    release();
    await delayed;
    assert.equal(calls.length, attachedCount);
    assert.equal(failure.getSnapshot().visible, true);
    assert.match(failure.getSnapshot().status, /Opne den opphavlege kanalen/);

    channel = "channel-1";
    reviewGate = new Promise<void>(resolve => { release = resolve; });
    const changedAccount = failure.review(job.id, "accept");
    identity = "person-2";
    release();
    await changedAccount;
    assert.equal(calls.length, attachedCount, "A response for the old account must not attach or dismiss the new account's workshop");
  } finally {
    for (const [key, descriptor] of descriptors) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
