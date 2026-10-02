import { Button, Status } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { GithubExport, WorkItemApi, WorkItemTask } from "../../work-items";

function safeIssueUrl(value: string | null | undefined): string | null {
  if (!value) return null;
  try { return new URL(value).protocol === "https:" ? value : null; }
  catch { return null; }
}

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
  const [githubTitle, setGithubTitle] = useState("");
  const [githubBody, setGithubBody] = useState("");
  const [githubRetry, setGithubRetry] = useState<{ title: string; body: string; send: boolean; expected_repository_id: number | null; expected_binding_revision: number | null } | null>(null);
  const [githubTarget, setGithubTarget] = useState<Pick<GithubExport, "repository" | "repository_id" | "binding_revision"> | null>(null);
  const githubDraftTouched = useRef(false);
  const githubDraftInitialized = useRef(false);
  useEffect(() => {
    const controller = new AbortController();
    let initial = true;
    const refresh = () => void api.task(taskId, messageId, controller.signal)
      .then(value => { if (!controller.signal.aborted) {
        if (initial) { setCategory(value.category ?? "bug"); setPriority(value.priority ?? "untriaged"); initial = false; }
        if (value.node_id === "publish-github" && !githubDraftInitialized.current) {
          setGithubTarget({ repository: value.github_export?.repository ?? null,
            repository_id: value.github_export?.repository_id ?? null,
            binding_revision: value.github_export?.binding_revision ?? null });
          const pending = api.pendingGithubExport(value);
          if (pending) { setGithubTitle(pending.title); setGithubBody(pending.body); setGithubRetry(pending); }
          else if (!githubDraftTouched.current) {
            setGithubTitle(value.title);
            setGithubBody(value.description);
          }
          githubDraftInitialized.current = true;
        }
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
      task.node_id === "provide-information" ? "" : decision,
      task.node_id === "provide-information" || decision === "needs_information" ? note : "")); }
    catch (cause) { setError(cause instanceof Error ? cause.message : "Kunne ikkje lagre avgjerda."); }
    finally { setSaving(false); }
  };
  const exportGithub = async (send: boolean) => {
    if (!task || saving || !task.can_decide || !task.github_export) return;
    const pending = githubRetry ?? api.pendingGithubExport(task);
    const title = pending?.title ?? (send ? githubTitle : "");
    const body = pending?.body ?? (send ? githubBody : "");
    const actualSend = pending?.send ?? send;
    setSaving(true); setError("");
    try {
      const result = await api.exportGithub(task, title, body, actualSend,
        pending ? { repository_id: pending.expected_repository_id, binding_revision: pending.expected_binding_revision }
          : githubTarget ?? { repository_id: null, binding_revision: null });
      setTask(result); setGithubRetry(null);
    } catch (cause) {
      setGithubRetry(api.pendingGithubExport(task));
      setError(cause instanceof Error ? cause.message : "Kunne ikkje lese svaret frå GitHub-innsendinga.");
    } finally { setSaving(false); }
  };
  if (!task) return <div className="sp-work-item-task"><Status tone={error ? "error" : undefined}>{error || "Hentar behandlaroppgåva …"}</Status></div>;
  const information = task.node_id === "provide-information";
  const githubTask = task.node_id === "publish-github";
  const taskLabel = githubTask ? "Send til GitHub" : information ? "Svar på spørsmål" : task.node_id === "followup-review" ? "Vurder etter svar" : "Vurder saka";
  const githubState = task.github_export?.status === "sent" ? "Sendt til GitHub"
    : task.github_export?.status === "skipped" ? "Berre internt"
    : task.github_export?.status === "uncertain" ? "Uvisst om GitHub tok imot saka"
    : task.github_export?.status === "sending" ? "Sender til GitHub"
    : task.github_export?.status === "pending" ? "Ventar på GitHub-innsending"
    : task.github_export?.status === "blocked" ? "GitHub-innsending blokkert"
    : taskLabel;
  const issueUrl = safeIssueUrl(task.github_export?.issue_url);
  const githubTargetChanged = githubTask && githubTarget && (githubTarget.repository !== task.github_export?.repository
    || githubTarget.repository_id !== task.github_export?.repository_id
    || githubTarget.binding_revision !== task.github_export?.binding_revision);
  const state = task.process_status === "failed" ? "Prosessen feila" : task.process_status === "cancelled" ? "Prosessen er avbroten"
    : githubTask && task.github_export?.status !== "ready" ? githubState
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
      {githubTask && task.github_export ? <>
        {issueUrl && task.github_export.status === "sent" && <p><a href={issueUrl} target="_blank" rel="noopener noreferrer">Opne GitHub-saka</a></p>}
        {task.github_export.status === "uncertain" && <Status tone="error">Det er uvisst om GitHub tok imot saka. Vent på avklaring; ikkje send ei ny sak.</Status>}
        {task.github_export.status === "blocked" && <Status tone="error">Eksporten er blokkert. Kretsansvarleg må sjekke GitHub-oppsettet og tilgangen.</Status>}
        {task.github_export.status === "pending" && <Status>Avgjerda er lagra og ventar på innsending.</Status>}
        {task.github_export.status === "sending" && <Status>GitHub-innsending pågår.</Status>}
        {task.can_decide && task.github_export.status === "ready" ? <form onSubmit={event => { event.preventDefault(); void exportGithub(true); }}>
          {githubTarget?.repository && <p>Offentleg mål: <strong>{githubTarget.repository}</strong></p>}
          {!githubTarget?.repository && <Status tone="error">GitHub-repositoriet er ikkje valt.</Status>}
          {githubTargetChanged && <Status tone="error">GitHub-målet er endra sidan du opna oppgåva. Last sida på nytt og les gjennom målet før du sender.</Status>}
          <p>Tittel og tekst under blir offentlege i GitHub. Les gjennom før du sender.</p>
          <label>Tittel<input required maxLength={160} value={githubTitle} disabled={saving || !!githubRetry} onChange={event => { githubDraftTouched.current = true; setGithubTitle(event.currentTarget.value); }} /></label>
          <label>Tekst<textarea required maxLength={8000} rows={6} value={githubBody} disabled={saving || !!githubRetry} onChange={event => { githubDraftTouched.current = true; setGithubBody(event.currentTarget.value); }} /></label>
          {githubRetry && <Status tone="error">Førre svar er ukjent. Same innsending kan prøvast igjen med same innhald.</Status>}
          <Button type="submit" disabled={saving || !!githubTargetChanged || !task.github_export.can_publish || !githubTarget?.repository || !githubTitle.trim() || !!githubRetry && !githubRetry.send} busy={saving}>{githubRetry?.send ? "Prøv same innsending igjen" : "Send til GitHub"}</Button>
          <Button type="button" variant="secondary" disabled={saving || !!githubRetry && githubRetry.send} onClick={() => void exportGithub(false)}>{githubRetry && !githubRetry.send ? "Prøv same val igjen" : "Berre internt"}</Button>
        </form> : task.github_export.status === "ready" && <Status>{state}</Status>}
      </> : task.can_decide ? <form onSubmit={event => { event.preventDefault(); void save(); }}>
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
