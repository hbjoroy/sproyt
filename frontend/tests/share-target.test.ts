import assert from "node:assert/strict";
import test from "node:test";
import { createShareTarget, type ShareInbox, type ShareReceipt, type ShareAdmission } from "../src/share-target";

function fixture() {
  let user = "actor", generation = 0, failEnqueue = false, failIdentity = false, switchDuringUpload = false;
  let item: ShareReceipt = { id: "request", owner: user, generation, createdAt: 1, text: "Delt tekst", file: new File(["image"], "original.png", { type: "image/png" }), admission: null, done: null };
  const enqueued: ShareAdmission[] = [], dispatched: ShareAdmission[] = [];
  let uploads = 0;
  const inbox = {
    identity: async () => { if (failIdentity) throw new Error("Offline identity"); return user; }, generation: async () => generation,
    list: async (owner: string) => item.owner === owner ? [item] : [],
    claim: async () => item,
    edit: async (_: string, owner: string, revision: number, text: string, channelId: string) => { assert.equal(owner, item.owner); assert.equal(revision, generation); return item = { ...item, text, channelId }; },
    admit: async (_: string, owner: string, revision: number, admission: ShareAdmission) => {
      assert.equal(owner, item.owner); assert.equal(revision, generation);
      if (item.admission) assert.deepEqual(admission, item.admission);
      return item = { ...item, admission };
    },
    owned: async () => { item = { ...item, file: null }; },
    finish: async () => { item = { ...item, text: "", done: 1 }; },
    discard: async () => {}, logout: async () => { generation++; }
  } as unknown as ShareInbox;
  const make = () => createShareTarget({ user: () => user, upload: async () => { uploads++; if (switchDuringUpload) user = "other"; return { id: "media", contentType: "image/png", originalFilename: "original.png" }; },
    enqueue: async admission => { if (failEnqueue) throw new Error("Quota"); enqueued.push(admission); },
    dispatch: admission => { assert.equal(target.isShare(admission.requestId), true); dispatched.push(admission); }
  }, inbox);
  let target = make();
  return { get target() { return target; }, get item() { return item; }, get uploads() { return uploads; }, enqueued, dispatched,
    failEnqueue(value: boolean) { failEnqueue = value; }, failIdentity(value: boolean) { failIdentity = value; },
    switchDuringUpload() { switchDuringUpload = true; },
    switchUser() { user = "other"; target.clear(); }, reload() { target = make(); } };
}

test("share handoff freezes admission before enqueue and preserves the blob on quota failure; retry after reload is exact", async () => {
  const f = fixture(); await f.target.refresh(); f.failEnqueue(true);
  await f.target.send("request", "channel", "Redigert");
  assert.equal(f.dispatched.length, 0); assert.ok(f.item.file); assert.match(f.target.getSnapshot().error, /Quota/);
  const admitted = f.item.admission;
  f.reload(); await f.target.refresh(); f.failEnqueue(false);
  await f.target.send("request", "different-channel", "Different text");
  assert.deepEqual(f.enqueued[0], admitted); assert.deepEqual(f.dispatched[0], admitted); assert.equal(f.uploads, 1); assert.equal(f.item.file, null);
  await f.target.accepted("request"); assert.equal(f.target.getSnapshot().receipts.length, 0);
  // A receipt retained after outbox-delete failure still identifies a replay
  // as a share and never lets it clear an unrelated channel draft.
  f.reload(); await f.target.refresh(); assert.equal(f.target.isShare("request"), true);
});

test("identity network failure never becomes anonymous or sends; switching accounts hides the original receipt", async () => {
  const f = fixture(); await f.target.refresh(); f.failIdentity(true);
  await f.target.send("request", "channel", "Text");
  assert.match(f.target.getSnapshot().error, /Offline identity/); assert.equal(f.uploads, 0); assert.equal(f.enqueued.length, 0);
  f.switchUser(); await f.target.refresh(); assert.equal(f.target.getSnapshot().receipts.length, 0); assert.ok(f.item.file);
});

test("an account switch during upload cannot enqueue the original account's private share for the new account", async () => {
  const f = fixture(); await f.target.refresh(); f.switchDuringUpload();
  await f.target.send("request", "channel", "Private old account text");
  assert.equal(f.enqueued.length, 0); assert.equal(f.dispatched.length, 0); assert.ok(f.item.file); assert.equal(f.item.admission, null);
});
