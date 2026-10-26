import type { CircleChatAgent, CircleChatAgentInput } from "../../chat-agents";
import type { AgentHandoffDraft } from "./agent-handoff";

export type AgentDraftValues = {
  name: string; triggers: string; phrases: string;
  memoryEnabled: boolean; visionEnabled: boolean;
  imagesEnabled: boolean; imageIdentity: string; imagesOccasional: boolean;
  weatherEnabled: boolean; location: string; latitude: string; longitude: string;
  seaEnabled: boolean; seaLocations: string[]; seaDefault: string; ferryEnabled: boolean;
};
export type AgentEditorPart = "behaviour" | "memory" | "vision" | "images" | "weather" | "sea" | "ferry";
export const agentPartFields: Record<AgentEditorPart, readonly (keyof AgentDraftValues)[]> = {
  behaviour: ["name", "triggers", "phrases"], memory: ["memoryEnabled"], vision: ["visionEnabled"],
  images: ["imagesEnabled", "imageIdentity", "imagesOccasional"],
  weather: ["weatherEnabled", "location", "latitude", "longitude"],
  sea: ["seaEnabled", "seaLocations", "seaDefault"], ferry: ["ferryEnabled"]
};
export type AgentEditorDraft = {
  baseline: CircleChatAgent | null; values: AgentDraftValues; conflicts: (keyof AgentDraftValues)[];
  pending: boolean; needsRead: boolean; error: string; notice: string;
  handoff?: AgentHandoffDraft;
};
const equal = (left: unknown, right: unknown) => JSON.stringify(left) === JSON.stringify(right);
const lines = (text: string) => text.split(/\r?\n/u).map(value => value.trim()).filter(Boolean);
function sameField(field: keyof AgentDraftValues, left: unknown, right: unknown) {
  if (typeof left === "string" && typeof right === "string") {
    if (field === "name" || field === "location") return left.trim() === right.trim();
    if (field === "triggers" || field === "phrases") return equal(lines(left), lines(right));
    if ((field === "latitude" || field === "longitude") && left.trim() && right.trim()
      && Number.isFinite(Number(left)) && Number.isFinite(Number(right))) return Number(left) === Number(right);
  }
  return equal(left, right);
}
export function agentValues(agent: CircleChatAgent | null): AgentDraftValues {
  return { name: agent?.displayName ?? "", triggers: agent?.triggerWords.join("\n") ?? "", phrases: agent?.responsePhrases.join("\n") ?? "",
    memoryEnabled: agent?.memoryEnabled ?? false, visionEnabled: agent?.visionEnabled ?? false,
    imagesEnabled: agent?.imageGeneration?.enabled ?? false, imageIdentity: agent?.imageGeneration?.identityId ?? "",
    imagesOccasional: agent?.imageGeneration?.occasional ?? false,
    weatherEnabled: !!agent?.weather, location: agent?.weather?.location ?? "Parikia",
    latitude: String(agent?.weather?.latitude ?? 37.085), longitude: String(agent?.weather?.longitude ?? 25.148),
    seaEnabled: !!agent?.seaTemperature, seaLocations: [...(agent?.seaTemperature?.locations ?? [])], seaDefault: agent?.seaTemperature?.defaultLocation ?? "",
    ferryEnabled: agent?.ferryPort === "paros" };
}
const key = (circleId: string, agentId: string) => `${circleId}:${agentId}`;
const dirty = (draft: AgentEditorDraft) => !equal(draft.values, agentValues(draft.baseline));
const protectedDraft = (draft: AgentEditorDraft) => dirty(draft) || draft.pending || !!draft.handoff?.dirty || !!draft.handoff?.pending;

/** PATCH still requires confirmed core fields. Other optional parts are absent. */
export function agentPartInput(draft: AgentEditorDraft, part: AgentEditorPart): CircleChatAgentInput {
  const v = draft.values, original = draft.baseline;
  const input: CircleChatAgentInput = {
    displayName: part === "behaviour" ? v.name.trim() : original!.displayName,
    triggerWords: part === "behaviour" ? lines(v.triggers) : original!.triggerWords,
    responsePhrases: part === "behaviour" ? lines(v.phrases) : original!.responsePhrases,
    enabled: original?.enabled ?? false, ...(original ? { revision: original.revision } : {})
  };
  switch (part) {
    case "memory": return { ...input, memoryEnabled: v.memoryEnabled };
    case "vision": return { ...input, visionEnabled: v.visionEnabled };
    case "images": return { ...input, imageGeneration: v.imageIdentity ? {
      identityId: v.imageIdentity, enabled: v.imagesEnabled, occasional: v.imagesEnabled && v.imagesOccasional } : null };
    case "weather": return { ...input, weather: v.weatherEnabled ? { location: v.location.trim(), latitude: Number(v.latitude), longitude: Number(v.longitude) } : null };
    case "sea": return { ...input, seaTemperature: v.seaEnabled ? { defaultLocation: v.seaDefault, locations: v.seaLocations } : null };
    case "ferry": return { ...input, ferryPort: v.ferryEnabled ? "paros" : null };
    default: return input;
  }
}

/** A bounded, in-memory workspace owned by one authenticated management view.
 * Navigation can unmount an editor without losing its text. Nothing is queued,
 * persisted to disk, or carried to another account. */
export class AgentSettingsDrafts {
  #drafts = new Map<string, AgentEditorDraft>();
  #listeners = new Set<() => void>();
  #version = 0;
  #writeEpoch = 0;
  #successEpoch = 0;
  get writeEpoch() { return this.#writeEpoch; }
  get successEpoch() { return this.#successEpoch; }
  getSnapshot = () => this.#version;
  subscribe = (listener: () => void) => { this.#listeners.add(listener); return () => { this.#listeners.delete(listener); }; };
  #changed() { this.#version++; for (const listener of this.#listeners) listener(); }
  get(circleId: string, agentId: string) { return this.#drafts.get(key(circleId, agentId)); }
  start(circleId: string, agentId: string, baseline: CircleChatAgent | null): AgentEditorDraft {
    const id = key(circleId, agentId), existing = this.#drafts.get(id);
    if (existing) return existing;
    if (this.#drafts.size >= 24) {
      const disposable = [...this.#drafts].find(([, draft]) => !protectedDraft(draft));
      if (!disposable) throw new Error("Du har mange uferdige agentutkast. Lagre eller forkast eit utkast først.");
      this.#drafts.delete(disposable[0]);
    }
    const draft: AgentEditorDraft = { baseline, values: agentValues(baseline), conflicts: [], pending: false, needsRead: false, error: "", notice: "" };
    this.#drafts.set(id, draft);
    return draft;
  }
  hasChanges() { return [...this.#drafts.values()].some(protectedDraft); }
  isDirty(circleId: string, agentId: string) { const draft = this.get(circleId, agentId); return !!draft && protectedDraft(draft); }
  update(circleId: string, agentId: string, values: Partial<AgentDraftValues>) {
    const draft = this.get(circleId, agentId); if (!draft || draft.pending) return;
    draft.values = { ...draft.values, ...values }; draft.notice = ""; this.#changed();
  }
  pending(circleId: string, agentId: string) {
    const draft = this.get(circleId, agentId); if (!draft) return;
    draft.pending = true; draft.error = ""; draft.notice = ""; this.#writeEpoch++; this.#changed();
  }
  failed(circleId: string, agentId: string, error: string, expected?: AgentEditorDraft) {
    const draft = this.get(circleId, agentId); if (!draft || (expected && draft !== expected)) return;
    if (draft.pending) this.#writeEpoch++;
    draft.pending = false; draft.needsRead = true; draft.error = error; this.#changed();
  }
  requireRead(circleId: string, agentId: string) {
    const draft = this.get(circleId, agentId); if (!draft) return;
    draft.needsRead = true; this.#changed();
  }
  handoff(circleId: string, agentId: string, value: AgentHandoffDraft, expected?: AgentHandoffDraft): boolean {
    const draft = this.get(circleId, agentId); if (!draft || (expected && draft.handoff !== expected)) return false;
    if (!!draft.handoff?.pending !== value.pending) this.#writeEpoch++;
    if (draft.handoff?.pending && !value.pending && !value.needsRead && !value.error
      && draft.baseline?.revision !== value.policy?.agentRevision) this.#successEpoch++;
    draft.handoff = value; this.#changed(); return true;
  }
  /** Reconcile confirmed fields, preserving unrelated edits and exposing overlap.
   * A handoff save must read the complete agent before advancing its revision. */
  confirmed(circleId: string, agent: CircleChatAgent, savedPart?: AgentEditorPart | "activation", expected?: AgentEditorDraft): boolean {
    const draft = this.get(circleId, agent.agentId); if (!draft || (expected && draft !== expected)) return false;
    if (draft.baseline && agent.revision < draft.baseline.revision) return false;
    // A read racing an already admitted write cannot resolve that write.
    if (draft.pending && !savedPart) return false;
    const old = agentValues(draft.baseline), latest = agentValues(agent), values = { ...draft.values };
    const conflicts = new Set(draft.conflicts);
    for (const field of Object.keys(values) as (keyof AgentDraftValues)[]) {
      const saved = savedPart && savedPart !== "activation" && agentPartFields[savedPart].includes(field);
      if (saved || sameField(field, values[field], old[field]) || sameField(field, values[field], latest[field])) {
        Object.assign(values, { [field]: latest[field] }); conflicts.delete(field);
      } else if (!equal(old[field], latest[field])) conflicts.add(field);
    }
    draft.baseline = agent; draft.values = values; draft.conflicts = [...conflicts];
    draft.pending = false; draft.needsRead = false; draft.error = "";
    if (savedPart) { this.#writeEpoch++; this.#successEpoch++; }
    draft.notice = savedPart ? "Endringa er lagra." : dirty(draft) ? "Lagra oppsett er henta. Utkastet ditt er bevart." : "Du viser det lagra oppsettet.";
    this.#changed(); return true;
  }
  created(circleId: string, agent: CircleChatAgent, expected: AgentEditorDraft): boolean {
    const draft = this.get(circleId, "new"); if (draft !== expected) return false;
    draft.baseline = agent; draft.values = agentValues(agent); draft.pending = false; draft.needsRead = false;
    draft.error = ""; draft.notice = "Agenten er oppretta.";
    this.#writeEpoch++; this.#successEpoch++; this.#changed(); return true;
  }
  acknowledge(circleId: string, agentId: string, part: AgentEditorPart, useServer: boolean) {
    const draft = this.get(circleId, agentId); if (!draft) return;
    if (useServer) {
      const latest = agentValues(draft.baseline);
      for (const field of agentPartFields[part]) Object.assign(draft.values, { [field]: latest[field] });
    }
    draft.conflicts = draft.conflicts.filter(field => !agentPartFields[part].includes(field)); this.#changed();
  }
  remove(circleId: string, agentId: string) { if (this.#drafts.delete(key(circleId, agentId))) this.#changed(); }
  /** A changed authority scope permanently retires old operation identities.
   * Preserve authored values, but never leave an abandoned request blocking a
   * newly admitted view. Silent mode is used before React replaces that view. */
  invalidateCircle(circleId: string, notify = true) {
    let changed = false;
    for (const [id, draft] of this.#drafts) {
      if (!id.startsWith(`${circleId}:`)) continue;
      const interrupted = draft.pending || !!draft.handoff?.pending;
      this.#drafts.set(id, { ...draft, pending: false, needsRead: draft.needsRead || interrupted,
        ...(draft.handoff ? { handoff: { ...draft.handoff, pending: false,
          needsRead: draft.handoff.needsRead || draft.handoff.pending } } : {}) });
      changed = true;
    }
    if (changed) { this.#writeEpoch++; if (notify) this.#changed(); else this.#version++; }
  }
  clear() { this.#drafts.clear(); this.#changed(); }
  forget() { this.#drafts.clear(); this.#listeners.clear(); }
  prune(allowed: (circleId: string) => boolean) {
    let changed = false;
    for (const id of this.#drafts.keys()) if (!allowed(id.slice(0, id.lastIndexOf(":")))) { this.#drafts.delete(id); changed = true; }
    if (changed) this.#changed();
  }
}
