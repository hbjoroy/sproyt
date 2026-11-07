import { Button, Status } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { CircleProcess, CircleProcessApi, ProcessStep } from "../../circle-processes";
import css from "./circle-processes.css";

const statuses: Readonly<Record<string, string>> = {
  starting: "Startar", pending: "Ventande", leased: "Under arbeid", active: "Under arbeid", waiting: "Ventar på svar",
  running: "Under arbeid", completed: "Fullført", cancelled: "Avbroten", canceled: "Avbroten", terminated: "Avslutta",
  failed: "Treng oppfølging", blocked: "Blokkert", ready: "Klar", skipped: "Hoppa over", sending: "Sender",
  uncertain: "Uavklart resultat", sent: "Sendt", new: "Ny", reviewing: "Til vurdering", needs_information: "Treng informasjon",
  planned: "Planlagd", in_development: "Under utvikling", resolved: "Løyst", rejected: "Avvist", duplicate: "Duplikat"
};
const statusName = (status: string) => statuses[status] ?? "Ukjend status";
function StepList({ items }: { items: readonly ProcessStep[] }) {
  return <ul className="sp-circle-process-steps">{items.map((step, index) => <li key={index}><span>{step.name}</span><span>{statusName(step.status)}</span></li>)}</ul>;
}
function ProcessRow({ process }: { process: CircleProcess }) {
  return <details className="sp-circle-process">
    <summary><span><strong>{process.title}</strong><span className="sp-circle-process-channel"># {process.channel_name}</span></span>
      <span className="sp-circle-process-status">{statusName(process.status)}</span></summary>
    <div className="sp-circle-process-body">
      <p className="sp-help">{process.projection ? "Sist registrerte status" : "Status"}: {statusName(process.status)}.</p>
      <h4>Registrerte steg</h4>
      {process.steps.length ? <StepList items={process.steps} /> : <p>Ingen steg er registrerte enno.</p>}
      {process.steps_truncated && <p className="sp-help">Dette er eit utdrag av dei registrerte stega.</p>}
      <h4>Attståande steg</h4>
      {process.remaining_steps === null ? <p>Heart opplyser ikkje om attståande steg.</p>
        : process.remaining_steps.length ? <StepList items={process.remaining_steps} /> : <p>Ingen attståande steg.</p>}
      <h4>Historikk</h4>
      {process.history.length ? <ol className="sp-circle-process-history">{process.history.map((event, index) => <li key={index}>
        <span>{event.name}</span><span>{statusName(event.status)}</span>
        {event.occurred_at && !Number.isNaN(Date.parse(event.occurred_at)) && <time dateTime={event.occurred_at}>{new Intl.DateTimeFormat("nb-NO", { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit", hourCycle: "h23" }).format(new Date(event.occurred_at))}</time>}
      </li>)}</ol> : <p>Ingen historikk er registrert enno.</p>}
      {process.history_truncated && <p className="sp-help">Dette er eit utdrag av historikken.</p>}
    </div>
  </details>;
}

export function CircleProcesses({ api, circleId, owner, admitted }: {
  api: CircleProcessApi; circleId: string; owner: string; admitted: () => boolean;
}) {
  const [all, setAll] = useState(false), [revision, setRevision] = useState(0);
  const [items, setItems] = useState<readonly CircleProcess[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(true), [error, setError] = useState("");
  const scope = `${owner}:${circleId}:${all}`;
  const current = useRef({ scope, admitted }); current.current = { scope, admitted };
  const request = useRef<AbortController | null>(null);
  const visibleScope = useRef(scope);
  const load = (after: string | null) => {
    request.current?.abort();
    const controller = new AbortController(); request.current = controller;
    const live = () => request.current === controller && current.current.scope === scope && current.current.admitted();
    setLoading(true); setError("");
    if (!after) { setItems([]); setCursor(null); visibleScope.current = scope; }
    void api.list(circleId, owner, all ? "all" : "open", after, controller.signal, live).then(page => {
      if (!live()) return;
      setItems(previous => after ? [...new Map([...previous, ...page.processes].map(item => [item.id, item])).values()] : page.processes);
      setCursor(page.next_cursor);
    }, cause => { if (live() && !controller.signal.aborted) {
      setItems([]); setCursor(null);
      setError(cause instanceof Error ? cause.message : "Kunne ikkje hente prosessane.");
    } })
      .finally(() => { if (live()) setLoading(false); });
  };
  useEffect(() => { load(null); return () => { request.current?.abort(); request.current = null; }; }, [api, scope, revision]);
  const visible = visibleScope.current === scope && admitted();
  return <section className="sp-circle-processes" aria-label="Prosessar">
    <style>{css}</style><h3>Prosessar</h3><p>Opne prosessar og oppgåver i kanalane du har tilgang til i denne kretsen.</p>
    <div className="sp-circle-process-controls"><label><input type="checkbox" checked={all} onChange={event => setAll(event.target.checked)} /> Vis òg avslutta prosessar</label>
      <Button variant="quiet" busy={loading} onClick={() => setRevision(value => value + 1)}>Oppdater prosessar</Button></div>
    {loading && <Status>Hentar prosessar …</Status>}
    {error && <Status tone="error">{error} <Button onClick={() => cursor ? load(cursor) : setRevision(value => value + 1)}>Prøv igjen</Button></Status>}
    {visible && items.map(process => <ProcessRow key={process.id} process={process} />)}
    {visible && !loading && !error && !items.length && <p>{all ? "Ingen prosessar er registrerte i kanalane du har tilgang til." : "Ingen opne prosessar i kanalane du har tilgang til."}</p>}
    {visible && cursor && !error && <Button busy={loading} onClick={() => load(cursor)}>Vis fleire prosessar</Button>}
  </section>;
}
