import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import { HttpError } from "../../api";
import { memoryTextBytes, validMemoryText, type AgentMemory, type AgentMemoryApi, type MemoryAction, type MemoryAgent } from "../../agent-memory";
import type { Channel } from "../../types";

const errorText = (error: unknown) => error instanceof HttpError && error.status === 409
  ? "Minnet vart endra på ei anna eining. Hent det på nytt før du lagrar. Utkastet ditt blir ståande."
  : error instanceof HttpError && [401,403].includes(error.status) ? "Du har ikkje tilgang no. Lukk og logg inn på nytt."
  : "Kunne ikkje hente eller lagre minnet. Prøv igjen.";

function MemoryPanel({ api, circleId, agent, channels }: { api: AgentMemoryApi; circleId: string; agent: MemoryAgent; channels: readonly Readonly<Channel>[] }) {
  const [view, setView] = useState<AgentMemory>();
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [blocked, setBlocked] = useState(false);
  const [edit, setEdit] = useState<{ id: string; text: string }>();
  const [confirm, setConfirm] = useState<MemoryAction>();
  const live = useRef(false);
  const request = useRef(0);
  const saving = useRef(false);
  useEffect(() => { live.current = true; return () => { live.current = false; request.current++; }; }, []);
  const load = async (signal?: AbortSignal) => {
    const ticket = ++request.current; setLoading(true); setError("");
    try {
      const current = await api.get(circleId, agent.agentId, signal);
      if (!live.current || ticket !== request.current) return;
      setView(current); setBlocked(false);
      // Never apply a draft to a note that was deleted or lost access.
      setEdit(previous => previous && current.notes.some(note => note.id === previous.id) ? previous : undefined);
      setConfirm(undefined);
    } catch (cause) {
      if (live.current && ticket === request.current && !signal?.aborted) {
        setError(errorText(cause)); setBlocked(true); setView(undefined);
        if (cause instanceof HttpError && [401,403].includes(cause.status)) setEdit(undefined);
      }
    } finally { if (live.current && ticket === request.current) setLoading(false); }
  };
  useEffect(() => { const controller = new AbortController(); void load(controller.signal); return () => controller.abort(); }, [api, circleId, agent.agentId]);
  const mutate = async (action: MemoryAction | boolean) => {
    if (!view || saving.current || blocked || loading) return;
    saving.current = true; setBusy(true); setError(""); setNotice("");
    const ticket = ++request.current;
    try {
      const next = typeof action === "boolean" ? await api.choice(circleId, agent.agentId, view.revision, action)
        : await api.action(circleId, agent.agentId, view.revision, action);
      if (!live.current || ticket !== request.current) return;
      setView(next); setEdit(undefined); setConfirm(undefined);
      setNotice(next.historyCompactions > view.historyCompactions ? "Lagret. Eldre kjelder er sperra frå ny innsamling."
        : typeof action === "boolean" ? "Minnevalet er lagra." : "Minnet er oppdatert.");
    } catch (cause) {
      if (live.current && ticket === request.current) {
        setError(errorText(cause)); setBlocked(true); setConfirm(undefined);
        // Access denial clears all private content, including drafts.
        if (cause instanceof HttpError && [401,403].includes(cause.status)) { setView(undefined); setEdit(undefined); }
      }
    } finally { saving.current = false; if (live.current && ticket === request.current) setBusy(false); }
  };
  const disabled = busy || loading || blocked;
  return <section className="sp-agent-memory" aria-label={`Ditt minne hos ${agent.displayName}`} aria-busy={loading || busy}>
    {loading && <Status>Hentar minnet …</Status>}
    {error && <Status tone="error">{error}</Status>}
    {notice && <Status>{notice}</Status>}
    <Button variant="quiet" disabled={busy || loading} onClick={() => void load()}>Hent minnet på nytt</Button>
    {view && <>
      <label className="sp-circle-agent-enabled"><input type="checkbox" checked={view.enabled} disabled={disabled}
        onChange={event => void mutate(event.target.checked)} />Tillat minne om meg</label>
      {!view.collectionAvailable ? <Status>Læring er ikkje aktivert enno. Valet blir lagra, men ingen meldingar blir samla inn eller brukte som minne.</Status>
        : !view.enabled ? <Status>Minne er av for deg. Lagrede notat kan framleis lesast og slettast.</Status>
        : !view.agentEnabled ? <Status>Minne er sett på pause for denne agenten.</Status>
        : !view.collectionStartedAt ? <Status>Valet er lagra. Innsamling har ikkje starta enno.</Status>
        : <Status>Minne er tillate frå {new Date(view.collectionStartedAt * 1000).toLocaleString("nn-NO")}.</Status>}
      {!view.notes.length && <p>Ingen synlege minnenotat om deg enno.</p>}
      {view.notes.map(note => <article key={note.id} className="sp-memory-note" aria-label="Minnenotat">
        <div className="sp-memory-meta">{channels.find(channel => channel.id === note.channelId)?.name ?? "Kanal"} · {({ preference: "Preferanse", temporary_context: "Mellombels", interaction: "Samspel" })[note.kind]}</div>
        {edit?.id === note.id ? <form onSubmit={event => { event.preventDefault(); if (validMemoryText(edit.text)) void mutate({ action: "correct", note_id: note.id, text: edit.text.trim() }); }}>
          <label htmlFor={`memory-edit-${note.id}`}>Rett minnenotatet</label>
          <textarea id={`memory-edit-${note.id}`} value={edit.text} disabled={busy || loading} rows={3} onChange={event => setEdit({ id: note.id, text: event.target.value })} aria-describedby={`memory-limit-${note.id}`} />
          <small id={`memory-limit-${note.id}`}>{memoryTextBytes(edit.text)} / 1024 byte</small>
          <div className="sp-memory-actions"><Button type="submit" disabled={disabled || !validMemoryText(edit.text)}>Lagre retting</Button>
            <Button variant="quiet" disabled={busy} onClick={() => setEdit(undefined)}>Avbryt retting</Button></div>
        </form> : <p className="sp-memory-text">{note.text}</p>}
        <div className="sp-memory-meta">{note.evidence === "user_confirmed" ? "Stadfesta av deg" : note.origin === "user" ? "Retta av deg" : "Samla frå samtalen"} · <time dateTime={new Date(note.updatedAt * 1000).toISOString()}>{new Date(note.updatedAt * 1000).toLocaleDateString("nn-NO")}</time>
          {note.expiresAt !== null && <> · Utløper {new Date(note.expiresAt * 1000).toLocaleDateString("nn-NO")}</>}</div>
        <details><summary>Kjelder ({note.sourceMessageIds.length})</summary><ul>{note.sourceMessageIds.map((id, index) => <li key={id}>
          <a href={`/?channel=${encodeURIComponent(note.channelId)}&message=${encodeURIComponent(id)}`}>Kjeldemelding {index + 1}</a></li>)}</ul></details>
        <div className="sp-memory-actions">
          <Button variant="quiet" disabled={disabled || !!edit} onClick={() => { setEdit({ id: note.id, text: note.text }); setConfirm(undefined); }}>Rett</Button>
          <Button variant="quiet" disabled={disabled || !!edit || note.evidence === "user_confirmed"} onClick={() => void mutate({ action: "confirm", note_id: note.id })}>Stadfest</Button>
          <Button variant="quiet" disabled={disabled || !!edit} onClick={() => setConfirm({ action: "forget", note_id: note.id })}>Gløym</Button>
        </div>
      </article>)}
      {view.unavailableNotes > 0 && <p>{view.unavailableNotes} notat er utilgjengelege eller utgåtte. Innhald og kjelder er skjulte. Nullstilling fjernar også desse.</p>}
      <p>Gløyming fjernar minnenotat og sperrar kjeldene frå ny læring. Originalmeldingar og svar som alt er publiserte blir ståande.</p>
      <Button variant="danger" disabled={disabled || !!edit} onClick={() => setConfirm({ action: "reset" })}>Nullstill mitt minne hos {agent.displayName}</Button>
      {confirm && <div className="sp-memory-confirm" role="group" aria-label="Stadfest gløyming">
        <p>{confirm.action === "reset" ? "Fjern alle notat, også skjulte, og start med ei ny grense for innsamling? Minnevalet ditt blir ståande."
          : "Gløym dette notatet og andre notat som byggjer på dei same kjeldene?"}</p>
        <div className="sp-memory-actions"><Button variant="quiet" disabled={busy} onClick={() => setConfirm(undefined)}>Avbryt</Button>
          <Button variant="danger" disabled={disabled} onClick={() => void mutate(confirm)}>Ja, gløym</Button></div>
      </div>}
    </>}
  </section>;
}

export function AgentMemoryDialog({ api, circleId, circleName, channels, onClose }: {
  api: AgentMemoryApi; circleId: string; circleName: string; channels: readonly Readonly<Channel>[]; onClose(): void;
}) {
  const [agents, setAgents] = useState<MemoryAgent[]>([]);
  const [selected, setSelected] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController(); let live = true;
    setLoading(true); setAgents([]); setSelected(""); setError("");
    void api.list(circleId, controller.signal).then(items => { if (live) { setAgents(items); setSelected(items[0]?.agentId ?? ""); } })
      .catch(cause => { if (live) setError(errorText(cause)); }).finally(() => { if (live) setLoading(false); });
    return () => { live = false; controller.abort(); };
  }, [api, circleId, attempt]);
  const agent = agents.find(item => item.agentId === selected);
  return <Dialog open title="Mitt agentminne" closeLabel="Lukk" onClose={onClose}>
    <div className="sp-memory-meta">{circleName}</div>
    <p>Dette er agenten sitt minne om deg i denne kretsen. Andre medlemmer og moderatorar kan ikkje opne det.</p>
    {loading && <Status>Hentar agentar …</Status>}
    {error && <><Status tone="error">{error}</Status><Button onClick={() => setAttempt(value => value + 1)}>Prøv igjen</Button></>}
    {!loading && !error && !agents.length && <Status>Ingen agentar i denne kretsen enno.</Status>}
    {agents.length > 0 && <div className="sp-agent-memory"><label htmlFor="memory-agent">Agent</label>
      <select id="memory-agent" value={selected} onChange={event => setSelected(event.target.value)}>{agents.map(item => <option key={item.agentId} value={item.agentId}>{item.displayName}</option>)}</select>
      {agent && <MemoryPanel key={agent.agentId} api={api} circleId={circleId} agent={agent} channels={channels} />}
    </div>}
  </Dialog>;
}
