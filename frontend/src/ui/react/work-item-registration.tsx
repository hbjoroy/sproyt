import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useEffect, useState } from "react";
import type { ChatMessage } from "../../types";
import type { WorkApplication, WorkItemApi, WorkItemDraft, WorkItemReceipt } from "../../work-items";

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
  useEffect(() => {
    if (!open) return;
    let active = true;
    setError(""); setReceipt(undefined); setDraft(undefined);
    void Promise.all([api.applications(message.channel_id), api.draft(message.channel_id, message.id)])
      .then(([apps, next]) => {
        if (!active) return;
        setApplications(apps); setApplicationId(apps[0]?.id ?? "");
        setDraft(next); setTitle(next.title); setDescription(next.source_body);
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
  return <Dialog open={open} title="Lag Issue" closeLabel="Lukk saksregistrering" onClose={onClose}>
    {receipt ? <div className="sp-work-item-form"><Status>Sak {receipt.id.slice(0, 8)} er registrert. Heart-start: {receipt.start_status === "started" ? "starta" : "ventar"}.</Status>
      <Button onClick={onClose}>Lukk</Button></div> : <form className="sp-work-item-form" onSubmit={event => { event.preventDefault(); void submit(); }}>
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
  </Dialog>;
}
