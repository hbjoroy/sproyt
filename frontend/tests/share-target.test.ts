import assert from "node:assert/strict";
import test from "node:test";
import { createShareTarget, type ShareInbox, type ShareReceipt, type ShareAdmission } from "../src/share-target";

function fixture() {
  let user = "actor", generation = "0", failEnqueue = false, failIdentity = false, switchDuringUpload = false;
  let resumeEdit: (() => void) | undefined, resumeUpload: (() => void) | undefined;
  let editGate: Promise<void> | undefined, uploadGate: Promise<void> | undefined;
  let failEdit = false;
  let generationReads = 0, generationGate: Promise<void> | undefined, resumeGeneration: (() => void) | undefined;
  let item: ShareReceipt = { id: "request", owner: user, generation, createdAt: 1, text: "Delt tekst", file: new File(["image"], "original.png", { type: "image/png" }), admission: null, done: null };
  const enqueued: ShareAdmission[] = [], dispatched: ShareAdmission[] = [];
  let uploads = 0;
  const inbox = {
    identity: async () => { if (failIdentity) throw new Error("Offline identity"); return user; }, generation: async () => { if (++generationReads === 1) await generationGate; return generation; },
    list: async (owner: string) => item.owner === owner ? [item] : [],
    claim: async () => item,
    edit: async (_: string, owner: string, revision: string, text: string, channelId: string) => { await editGate; if (failEdit) throw new Error("Draft storage failed"); assert.equal(owner, item.owner); assert.equal(revision, generation); return item = { ...item, text, channelId }; },
    admit: async (_: string, owner: string, revision: string, admission: ShareAdmission) => {
      assert.equal(owner, item.owner); assert.equal(revision, generation);
      if (item.admission) assert.deepEqual(admission, item.admission);
      return item = { ...item, admission };
    },
    owned: async () => { item = { ...item, file: null, rejected: false }; },
    reject: async () => { item = { ...item, rejected: true }; },
    finish: async () => { item = { ...item, text: "", done: 1 }; },
    discard: async () => { if (item.admission && !item.rejected) throw new Error("Ambiguous"); item = { ...item, done: 1 }; }, logout: async () => { generation = String(Number(generation) + 1); }
  } as unknown as ShareInbox;
  const make = () => createShareTarget({ user: () => user, upload: async () => { uploads++; await uploadGate; if (switchDuringUpload) user = "other"; return { id: "media", contentType: "image/png", originalFilename: "original.png" }; },
    enqueue: async admission => { if (failEnqueue) throw new Error("Quota"); enqueued.push(admission); },
    drop: async () => { enqueued.length = 0; },
    dispatch: admission => { assert.equal(target.isShare(admission.requestId), true); dispatched.push(admission); }
  }, inbox);
  let target = make();
  return { get target() { return target; }, get item() { return item; }, get uploads() { return uploads; }, enqueued, dispatched,
    failEnqueue(value: boolean) { failEnqueue = value; }, failIdentity(value: boolean) { failIdentity = value; },
    switchDuringUpload() { switchDuringUpload = true; },
    failEdit(value: boolean) { failEdit = value; },
    holdFirstGeneration() { generationGate = new Promise<void>(resolve => { resumeGeneration = resolve; }); }, resumeGeneration() { resumeGeneration?.(); }, get generationReads() { return generationReads; },
    holdEdit() { editGate = new Promise<void>(resolve => { resumeEdit = resolve; }); }, resumeEdit() { resumeEdit?.(); },
    holdUpload() { uploadGate = new Promise<void>(resolve => { resumeUpload = resolve; }); }, resumeUpload() { resumeUpload?.(); },
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

test("definitive rejection can be discarded while ambiguous admission remains locked and never silently replays", async () => {
  const f = fixture(); await f.target.refresh(); await f.target.send("request", "channel", "Text");
  await f.target.discard("request"); assert.match(f.target.getSnapshot().error, /Ambiguous/); assert.equal(f.target.getSnapshot().receipts.length, 1);
  await f.target.rejected("request", "Permission denied");
  f.reload(); await f.target.refresh(); assert.equal(f.target.canReplay("request"), false);
  await f.target.discard("request"); assert.equal(f.target.getSnapshot().receipts.length, 0); assert.equal(f.enqueued.length, 0);
});

test("update guard waits for draft writes and share upload without disabling editable text during persistence", async () => {
  const f = fixture(); await f.target.refresh(); f.holdEdit();
  const edit = f.target.edit("request", "channel", "Edited");
  assert.equal(f.target.canReload(), false); assert.equal(f.target.getSnapshot().busy, false);
  f.resumeEdit(); await edit; assert.equal(f.target.canReload(), true);
  f.holdUpload(); const send = f.target.send("request", "channel", "Edited");
  assert.equal(f.target.canReload(), false);
  f.resumeUpload(); await send; assert.equal(f.target.canReload(), true);
});

test("failed draft persistence keeps the edited text and blocks refresh until an explicit successful edit or discard", async () => {
  const f = fixture(); await f.target.refresh(); f.failEdit(true);
  await f.target.edit("request", "channel", "Unsaved edit"); await f.target.refresh();
  assert.equal(f.target.getSnapshot().receipts[0]?.text, "Unsaved edit"); assert.equal(f.target.canReload(), false);
  assert.equal(f.target.getSnapshot().busy, false);
  f.failEdit(false); await f.target.edit("request", "channel", "Unsaved edit"); assert.equal(f.target.canReload(), true);
});

test("older delayed IDB open cannot overwrite a newer draft, and send waits for preceding edit writes", async () => {
  const f = fixture(); await f.target.refresh(); f.holdFirstGeneration();
  const older = f.target.edit("request", "channel", "Older");
  const newer = f.target.edit("request", "channel", "Newer");
  await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
  assert.equal(f.generationReads, 1); assert.equal(f.target.canReload(), false);
  f.resumeGeneration(); await Promise.all([older, newer]);
  assert.equal(f.item.text, "Newer"); assert.equal(f.target.canReload(), true);
  f.reload(); await f.target.refresh(); assert.equal(f.target.getSnapshot().receipts[0]?.text, "Newer");
  f.holdEdit(); const pending = f.target.edit("request", "channel", "Before Send");
  const send = f.target.send("request", "channel", "Before Send");
  await Promise.resolve(); await Promise.resolve(); assert.equal(f.uploads, 0); assert.equal(f.enqueued.length, 0);
  f.resumeEdit(); await Promise.all([pending, send]);
  assert.equal(f.enqueued[0]?.draft, "Before Send"); assert.equal(f.item.text, "Before Send");
});
