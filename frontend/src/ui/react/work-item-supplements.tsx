import { Button, Status } from "@sproyt/ui/react";
import { useState } from "react";
import { HttpError } from "../../api";
import type { SourceWorkItem, WorkItemApi, WorkItemSupplement } from "../../work-items";
import { validSupplementBody } from "../../work-items";

function supplementTime(value: string): string {
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? new Intl.DateTimeFormat("nn-NO", { dateStyle: "short", timeStyle: "short" }).format(date) : "Ukjent tidspunkt";
}

export function SupplementHistory({ supplements }: { supplements: readonly WorkItemSupplement[] }) {
  if (!supplements.length) return null;
  return <div className="sp-work-item-information"><strong>Tilleggsinformasjon</strong>
    <ol>{supplements.map(entry => <li key={entry.id}><strong>{entry.actor_name}</strong> <time dateTime={entry.created_at}>{supplementTime(entry.created_at)}</time><p>{entry.body}</p></li>)}</ol>
  </div>;
}

export function WorkItemSupplementForm({ api, item, channelId, onSaved, onCancel }: {
  api: WorkItemApi; item: SourceWorkItem; channelId: string; onSaved: (saved: SourceWorkItem) => void; onCancel: () => void;
}) {
  const [current, setCurrent] = useState(item);
  const [retry, setRetry] = useState(() => api.pendingSupplement(item));
  const [body, setBody] = useState(() => api.pendingSupplement(item) ?? "");
  const [error, setError] = useState("");
  const [conflict, setConflict] = useState(false);
  const [busy, setBusy] = useState(false);
  const refresh = async () => {
    setBusy(true); setError("");
    try {
      const next = (await api.sourceItems(channelId, item.source_message_id)).find(value => value.id === item.id);
      if (!next) throw new Error("Saka er ikkje tilgjengeleg her no.");
      setCurrent(next); setConflict(false);
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Kunne ikkje hente saka."); }
    finally { setBusy(false); }
  };
  const submit = async () => {
    if (busy || conflict || !current.can_supplement && retry === null || !validSupplementBody(retry ?? body)) return;
    setBusy(true); setError("");
    try { onSaved(await api.supplement(current, retry ?? body)); }
    catch (cause) {
      setRetry(api.pendingSupplement(current)); setConflict(cause instanceof HttpError && cause.status === 409);
      setError(cause instanceof Error ? cause.message : "Kunne ikkje leggje til informasjonen.");
    } finally { setBusy(false); }
  };
  return <form className="sp-work-item-form" onSubmit={event => { event.preventDefault(); void submit(); }}>
    <h3>Legg til informasjon: {current.title}</h3>
    <SupplementHistory supplements={current.supplements} />
    <label>Ny informasjon<textarea rows={4} maxLength={8000} value={body} disabled={busy || retry !== null} onChange={event => setBody(event.currentTarget.value)} /></label>
    <small>Informasjonen blir lagd til saka. Tidlegare tekst og informasjon blir ståande.</small>
    {body.trim() && !validSupplementBody(body) && <Status tone="error">Teksten er for lang. Kort han ned.</Status>}
    {retry !== null && <Status>Førre svar er uklart. Prøv same informasjon igjen før du endrar teksten.</Status>}
    {!current.can_supplement && retry === null && <Status>Saka tek ikkje imot meir informasjon no.</Status>}
    {error && <Status tone="error">{error}</Status>}
    {conflict && <Button type="button" disabled={busy} onClick={() => void refresh()}>Hent saka på nytt</Button>}
    <div className="sp-row"><Button type="button" disabled={busy} onClick={onCancel}>Avbryt</Button>
      <Button type="submit" disabled={busy || conflict || !current.can_supplement && retry === null || !validSupplementBody(retry ?? body)} busy={busy}>{retry !== null ? "Prøv same informasjon igjen" : "Legg til informasjon"}</Button></div>
  </form>;
}
