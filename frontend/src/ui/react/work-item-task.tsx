import { Button, Status } from "@sproyt/ui/react";
import { useEffect, useState } from "react";
import type { WorkItemApi, WorkItemTask } from "../../work-items";

export function WorkItemTaskMessage({ api, taskId, messageId }: {
  readonly api: WorkItemApi; readonly taskId: string; readonly messageId: string;
}) {
  const [task, setTask] = useState<WorkItemTask>();
  const [error, setError] = useState("");
  const [expanded, setExpanded] = useState(false);
  const [category, setCategory] = useState("bug");
  const [priority, setPriority] = useState("untriaged");
  const [decision, setDecision] = useState("planned");
  const [note, setNote] = useState("");
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    let initial = true;
    const refresh = () => void api.task(taskId, messageId, controller.signal)
      .then(value => { if (!controller.signal.aborted) {
        if (initial) { setCategory(value.category ?? "bug"); setPriority(value.priority ?? "untriaged"); initial = false; }
        setTask(value); setError("");
      } })
      .catch(cause => { if (!controller.signal.aborted) setError(cause instanceof Error ? cause.message : "Kunne ikkje lese oppgåva."); });
    refresh();
    const timer = window.setInterval(refresh, 5000);
    return () => { controller.abort(); window.clearInterval(timer); };
  }, [api, taskId, messageId]);
  const save = async () => {
    if (!task || saving || !task.can_decide) return;
    setSaving(true); setError("");
    try { setTask(await api.decide(task, task.node_id === "provide-information" ? "" : category,
      task.node_id === "provide-information" ? "" : priority,
      task.node_id === "provide-information" ? "" : decision, note)); }
    catch (cause) { setError(cause instanceof Error ? cause.message : "Kunne ikkje lagre avgjerda."); }
    finally { setSaving(false); }
  };
  if (!task) return <div className="sp-work-item-task"><Status tone={error ? "error" : undefined}>{error || "Hentar behandlaroppgåva …"}</Status></div>;
  const information = task.node_id === "provide-information";
  const taskLabel = information ? "Svar på spørsmål" : task.node_id === "followup-review" ? "Vurder etter svar" : "Vurder saka";
  const state = task.process_status === "failed" ? "Prosessen feila" : task.process_status === "cancelled" ? "Prosessen er avbroten"
    : task.status === "completed" ? "Fullført" : task.status === "cancelled" ? "Avbroten"
    : task.delivery_status === "failed" ? "Avgjerda kunne ikkje leverast"
    : task.blocked ? "Blokkert: rett eller kanaltilgang manglar"
    : task.delivery_status === "pending" ? "Innsending lagra · ventar på Heart" : taskLabel;
  return <section className="sp-work-item-task" aria-label={`${taskLabel}: ${task.title}`}>
    <Button className="sp-work-item-task-summary" variant="quiet" aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>
      <span aria-hidden="true">{expanded ? "▾" : "▸"}</span>
      <span><strong>{task.title}</strong><small>{task.application_name} · {state} · {task.assignee_name}</small></span>
    </Button>
    {expanded && <div className="sp-work-item-task-details">
      <p>{task.description}</p>
      {task.information_request && <div className="sp-work-item-information"><strong>Spørsmål frå behandlar</strong><p>{task.information_request}</p></div>}
      {task.information_response && <div className="sp-work-item-information"><strong>Svar frå innmeldar</strong><p>{task.information_response}</p></div>}
      {task.blocked && <Status tone="error">Den tildelte personen manglar rett eller kanaltilgang. Kretsansvarleg må rette oppsettet.</Status>}
      {task.category && task.decision_status && <p>Kategori: {task.category} · Prioritet: {task.priority} · Status: {task.decision_status}</p>}
      {task.can_decide ? <form onSubmit={event => { event.preventDefault(); void save(); }}>
        {information ? <label>Svar til behandlar<textarea required maxLength={8000} rows={3} value={note} onChange={event => setNote(event.currentTarget.value)} /></label> : <>
        <label>Kategori<select value={category} onChange={event => setCategory(event.currentTarget.value)}>
          <option value="bug">Feil</option><option value="change">Endringsønske</option><option value="question">Spørsmål/anna</option>
        </select></label>
        <label>Prioritet<select value={priority} onChange={event => setPriority(event.currentTarget.value)}>
          <option value="untriaged">Uavklart</option><option value="low">Låg</option><option value="normal">Normal</option>
          <option value="high">Høg</option><option value="critical">Kritisk</option>
        </select></label>
        <label>Avgjerd<select value={decision} onChange={event => setDecision(event.currentTarget.value)}>
          <option value="planned">Planlagt</option>
          {task.can_request_information && <option value="needs_information">Be om informasjon</option>}
          <option value="resolved">Løyst</option><option value="rejected">Avvist</option>
        </select></label>
        {decision === "needs_information" && <label>Spørsmål til innmeldar<textarea required maxLength={8000} rows={3} value={note} onChange={event => setNote(event.currentTarget.value)} /></label>}
        </>}
        <Button type="submit" disabled={saving || ((information || decision === "needs_information") && !note.trim())} busy={saving}>{information ? "Send svar" : decision === "needs_information" ? "Send spørsmål" : "Lagre avgjerd"}</Button>
      </form> : <Status>{task.status === "pending" && task.delivery_status === "ready" ? "Berre den tildelte personen kan sende inn denne oppgåva." : state}</Status>}
      {error && <Status tone="error">{error}</Status>}
    </div>}
  </section>;
}
