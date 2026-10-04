import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useEffect, useState } from "react";
import type { ChatMessage } from "../../types";
import type { SourceWorkItem, WorkApplication, WorkItemApi, WorkItemDraft, WorkItemReceipt } from "../../work-items";
import { SupplementHistory, WorkItemSupplementForm } from "./work-item-supplements";

const sourceStatus = (value: string) => ({ new: "Til vurdering", reviewing: "Til vurdering", needs_information: "Ventar på informasjon",
  planned: "Planlagt", in_development: "Under utvikling", resolved: "Løyst", rejected: "Avvist" } as Record<string, string>)[value] ?? value;

export function WorkItemRegistration({ api, message, open, onClose }: {
  readonly api: WorkItemApi; readonly message: ChatMessage; readonly open: boolean; readonly onClose: () => void;
}) {
  const [applications, setApplications] = useState<readonly WorkApplication[]>([]);
  const [draft, setDraft] = useState<WorkItemDraft>();
  const [applicationId, setApplicationId] = useState("");
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const [receipt, setReceipt] = useState<WorkItemReceipt>();
  const [items, setItems] = useState<SourceWorkItem[]>([]);
  const [selectedItem, setSelectedItem] = useState<SourceWorkItem | null>(null);
  const [registerNew, setRegisterNew] = useState(false);
  const [supplementSaved, setSupplementSaved] = useState(false);
  const [sourceLoaded, setSourceLoaded] = useState(false);
  useEffect(() => {
    if (!open) return;
    let active = true;
    setError(""); setReceipt(undefined); setDraft(undefined); setSourceLoaded(false);
    void Promise.all([api.applications(message.channel_id).catch(() => []), api.sourceItems(message.channel_id, message.id)])
      .then(async ([apps, source]) => {
        if (!active) return;
        setApplications(apps); setApplicationId(apps[0]?.id ?? "");
        setItems(source); setRegisterNew(source.length === 0); setSourceLoaded(true);
        if (source.length === 0 && apps.length > 0) {
          const next = await api.draft(message.channel_id, message.id);
          if (active) { setDraft(next); setTitle(next.title); setDescription(next.source_body); }
        }
      }).catch(cause => { if (active) setError(cause instanceof Error ? cause.message : "Kunne ikkje opne saksutkastet."); });
    return () => { active = false; };
  }, [api, message.channel_id, message.id, open]);
  const submit = async () => {
    if (!draft || !applicationId || pending) return;
    setPending(true); setError("");
    try { setReceipt(await api.register(message.channel_id, message.id, applicationId, title.trim(), description.trim(), draft.source_body)); }
    catch (cause) { setError(cause instanceof Error ? cause.message : "Kunne ikkje registrere saka."); }
    finally { setPending(false); }
  };
  return <Dialog open={open} title="Arbeidssaker" closeLabel="Lukk saksregistrering" onClose={onClose}>
    {selectedItem ? <WorkItemSupplementForm key={selectedItem.id} api={api} item={selectedItem} channelId={message.channel_id}
      onCancel={() => setSelectedItem(null)} onSaved={saved => { setItems(previous => previous.map(item => item.id === saved.id ? saved : item)); setSelectedItem(null); setSupplementSaved(true); }} /> : <>
    {items.length > 0 && <div className="sp-work-item-form"><h3>Registrerte saker frå meldinga</h3>
      {items.map(item => <section key={item.id}><strong>{item.title}</strong><p>{item.application_name} · {sourceStatus(item.status)}</p>
        <div className="sp-work-item-information"><p>{item.description}</p></div>
        <SupplementHistory supplements={item.supplements} />
        {(item.can_supplement || api.pendingSupplement(item) !== null) && <Button onClick={() => { setSelectedItem(item); setSupplementSaved(false); }}>
          {api.pendingSupplement(item) !== null ? "Prøv same informasjon igjen" : "Legg til informasjon"}</Button>}
      </section>)}
      {supplementSaved && <Status>Informasjonen er lagd til saka.</Status>}
      {!registerNew && applications.length > 0 && <Button variant="quiet" onClick={() => {
        setRegisterNew(true);
        if (!draft) void api.draft(message.channel_id, message.id).then(next => { setDraft(next); setTitle(next.title); setDescription(next.source_body); })
          .catch(cause => setError(cause instanceof Error ? cause.message : "Kunne ikkje lage saksutkast."));
      }}>Registrer ei ny sak</Button>}
    </div>}
    {receipt ? <div className="sp-work-item-form"><Status>Saka er registrert og ventar på vurdering.</Status>
      <Button onClick={onClose}>Lukk</Button></div> : registerNew && <form className="sp-work-item-form" onSubmit={event => { event.preventDefault(); void submit(); }}>
      <p>Opprett ei sak frå denne meldinga. Du kan rette tittelen og beskrivinga før registrering.</p>
      <label>Applikasjon<select value={applicationId} required onChange={event => setApplicationId(event.currentTarget.value)}>
        {applications.map(app => <option key={app.id} value={app.id}>{app.name}</option>)}
      </select></label>
      <label>Tittel<input value={title} maxLength={160} required onChange={event => setTitle(event.currentTarget.value)} /></label>
      {draft && <small>{draft.suggested_by_model ? "Tittelforslag frå Santorini; du kan endre det." : "Automatisk tittel var ikkje tilgjengeleg. Kontroller tittelen."}</small>}
      <label>Beskriving<textarea value={description} rows={5} maxLength={8000} required onChange={event => setDescription(event.currentTarget.value)} /></label>
      {error && <Status tone="error">{error}</Status>}
      <Button type="submit" disabled={!draft || !applicationId || !title.trim() || !description.trim() || pending} busy={pending}>Registrer</Button>
    </form>}
    {!registerNew && error && <Status tone="error">{error}</Status>}
    {!sourceLoaded && !error && <Status>Hentar registrerte saker …</Status>}
    </>}
  </Dialog>;
}
