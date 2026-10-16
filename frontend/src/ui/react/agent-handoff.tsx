import { Button, Status } from "@sproyt/ui/react";
import { useEffect, useState } from "react";
import type { CircleChatAgent, CircleChatAgentApi } from "../../chat-agents";

type Handoff = Awaited<ReturnType<CircleChatAgentApi["handoff"]>>;
const errorText = (error: unknown) => error instanceof Error ? error.message : "Kunne ikkje lagre videresendinga. Prøv igjen.";

/** Separate from the main editor: saving a policy also revises its source agent. */
export function AgentHandoff({ api, circleId, source, agents, onClose, onSaved }: {
  api: CircleChatAgentApi; circleId: string; source: CircleChatAgent; agents: readonly CircleChatAgent[];
  onClose: () => void; onSaved: (agentRevision: number) => void;
}) {
  const [policy, setPolicy] = useState<Handoff | null>(null);
  const [targets, setTargets] = useState<readonly string[]>([]);
  const [guidance, setGuidance] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [reload, setReload] = useState(0);
  useEffect(() => {
    let live = true;
    setLoading(true); setError("");
    void api.handoff(circleId, source.agentId).then(result => {
      if (!live) return;
      setPolicy(result); setTargets(result.targetAgentIds); setGuidance(result.guidance);
    }).catch(cause => { if (live) setError(errorText(cause)); })
      .finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, [api, circleId, source.agentId, reload]);
  const eligible = agents.filter(agent => agent.agentId !== source.agentId && agent.enabled);
  const unavailable = targets.filter(id => !eligible.some(agent => agent.agentId === id));
  const save = async () => {
    if (!policy?.available || saving || loading || unavailable.length > 0 || targets.length > 4 || [...guidance].length > 1000) return;
    setSaving(true); setError("");
    try {
      const saved = await api.updateHandoff(circleId, source.agentId, { revision: policy.revision, targetAgentIds: targets, guidance });
      onSaved(saved.agentRevision);
    } catch (cause) { setError(errorText(cause)); }
    finally { setSaving(false); }
  };
  return <section className="sp-circle-agent-form" aria-label={`Videresending frå ${source.displayName}`}>
    <h3>Videresending frå {source.displayName}</h3>
    <p>Agenten kan sende den opphavlege førespurnaden vidare til ein annan agent i same kanal. Mottakaren må ha eigen tilgang til kanalen og brukar sine eigne verktøy. Agentane lagar ikkje nye deloppgåver for kvarandre.</p>
    <p>Førebels gjeld dette berre nye førespurnader sende gjennom canary. Ingen mottakarar er valde automatisk.</p>
    {loading ? <Status>Lastar videresending …</Status> : <>
      {policy && !policy.available && <Status>Videresending er ikkje tilgjengeleg i dette miljøet enno.</Status>}
      {policy && <>
        <fieldset disabled={saving || !policy.available} style={{ minWidth: 0 }}>
          <legend>Mottakarar, opptil fire</legend>
          {eligible.length === 0 && <p>Ingen andre aktive agentar i kretsen enno.</p>}
          {eligible.map(agent => <label className="sp-circle-agent-enabled" key={agent.agentId}>
            <input type="checkbox" checked={targets.includes(agent.agentId)}
              disabled={!targets.includes(agent.agentId) && targets.length >= 4}
              onChange={event => setTargets(previous => event.target.checked ? [...previous, agent.agentId] : previous.filter(id => id !== agent.agentId))} />{agent.displayName}
          </label>)}
          {unavailable.map(id => <label className="sp-circle-agent-enabled" key={id}>
            <input type="checkbox" checked onChange={() => setTargets(previous => previous.filter(target => target !== id))} />
            {agents.find(agent => agent.agentId === id)?.displayName ?? "Tidlegare mottakar"} (ikkje tilgjengeleg; fjern frå utvalet)
          </label>)}
          {unavailable.length > 0 && <Status tone="error">Fjern mottakarar som ikkje lenger er tilgjengelege før du lagrar.</Status>}
          <label htmlFor="circle-agent-handoff-guidance">Når bør agenten sende vidare? (valfritt)</label>
          <textarea id="circle-agent-handoff-guidance" rows={3} maxLength={1000} value={guidance}
            onChange={event => setGuidance(event.target.value)} />
        </fieldset>
        <p>Fjern alle mottakarane for å stoppe nye videresendingar. Førespurnader som alt er sende, kan bli fullførte dersom mottakaren framleis har tilgang.</p>
      </>}
    </>}
    {error && <Status tone="error">{error}</Status>}
    <div className="sp-row">
      <Button disabled={saving} onClick={onClose}>Avbryt videresending</Button>
      {error && <Button disabled={saving || loading} onClick={() => setReload(value => value + 1)}>Last oppsett på nytt</Button>}
      <Button busy={saving} disabled={loading || !policy?.available || unavailable.length > 0 || targets.length > 4 || [...guidance].length > 1000}
        onClick={() => { void save(); }}>Lagre videresending</Button>
    </div>
  </section>;
}
