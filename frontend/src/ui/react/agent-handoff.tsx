import { Button, Status } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { CircleChatAgent, CircleChatAgentApi } from "../../chat-agents";

type AgentHandoffPolicy = Awaited<ReturnType<CircleChatAgentApi["handoff"]>>;
export type AgentHandoffDraft = {
  policy: AgentHandoffPolicy | null; targets: readonly string[]; guidance: string; pending: boolean;
  needsRead: boolean; conflict: boolean; error: string; notice: string; dirty: boolean;
};
const emptyDraft = (): AgentHandoffDraft => ({ policy: null, targets: [], guidance: "", pending: false,
  needsRead: false, conflict: false, error: "", notice: "", dirty: false });
const sameTargets = (left: readonly string[], right: readonly string[]) => {
  const sortedRight = [...right].map(id => id.toLowerCase()).sort();
  return left.length === right.length && [...left].map(id => id.toLowerCase()).sort().every((id, index) => id === sortedRight[index]);
};
const dirtyDraft = (draft: AgentHandoffDraft): AgentHandoffDraft => ({ ...draft,
  dirty: draft.policy ? !sameTargets(draft.targets, draft.policy.targetAgentIds) || draft.guidance !== draft.policy.guidance : draft.dirty });
function readDraft(draft: AgentHandoffDraft, policy: AgentHandoffPolicy): AgentHandoffDraft {
  const targetsDirty = draft.policy ? !sameTargets(draft.targets, draft.policy.targetAgentIds) : draft.dirty;
  const guidanceDirty = draft.policy ? draft.guidance !== draft.policy.guidance : draft.dirty;
  const targetsConflict = targetsDirty && !sameTargets(draft.targets, policy.targetAgentIds)
    && (!draft.policy || !sameTargets(draft.policy.targetAgentIds, policy.targetAgentIds));
  const guidanceConflict = guidanceDirty && draft.guidance !== policy.guidance
    && (!draft.policy || draft.policy.guidance !== policy.guidance);
  const next = dirtyDraft({ ...draft, policy, targets: targetsDirty ? draft.targets : policy.targetAgentIds,
    guidance: guidanceDirty ? draft.guidance : policy.guidance, needsRead: false, error: "", notice: "Lagra oppsett er henta.",
    conflict: targetsConflict || guidanceConflict || draft.conflict });
  return { ...next, conflict: next.dirty && next.conflict };
}
const errorText = (error: unknown) => error instanceof Error ? error.message : "Kunne ikkje lagre videresendinga. Prøv igjen.";

/** Separate from the main editor: saving a policy also revises its source agent. */
export function AgentHandoff({ api, circleId, source, agents, onClose, onSaved, initialDraft, onDraftChange, admitted }: {
  api: CircleChatAgentApi; circleId: string; source: CircleChatAgent; agents: readonly CircleChatAgent[];
  onClose: () => void; onSaved: (agentRevision: number) => Promise<void> | void;
  initialDraft?: AgentHandoffDraft; onDraftChange?: (draft: AgentHandoffDraft, expected?: AgentHandoffDraft) => boolean | void; admitted?: () => boolean;
}) {
  const [draft, setDraft] = useState<AgentHandoffDraft>(() => initialDraft ?? emptyDraft());
  const [loading, setLoading] = useState(true);
  const [reload, setReload] = useState(0);
  const callbacks = useRef({ onDraftChange, admitted });
  callbacks.current = { onDraftChange, admitted };
  type Session = { live: boolean; draft: AgentHandoffDraft; readAfterPending: boolean;
    publish?: (draft: AgentHandoffDraft, expected?: AgentHandoffDraft) => boolean | void; admitted: () => boolean };
  const session = useRef<Session | null>(null);
  const publish = (scope: Session, next: AgentHandoffDraft, expected?: AgentHandoffDraft) => {
    if (!scope.admitted() || (expected && scope.draft !== expected)) return;
    if (scope.publish?.(next, expected) === false) return;
    scope.draft = next;
    if (scope.live && session.current === scope) setDraft(next);
  };
  useEffect(() => {
    const scope: Session = { live: true, draft: initialDraft ?? emptyDraft(), readAfterPending: initialDraft?.pending === true,
      publish: callbacks.current.onDraftChange, admitted: callbacks.current.admitted ?? (() => true) };
    session.current = scope;
    setDraft(scope.draft);
    return () => { scope.live = false; };
  }, [api, circleId, source.agentId]);
  useEffect(() => {
    const scope = session.current;
    if (!scope?.live || !initialDraft || !scope.admitted()) return;
    scope.draft = initialDraft; setDraft(initialDraft);
    if (scope.readAfterPending && !initialDraft.pending) {
      scope.readAfterPending = false;
      setReload(value => value + 1);
    }
  }, [initialDraft]);
  useEffect(() => {
    const scope = session.current;
    if (!scope?.live || !scope.admitted()) return;
    if (scope.draft.pending) { setLoading(false); return; }
    let live = true;
    setLoading(true);
    void api.handoff(circleId, source.agentId).then(result => {
      if (!live || !scope.live || !scope.admitted()) return;
      publish(scope, readDraft(scope.draft, result));
    }).catch(cause => {
      if (live && scope.live && scope.admitted()) publish(scope, { ...scope.draft, needsRead: true, error: errorText(cause) });
    }).finally(() => { if (live && scope.live && scope.admitted()) setLoading(false); });
    return () => { live = false; };
  }, [api, circleId, source.agentId, reload]);
  const { policy, targets, guidance, pending: saving, error, conflict, needsRead, notice } = draft;
  const eligible = agents.filter(agent => agent.agentId !== source.agentId && agent.enabled);
  const unavailable = targets.filter(id => !eligible.some(agent => agent.agentId === id));
  const edit = (change: Partial<AgentHandoffDraft>) => {
    const scope = session.current;
    if (scope?.live && scope.admitted() && !scope.draft.pending && !loading) publish(scope, dirtyDraft({ ...scope.draft, error: "", notice: "", ...change }));
  };
  const save = async () => {
    const scope = session.current;
    if (!scope?.live || !scope.admitted() || !policy?.available || scope.draft.pending || saving || loading || conflict || needsRead
      || unavailable.length > 0 || targets.length > 4 || [...guidance].length > 1000) return;
    const pending = { ...scope.draft, pending: true, error: "", notice: "" };
    publish(scope, pending);
    try {
      if (!scope.admitted()) return;
      const saved = await api.updateHandoff(circleId, source.agentId, { revision: policy.revision, targetAgentIds: targets, guidance });
      if (!scope.admitted() || scope.draft !== pending) return;
      if (scope.live && session.current === scope && scope.admitted()) await onSaved(saved.agentRevision);
      if (!scope.admitted()) return;
      publish(scope, { ...pending, policy: saved, targets: saved.targetAgentIds, guidance: saved.guidance,
        pending: false, needsRead: false, conflict: false, dirty: false, error: "", notice: "Videresendinga er lagra." }, pending);
    } catch (cause) {
      if (scope.admitted()) publish(scope, { ...pending, pending: false, needsRead: true, error: errorText(cause), notice: "" }, pending);
    }
  };
  return <section className="sp-circle-agent-form" aria-label={`Videresending frå ${source.displayName}`}>
    <h3>Videresending frå {source.displayName}</h3>
    <p>Agenten kan sende den opphavlege førespurnaden vidare til ein annan agent i same kanal. Mottakaren må ha eigen tilgang til kanalen og brukar sine eigne verktøy. Agentane lagar ikkje nye deloppgåver for kvarandre.</p>
    <p>Førebels gjeld dette berre nye førespurnader sende gjennom canary. Ingen mottakarar er valde automatisk.</p>
    {loading ? <Status>Lastar videresending …</Status> : <>
      {policy && !policy.available && <Status>Videresending er ikkje tilgjengeleg i dette miljøet enno.</Status>}
      {policy && <>
        <fieldset disabled={saving || loading || !policy.available || conflict} style={{ minWidth: 0 }}>
          <legend>Mottakarar, opptil fire</legend>
          {eligible.length === 0 && <p>Ingen andre aktive agentar i kretsen enno.</p>}
          {eligible.map(agent => <label className="sp-circle-agent-enabled" key={agent.agentId}>
            <input type="checkbox" checked={targets.includes(agent.agentId)}
              disabled={!targets.includes(agent.agentId) && targets.length >= 4}
              onChange={event => edit({ targets: event.target.checked ? [...targets, agent.agentId] : targets.filter(id => id !== agent.agentId) })} />{agent.displayName}
          </label>)}
          {unavailable.map(id => <label className="sp-circle-agent-enabled" key={id}>
            <input type="checkbox" checked onChange={() => edit({ targets: targets.filter(target => target !== id) })} />
            {agents.find(agent => agent.agentId === id)?.displayName ?? "Tidlegare mottakar"} (ikkje tilgjengeleg; fjern frå utvalet)
          </label>)}
          {unavailable.length > 0 && <Status tone="error">Fjern mottakarar som ikkje lenger er tilgjengelege før du lagrar.</Status>}
          <label htmlFor="circle-agent-handoff-guidance">Når bør agenten sende vidare? (valfritt)</label>
          <textarea id="circle-agent-handoff-guidance" rows={3} maxLength={1000} value={guidance}
            onChange={event => edit({ guidance: event.target.value })} />
        </fieldset>
        <p>Fjern alle mottakarane for å stoppe nye videresendingar. Førespurnader som alt er sende, kan bli fullførte dersom mottakaren framleis har tilgang.</p>
      </>}
    </>}
    {error && <Status tone="error">{error}</Status>}
    {notice && <Status>{notice}</Status>}
    {needsRead && <Status>Hent lagra oppsett før du prøver å lagre igjen. Utkastet ditt blir bevart.</Status>}
    {conflict && <div>
      <Status tone="error">Same felt er endra både i utkastet og i det lagra oppsettet. Vel kva du vil bruke før du lagrar.</Status>
      <Button disabled={saving || loading || needsRead} onClick={() => edit({ conflict: false, notice: "Utkastet er behalde. Lagre for å bruke endringane." })}>Behald utkastet mitt</Button>
      <Button disabled={saving || loading || needsRead} onClick={() => { if (policy) edit({ targets: policy.targetAgentIds, guidance: policy.guidance, conflict: false }); }}>Bruk lagra oppsett</Button>
    </div>}
    <div className="sp-row">
      <Button disabled={saving} onClick={() => { if (session.current?.live && session.current.admitted()) onClose(); }}>Avbryt videresending</Button>
      {(error || needsRead) && <Button disabled={saving || loading} onClick={() => { if (session.current?.admitted()) setReload(value => value + 1); }}>Hent lagra oppsett</Button>}
      <Button busy={saving} disabled={saving || loading || needsRead || conflict || !policy?.available || unavailable.length > 0 || targets.length > 4 || [...guidance].length > 1000}
        onClick={() => { void save(); }}>Lagre videresending</Button>
    </div>
  </section>;
}
