import { Button, Dialog, Status, TextField } from "@sproyt/ui/react";
import { useEffect, useState } from "react";
import type { CircleChatAgent, CircleChatAgentApi, CircleChatAgentInput } from "../../chat-agents";

const lines = (value: string) => value.split(/\r?\n/u).map(item => item.trim()).filter(Boolean);
const errorText = (error: unknown) => error instanceof Error ? error.message : "Prøv igjen.";

export function CircleChatAgentsDialog({ api, circleId, circleName, onClose }: {
  api: CircleChatAgentApi; circleId: string; circleName: string; onClose: () => void;
}) {
  const [agents, setAgents] = useState<CircleChatAgent[]>([]);
  const [available, setAvailable] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<CircleChatAgent | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [name, setName] = useState("");
  const [triggers, setTriggers] = useState("");
  const [phrases, setPhrases] = useState("");
  const [enabled, setEnabled] = useState(false);

  useEffect(() => {
    let live = true;
    void api.list(circleId).then(result => {
      if (!live) return;
      setAgents(result.agents); setAvailable(result.workerAvailable); setLoading(false);
    }).catch(cause => { if (live) { setError(errorText(cause)); setLoading(false); } });
    return () => { live = false; };
  }, [api, circleId]);

  const edit = (agent: CircleChatAgent | null) => {
    setSelected(agent); setName(agent?.displayName ?? "");
    setTriggers(agent?.triggerWords.join("\n") ?? "");
    setPhrases(agent?.responsePhrases.join("\n") ?? "");
    setEnabled(agent?.enabled ?? false); setError(""); setFormOpen(true);
  };
  const save = async () => {
    if (saving) return;
    setError(""); setSaving(true);
    const input: CircleChatAgentInput = { displayName: name.trim(), triggerWords: lines(triggers),
      responsePhrases: lines(phrases), enabled, revision: selected?.revision };
    try {
      const saved = selected ? await api.update(circleId, selected.agentId, input) : await api.create(circleId, input);
      setAgents(previous => [...previous.filter(item => item.agentId !== saved.agentId), saved]
        .sort((a, b) => a.displayName.localeCompare(b.displayName)));
      setFormOpen(false); setSelected(null);
    } catch (cause) { setError(errorText(cause)); }
    finally { setSaving(false); }
  };
  return <Dialog open title={`Agentar i ${circleName}`} closeLabel="Lukk agentar" onClose={onClose}>
    <p>Agentar svarar når ei ny melding inneheld eit triggeruttrykk. Dei les berre dei siste 20 minutta i same opne kanal eller tråd. Private kanalar og direktemeldingar er ikkje med.</p>
    {!available && <Status tone="error">Modelltenesta for samtaleagentar er ikkje aktiv. Du kan lagre oppsettet, men ikkje slå på ein agent enno.</Status>}
    {loading ? <Status>Lastar agentar …</Status> : <div style={{ display: "grid", gap: 8 }}>
      {agents.length === 0 && <Status>Ingen agentar i denne kretsen enno.</Status>}
      {agents.map(agent => <div key={agent.agentId} className="sp-circle-agent-row">
        <span><strong>{agent.displayName}</strong> · {agent.enabled ? "På" : "Av"}</span>
        <Button variant="quiet" onClick={() => edit(agent)}>Rediger</Button>
      </div>)}
      {!formOpen && <Button onClick={() => edit(null)}>Ny agent</Button>}
    </div>}
    {formOpen && <form className="sp-circle-agent-form" onSubmit={event => { event.preventDefault(); void save(); }}>
      <h3>{selected ? `Rediger ${selected.displayName}` : "Ny agent"}</h3>
      <TextField label="Namn" value={name} maxLength={80} onChange={event => setName(event.target.value)} />
      <label htmlFor="circle-agent-triggers">Triggerord eller -frasar, eitt per linje</label>
      <textarea id="circle-agent-triggers" rows={4} value={triggers} onChange={event => setTriggers(event.target.value)} />
      <label htmlFor="circle-agent-phrases">Stikkord og svarføringar, eitt per linje</label>
      <textarea id="circle-agent-phrases" rows={4} value={phrases} onChange={event => setPhrases(event.target.value)} />
      <p>Agenten brukar dette som innhald og tone, ikkje som eit ferdig svar. Skriv gjerne kva han bør vite eller spørje om.</p>
      <label className="sp-circle-agent-enabled"><input type="checkbox" checked={enabled}
        disabled={!available && !enabled} onChange={event => setEnabled(event.target.checked)} />Aktiv</label>
      {error && <Status tone="error">{error}</Status>}
      <div className="sp-row"><Button type="button" onClick={() => { setFormOpen(false); setError(""); }}>Avbryt</Button>
        <Button type="submit" busy={saving} disabled={!name.trim() || !lines(triggers).length || !lines(phrases).length || (enabled && !available)}>Lagre agent</Button></div>
    </form>}
    {!formOpen && error && <Status tone="error">{error}</Status>}
  </Dialog>;
}
