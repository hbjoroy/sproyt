import assert from "node:assert/strict";
import test from "node:test";
import type { CircleChatAgent } from "../src/chat-agents";
import type { AgentHandoffDraft } from "../src/ui/react/agent-handoff";
import { AgentSettingsDrafts, agentPartInput } from "../src/ui/react/agent-settings-drafts";

const circle = "00000000-0000-7000-8000-000000008001", id = "00000000-0000-7000-8000-000000008002";
const agent = (patch: Partial<CircleChatAgent> = {}): CircleChatAgent => ({ agentId: id, circleId: circle, displayName: "Maria",
  triggerWords: ["hei"], responsePhrases: ["Svar venleg"], enabled: true, revision: 3, workerAvailable: true,
  memoryEnabled: false, visionEnabled: false, visionAvailable: true, weather: { location: "Bjorøy", latitude: 60.322, longitude: 5.199 },
  seaTemperature: { defaultLocation: "legacy", locations: ["legacy"] }, ferryPort: "paros",
  imageGeneration: { identityId: "old-identity", enabled: true, occasional: true }, imageGenerationAvailable: true, ...patch });

test("agent draft parts preserve unsubmitted text and only send their own optional settings", () => {
  const store = new AgentSettingsDrafts(), original = agent(), draft = store.start(circle, id, original);
  store.update(circle, id, { name: "Mitt uferdige namn", memoryEnabled: true, location: "Parikia", latitude: "37.085", longitude: "25.148" });
  assert.equal(store.start(circle, id, agent({ revision: 4 })).values.name, "Mitt uferdige namn");
  assert.deepEqual(agentPartInput(draft, "weather"), { displayName: "Maria", triggerWords: ["hei"], responsePhrases: ["Svar venleg"],
    enabled: true, revision: 3, weather: { location: "Parikia", latitude: 37.085, longitude: 25.148 } });
  store.pending(circle, id);
  const saved = agent({ revision: 4, weather: { location: "Parikia", latitude: 37.085, longitude: 25.148 } });
  assert.equal(store.confirmed(circle, saved, "weather", draft), true);
  assert.equal(draft.values.name, "Mitt uferdige namn"); assert.equal(draft.values.memoryEnabled, true);
  assert.equal(draft.baseline?.seaTemperature?.defaultLocation, "legacy");
  const next = agentPartInput(draft, "memory");
  assert.equal(next.displayName, "Maria"); assert.equal(next.revision, 4); assert.equal(next.memoryEnabled, true);
  for (const field of ["weather", "seaTemperature", "ferryPort", "visionEnabled", "imageGeneration"]) assert.equal(Object.hasOwn(next, field), false);
});

test("a conflict preserves the draft, shows a new baseline, and requires an explicit field decision", () => {
  const store = new AgentSettingsDrafts(), draft = store.start(circle, id, agent());
  store.update(circle, id, { location: "Min stad", latitude: "ugyldig men viktig tekst" });
  store.pending(circle, id); store.failed(circle, id, "ukjent lagringsutfall", draft);
  assert.equal(draft.needsRead, true); assert.equal(draft.values.latitude, "ugyldig men viktig tekst");
  store.confirmed(circle, agent({ revision: 4, weather: { location: "Ein annan stad", latitude: 50, longitude: 5.199 } }));
  assert.equal(draft.needsRead, false); assert.equal(draft.values.location, "Min stad");
  assert.ok(draft.conflicts.includes("location")); assert.ok(draft.conflicts.includes("latitude"));
  store.acknowledge(circle, id, "weather", false);
  assert.deepEqual(draft.conflicts, []); assert.equal(draft.values.location, "Min stad");
  store.acknowledge(circle, id, "weather", true);
  assert.equal(draft.values.location, "Ein annan stad"); assert.equal(draft.values.latitude, "50");
});

test("read-back recognises an already applied normalized update without replaying it", () => {
  const store = new AgentSettingsDrafts(), draft = store.start(circle, id, agent());
  store.update(circle, id, { location: "  Parikia  ", latitude: "037.085", longitude: "25.1480" });
  store.pending(circle, id); store.failed(circle, id, "timeout", draft);
  store.confirmed(circle, agent({ revision: 4, weather: { location: "Parikia", latitude: 37.085, longitude: 25.148 } }));
  assert.deepEqual(draft.conflicts, []); assert.equal(draft.needsRead, false); assert.equal(store.isDirty(circle, id), false);
});

test("a stale GET cannot resolve pending writes or roll back a newer confirmed revision", () => {
  const store = new AgentSettingsDrafts(), draft = store.start(circle, id, agent());
  const beforeWrite = store.writeEpoch;
  store.pending(circle, id); assert.notEqual(store.writeEpoch, beforeWrite);
  const duringWrite = store.writeEpoch;
  assert.equal(store.confirmed(circle, agent({ revision: 2 })), false); assert.equal(draft.pending, true);
  assert.equal(store.confirmed(circle, agent({ revision: 3 })), false); assert.equal(draft.pending, true);
  store.confirmed(circle, agent({ revision: 4, displayName: "Ny Maria" }), "behaviour", draft);
  assert.notEqual(store.writeEpoch, duringWrite); assert.ok(store.successEpoch > 0);
  assert.equal(store.confirmed(circle, agent({ revision: 3 })), false);
  assert.equal(draft.baseline?.displayName, "Ny Maria"); assert.equal(draft.baseline?.revision, 4);
});

test("a pruned or replaced resource rejects completions from the old draft identity", () => {
  const store = new AgentSettingsDrafts(), old = store.start(circle, id, agent());
  store.pending(circle, id); store.prune(() => false);
  const current = store.start(circle, id, agent({ revision: 7 })); store.update(circle, id, { name: "Nytt utkast" });
  assert.equal(store.confirmed(circle, agent({ revision: 8 }), "behaviour", old), false);
  store.failed(circle, id, "Gammal feil", old);
  assert.equal(current.values.name, "Nytt utkast"); assert.equal(current.error, ""); assert.equal(current.baseline?.revision, 7);
  store.forget(); assert.equal(store.get(circle, id), undefined);
  assert.equal(store.confirmed(circle, agent({ revision: 9 }), "behaviour", current), false);
});

test("the bounded workspace never silently evicts authored edits or handoff drafts", () => {
  const store = new AgentSettingsDrafts();
  for (let n=0;n<24;n++) { const resource = String(n); store.start(circle, resource, agent({ agentId: resource })); store.update(circle, resource, { name: `Utkast ${n}` }); }
  assert.throws(() => store.start(circle, "overflow", agent()), /uf.*agentutkast/u);
  assert.equal(store.get(circle, "0")?.values.name, "Utkast 0");
  store.remove(circle, "3"); store.start(circle, "overflow", agent());
  assert.equal(store.get(circle, "0")?.values.name, "Utkast 0");
});

test("handoff cache CAS rejects old pending completions and preserves a newer session", () => {
  const store = new AgentSettingsDrafts(); store.start(circle, id, agent());
  const pending: AgentHandoffDraft = { policy: { available: true, revision: 1, agentRevision: 3, targetAgentIds: [], guidance: "" },
    targets: [], guidance: "Uferdig", pending: true, needsRead: false, conflict: false, error: "", notice: "", dirty: true };
  store.handoff(circle, id, pending);
  const newer = { ...pending, guidance: "Nytt utkast", pending: false };
  store.handoff(circle, id, newer);
  assert.equal(store.handoff(circle, id, { ...pending, pending: false, dirty: false }, pending), false);
  assert.equal(store.get(circle, id)?.handoff, newer); assert.equal(store.hasChanges(), true);
});

test("new agents are created off, and late completion leaves a receipt instead of another create form", () => {
  const store = new AgentSettingsDrafts(), draft = store.start(circle, "new", null);
  store.update(circle, "new", { name: " Hjelpar ", triggers: " hei \n", phrases: " Ver venleg " });
  assert.deepEqual(agentPartInput(draft, "behaviour"), { displayName: "Hjelpar", triggerWords: ["hei"], responsePhrases: ["Ver venleg"], enabled: false });
  store.pending(circle, "new");
  const saved = agent({ displayName: "Hjelpar", enabled: false, revision: 1 });
  assert.equal(store.created(circle, saved, draft), true);
  assert.equal(store.get(circle, "new")?.baseline?.agentId, id); assert.equal(store.hasChanges(), false);
});

test("authority epoch changes preserve text while retiring pending save and handoff identities", () => {
  const store = new AgentSettingsDrafts(), old = store.start(circle, id, agent());
  store.update(circle, id, { name: "Utkast under rollebyte" }); store.pending(circle, id);
  const handoff: AgentHandoffDraft = { policy: null, targets: [], guidance: "Uferdig samarbeid", pending: true,
    needsRead: false, conflict: false, error: "", notice: "", dirty: true };
  store.handoff(circle, id, handoff);
  store.invalidateCircle(circle);
  const next = store.get(circle, id)!;
  assert.notEqual(next, old); assert.equal(next.pending, false); assert.equal(next.needsRead, true);
  assert.equal(next.values.name, "Utkast under rollebyte"); assert.equal(next.handoff?.pending, false);
  assert.equal(next.handoff?.guidance, "Uferdig samarbeid"); assert.equal(next.handoff?.needsRead, true);
  assert.equal(store.confirmed(circle, agent({ revision: 4 }), "behaviour", old), false);
  assert.equal(store.handoff(circle, id, { ...handoff, pending: false }, handoff), false);
  store.confirmed(circle, agent());
  assert.equal(next.needsRead, false); assert.equal(next.values.name, "Utkast under rollebyte");
});
