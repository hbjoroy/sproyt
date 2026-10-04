import { Button, Status } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { GithubExport, StatusChangeReceipt, WorkItemApi, WorkItemTask } from "../../work-items";
import { githubWorkItemDraft } from "../../work-items";
import { HttpError } from "../../api";
import { SupplementHistory } from "./work-item-supplements";

const statusName = (value: string): string => ({ planned: "Planlagt", in_development: "Under utvikling",
  resolved: "Løyst", rejected: "Avvist" })[value as "planned" | "in_development" | "resolved" | "rejected"] ?? value;

function safeIssueUrl(value: string | null | undefined): string | null {
  if (!value) return null;
  try { return new URL(value).protocol === "https:" ? value : null; }
  catch { return null; }
}

export function WorkItemTaskMessage({ api, taskId, messageId }: {
  readonly api: WorkItemApi; readonly taskId: string; readonly messageId: string;
}) {
  const [task, setTask] = useState<WorkItemTask>();
  const baseline = useRef<WorkItemTask | undefined>(undefined);
  const observedRevision = useRef(0);
  const [newTask, setNewTask] = useState<WorkItemTask>();
  const [decisionRetry, setDecisionRetry] = useState<ReturnType<WorkItemApi["pendingDecision"]>>(null);
  const acceptTask = (value: WorkItemTask) => { observedRevision.current = Math.max(observedRevision.current, value.revision); baseline.current = value; setTask(value); setNewTask(undefined); };
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
  const [statusChoice, setStatusChoice] = useState("");
  const [internalNote, setInternalNote] = useState("");
  const [publicFeedback, setPublicFeedback] = useState("");
  const [statusRetry, setStatusRetry] = useState<{ status: string; internal_note: string; public_feedback: string; no_change: boolean } | null>(null);
  const [startReceipt, setStartReceipt] = useState<StatusChangeReceipt | null>(null);
  const [startingStatus, setStartingStatus] = useState(false);
  const statusDraftInitialized = useRef(false);
  const statusDraftTouched = useRef(false);
  useEffect(() => {
    const controller = new AbortController();
    let initial = true;
    const refresh = () => void api.task(taskId, messageId, controller.signal)
      .then(value => { if (!controller.signal.aborted) {
        if (value.revision < observedRevision.current) return;
        observedRevision.current = value.revision;
        if (initial) {
          const pending = api.pendingDecision(value);
          setCategory(pending?.category || value.category || "bug"); setPriority(pending?.priority || value.priority || "untriaged");
          if (pending) { setDecision(pending.status); setNote(pending.note); setDecisionRetry(pending); }
          initial = false;
        }
        if (value.node_id === "publish-github" && !githubDraftInitialized.current) {
          setGithubTarget({ repository: value.github_export?.repository ?? null,
            repository_id: value.github_export?.repository_id ?? null,
            binding_revision: value.github_export?.binding_revision ?? null });
          const pending = api.pendingGithubExport(value);
          if (pending) { setGithubTitle(pending.title); setGithubBody(pending.body); setGithubRetry(pending); }
          else if (!githubDraftTouched.current) {
            setGithubTitle(value.github_export?.title ?? value.title);
            setGithubBody(value.github_export?.body ?? githubWorkItemDraft(value));
          }
          githubDraftInitialized.current = true;
        }
        if (value.node_id === "change-status" && !statusDraftInitialized.current) {
          const pending = api.pendingStatusChange(value);
          if (pending) {
            setStatusChoice(pending.status); setInternalNote(pending.internal_note);
            setPublicFeedback(pending.public_feedback); setStatusRetry(pending);
          } else if (!statusDraftTouched.current) setStatusChoice(value.lifecycle?.allowed_statuses[0] ?? "");
          statusDraftInitialized.current = true;
        }
        if (baseline.current?.status === "pending" && baseline.current.can_decide && value.revision !== baseline.current.revision) setNewTask(value);
        else acceptTask(value);
      } })
      .catch(cause => { if (!controller.signal.aborted) setError(cause instanceof Error ? cause.message : "Kunne ikkje lese oppgåva."); });
    refresh();
    const timer = window.setInterval(refresh, 5000);
    return () => { controller.abort(); window.clearInterval(timer); };
  }, [api, taskId, messageId]);
  const save = async () => {
    if (!task || saving || !task.can_decide && !decisionRetry || newTask && !decisionRetry) return;
    setSaving(true); setError("");
    try { acceptTask(await api.decide(task, task.node_id === "provide-information" ? "" : category,
      task.node_id === "provide-information" ? "" : priority,
      task.node_id === "provide-information" ? "" : decision,
      task.node_id === "provide-information" || decision === "needs_information" ? note : "")); setDecisionRetry(null); }
    catch (cause) {
      setDecisionRetry(api.pendingDecision(task));
      setError(cause instanceof Error ? cause.message : "Kunne ikkje lagre avgjerda.");
      if (cause instanceof HttpError && cause.status === 409) {
        try { setNewTask(await api.task(taskId, messageId)); } catch { /* keep draft and original failure */ }
      }
    }
    finally { setSaving(false); }
  };
  const exportGithub = async (send: boolean) => {
    if (!task || saving || !task.can_decide || !task.github_export || newTask && !githubRetry) return;
    const pending = githubRetry ?? api.pendingGithubExport(task);
    const title = pending?.title ?? (send ? githubTitle : "");
    const body = pending?.body ?? (send ? githubBody : "");
    const actualSend = pending?.send ?? send;
    setSaving(true); setError("");
    try {
      const result = await api.exportGithub(task, title, body, actualSend,
        pending ? { repository_id: pending.expected_repository_id, binding_revision: pending.expected_binding_revision }
          : githubTarget ?? { repository_id: null, binding_revision: null });
      acceptTask(result); setGithubRetry(null);
    } catch (cause) {
      setGithubRetry(api.pendingGithubExport(task));
      setError(cause instanceof Error ? cause.message : "Kunne ikkje lese svaret frå GitHub-innsendinga.");
    } finally { setSaving(false); }
  };
  const startStatus = async () => {
    if (!task || startingStatus || !task.lifecycle?.can_start) return;
    setStartingStatus(true); setError("");
    try { setStartReceipt(await api.startStatusChange(task)); }
    catch (cause) { setError(cause instanceof Error ? cause.message : "Kunne ikkje starte statusoppgåva."); }
    finally { setStartingStatus(false); }
  };
  const submitStatus = async (noChange: boolean) => {
    if (!task || saving || !task.can_decide || !task.lifecycle) return;
    const pending = statusRetry ?? api.pendingStatusChange(task);
    const actual = pending ?? { status: noChange ? "" : statusChoice, internal_note: noChange ? "" : internalNote,
      public_feedback: noChange ? "" : publicFeedback, no_change: noChange };
    setSaving(true); setError("");
    try {
      acceptTask(await api.changeStatus(task, actual.status, actual.internal_note, actual.public_feedback, actual.no_change));
      setStatusRetry(null);
    } catch (cause) {
      setStatusRetry(api.pendingStatusChange(task));
      setError(cause instanceof Error ? cause.message : "Kunne ikkje lese svaret frå statusendringa.");
    } finally { setSaving(false); }
  };
  if (!task) return <div className="sp-work-item-task"><Status tone={error ? "error" : undefined}>{error || "Hentar behandlaroppgåva …"}</Status></div>;
  const information = task.node_id === "provide-information";
  const githubTask = task.node_id === "publish-github";
  const statusTask = task.node_id === "change-status";
  const taskLabel = statusTask ? "Endre status" : githubTask ? "Send til GitHub" : information ? "Svar på spørsmål" : task.node_id === "followup-review" ? "Vurder etter svar" : "Vurder saka";
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
    : statusTask && task.delivery_status === "pending" ? "Status lagra · ventar på Heart"
    : task.status === "completed" ? "Fullført" : task.status === "cancelled" ? "Avbroten"
    : task.delivery_status === "failed" ? "Avgjerda kunne ikkje leverast"
    : task.blocked ? "Blokkert: rett eller kanaltilgang manglar"
    : task.delivery_status === "pending" ? "Innsending lagra · ventar på Heart" : taskLabel;
  return <section className="sp-work-item-task" aria-label={`${taskLabel}: ${task.title}`}>
    <Button className="sp-work-item-task-summary" variant="quiet" aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>
      <span aria-hidden="true">{expanded ? "▾" : "▸"}</span>
      <span><strong>{task.title}</strong><small>{task.application_name} · {state} · {task.assignee_name}</small></span>
    </Button>
    {newTask && !expanded && <Status>Ny informasjon i saka. Opne oppgåva og les gjennom før du sender ei avgjerd.</Status>}
    {expanded && <div className="sp-work-item-task-details">
    {task.status === "completed" && task.lifecycle?.can_start && <div className="sp-work-item-task-details">
      <Button variant="secondary" disabled={startingStatus || !!startReceipt} busy={startingStatus} onClick={() => void startStatus()}>
        {api.pendingStatusStart(task) ? "Prøv same start igjen" : "Endre status"}
      </Button>
      {startReceipt && <Status>Statusoppgåva er sett i kø i {startReceipt.channel_name} ({startReceipt.start_status}).</Status>}
    </div>}

      <p>{task.description}</p>
      <SupplementHistory supplements={task.supplements ?? []} />
      {newTask && <div className="sp-work-item-information"><Status>Ny informasjon eller endring i saka. Les gjennom før du sender ei avgjerd. Utkastet ditt er bevart.</Status>
        {newTask.description !== task.description && <p>{newTask.description}</p>}
        <SupplementHistory supplements={newTask.supplements ?? []} />
        <Button disabled={saving || !!decisionRetry || !!githubRetry || !!statusRetry} onClick={() => { acceptTask(newTask); setError(""); }}>Eg har lese den nye informasjonen</Button>
      </div>}
      {task.information_request && <div className="sp-work-item-information"><strong>Spørsmål frå behandlar</strong><p>{task.information_request}</p></div>}
      {task.information_response && <div className="sp-work-item-information"><strong>Svar frå innmeldar</strong><p>{task.information_response}</p></div>}
      {task.blocked && <Status tone="error">Den tildelte personen manglar rett eller kanaltilgang. Kretsansvarleg må rette oppsettet.</Status>}
      {task.category && task.decision_status && <p>Kategori: {task.category} · Prioritet: {task.priority} · Status: {task.decision_status}</p>}
      {task.lifecycle && <>
        <p>Saksstatus: <strong>{statusName(task.lifecycle.case_status)}</strong></p>
        {task.lifecycle.history.length > 0 && <div className="sp-work-item-information"><strong>Statushistorikk</strong>
          <ol>{task.lifecycle.history.map((entry, index) => <li key={`${entry.created_at}-${index}`}>
            {statusName(entry.from_status)} → {statusName(entry.to_status)} · {entry.actor_name}
            {entry.public_feedback && <p>Til innmeldar: {entry.public_feedback}</p>}
            {entry.internal_note && <p>Internt notat: {entry.internal_note}</p>}
          </li>)}</ol>
        </div>}
      </>}
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
          <Button type="submit" disabled={saving || !!newTask && !githubRetry || !!githubTargetChanged || !task.github_export.can_publish || !githubTarget?.repository || !githubTitle.trim() || !!githubRetry && !githubRetry.send} busy={saving}>{githubRetry?.send ? "Prøv same innsending igjen" : "Send til GitHub"}</Button>
          <Button type="button" variant="secondary" disabled={saving || !!newTask && !githubRetry || !!githubRetry && githubRetry.send} onClick={() => void exportGithub(false)}>{githubRetry && !githubRetry.send ? "Prøv same val igjen" : "Berre internt"}</Button>
        </form> : task.github_export.status === "ready" && <Status>{state}</Status>}
      </> : statusTask ? task.can_decide && task.lifecycle ? <form onSubmit={event => { event.preventDefault(); void submitStatus(false); }}>
        <label>Ny status<select required value={statusChoice} disabled={saving || !!statusRetry} onChange={event => { statusDraftTouched.current = true; setStatusChoice(event.currentTarget.value); }}>
          {task.lifecycle.allowed_statuses.map(value => <option key={value} value={value}>{statusName(value)}</option>)}
        </select></label>
        <label>Internt notat<textarea maxLength={2000} rows={3} value={internalNote} disabled={saving || !!statusRetry} onChange={event => { statusDraftTouched.current = true; setInternalNote(event.currentTarget.value); }} /></label>
        <label>Tilbakemelding til innmeldaren<textarea maxLength={2000} rows={3} value={publicFeedback} disabled={saving || !!statusRetry} onChange={event => { statusDraftTouched.current = true; setPublicFeedback(event.currentTarget.value); }} /></label>
        {statusRetry && <Status tone="error">Førre svar er uklart. Same avgjerd kan prøvast igjen med same innhald.</Status>}
        <Button type="submit" busy={saving} disabled={saving || !statusChoice || !!statusRetry && statusRetry.no_change}>{statusRetry && !statusRetry.no_change ? "Prøv same status igjen" : "Lagre status"}</Button>
        <Button type="button" variant="secondary" disabled={saving || !!statusRetry && !statusRetry.no_change} onClick={() => void submitStatus(true)}>{statusRetry?.no_change ? "Prøv same val igjen" : "Avslutt utan endring"}</Button>
      </form> : <Status>{state}</Status> : task.can_decide || decisionRetry ? <form onSubmit={event => { event.preventDefault(); void save(); }}>
        {decisionRetry && <Status>Førre svar er uklart. Prøv same avgjerd igjen før du endrar vala.</Status>}
        {information ? <label>Svar til behandlar<textarea required maxLength={8000} rows={3} value={note} disabled={saving || !!decisionRetry} onChange={event => setNote(event.currentTarget.value)} /></label> : <>
        <label>Kategori<select value={category} disabled={saving || !!decisionRetry} onChange={event => setCategory(event.currentTarget.value)}>
          <option value="bug">Feil</option><option value="change">Endringsønske</option><option value="question">Spørsmål/anna</option>
        </select></label>
        <label>Prioritet<select value={priority} disabled={saving || !!decisionRetry} onChange={event => setPriority(event.currentTarget.value)}>
          <option value="untriaged">Uavklart</option><option value="low">Låg</option><option value="normal">Normal</option>
          <option value="high">Høg</option><option value="critical">Kritisk</option>
        </select></label>
        <label>Avgjerd<select value={decision} disabled={saving || !!decisionRetry} onChange={event => setDecision(event.currentTarget.value)}>
          <option value="planned">Planlagt</option>
          {task.can_request_information && <option value="needs_information">Be om informasjon</option>}
          <option value="resolved">Løyst</option><option value="rejected">Avvist</option>
        </select></label>
        {decision === "needs_information" && <label>Spørsmål til innmeldar<textarea required maxLength={8000} rows={3} value={note} disabled={saving || !!decisionRetry} onChange={event => setNote(event.currentTarget.value)} /></label>}
        </>}
        <Button type="submit" disabled={saving || !!newTask && !decisionRetry || ((information || decision === "needs_information") && !note.trim())} busy={saving}>{information ? "Send svar" : decision === "needs_information" ? "Send spørsmål" : "Lagre avgjerd"}</Button>
      </form> : <Status>{task.status === "pending" && task.delivery_status === "ready" ? "Berre den tildelte personen kan sende inn denne oppgåva." : state}</Status>}
      {error && <Status tone="error">{error}</Status>}
    </div>}
  </section>;
}
