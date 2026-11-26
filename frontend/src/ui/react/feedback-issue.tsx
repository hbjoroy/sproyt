import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useEffect, useState } from "react";
import { activationLabels, feedbackStage, intakeStage, type Clarification, type FeedbackActivation, type FeedbackPanel, type FeedbackIntake } from "../../feedback-issue";
import { HttpError } from "../../api";
import type { WorkItemApi } from "../../work-items";

type Source = { api: WorkItemApi; channelId: string; messageId: string };

export function FeedbackIssueMenuAction({ api, channelId, messageId, onOpen }: Source & {
  onOpen: (panel: FeedbackPanel) => void;
}) {
  const [panel, setPanel] = useState<FeedbackPanel | null>(null);
  const [activation, setActivation] = useState<FeedbackActivation | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setFailed(false);
    setPanel(null);
    setActivation(null);
    void api.feedbackPanel(channelId, messageId, controller.signal)
      .then(async value => {
        if (controller.signal.aborted) return;
        if (value) return setPanel(value);
        const status = await api.feedbackActivation(channelId, messageId, false, controller.signal);
        if (!controller.signal.aborted) setActivation(status);
      })
      .catch(() => { if (!controller.signal.aborted) setFailed(true); });
    return () => controller.abort();
  }, [api, channelId, messageId, attempt]);
  if (failed) return <Button variant="quiet" onClick={() => setAttempt(attempt + 1)}>Prøv saksoppslaget igjen</Button>;
  if (panel) return <Button onClick={() => onOpen(panel)}>{panel.intake?.can_clarify ? "Avklar tilbakemelding"
    : panel.issue ? `GitHub-sak #${panel.issue.issue_number}` : "Tilbakemeldingsprosess"}</Button>;
  if (activation === "available") return <Button variant="quiet" disabled={submitting} onClick={async () => {
    setSubmitting(true);
    try { setActivation(await api.feedbackActivation(channelId, messageId, true)); }
    catch { setFailed(true); }
    finally { setSubmitting(false); }
  }}>{submitting ? "Aktiverer …" : activationLabels.available}</Button>;
  return activation && <span role="status">{activationLabels[activation]}</span>;
}

function ClarificationForm({ intake, api, channelId, messageId, onAccepted }: Source & {
  intake: FeedbackIntake; onAccepted: (intake: FeedbackIntake) => void;
}) {
  const [decision, setDecision] = useState<"" | Clarification["decision"]>("");
  const [number, setNumber] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState<Clarification | null>(null);
  const final = intake.stage === "final-clarify" || intake.dismiss_only === true;
  const choice = final ? "dismiss" : decision;
  const issueNumber = /^[1-9]\d*$/.test(number) ? Number(number) : 0;
  const valid = !!choice && !!note.trim() && (choice !== "existing" || (Number.isSafeInteger(issueNumber) && issueNumber > 0));
  async function submit() {
    if (!intake.clarification_task_id || !choice || !valid || busy) return;
    const command: Clarification = retry ?? { task_id: intake.clarification_task_id, decision: choice,
      issue_number: choice === "existing" ? issueNumber : null, note: note.trim() };
    setBusy(true); setError("");
    try { onAccepted(await api.clarifyFeedback(channelId, messageId, command)); }
    catch (cause) {
      const definitive = cause instanceof HttpError && [400, 403, 404, 409, 422].includes(cause.status);
      setRetry(definitive ? null : command);
      setError(definitive ? "Valet er ikkje tilgjengeleg. Oppdater saksstatus før du prøver igjen."
        : "Fekk ikkje stadfesta avgjerda. Du kan prøve same avklaring igjen.");
    } finally { setBusy(false); }
  }
  return <form className="sp-work-item-form" onSubmit={event => { event.preventDefault(); void submit(); }}>
    <fieldset disabled={busy || retry !== null}>
      <legend>Avklar tilbakemeldinga</legend>
      {final ? <p>Grunnlaget for publisering har endra seg. Du kan avslutte behandlinga her.</p>
        : <label>Avgjerd<select required value={decision} onChange={event => setDecision(event.currentTarget.value as typeof decision)}>
          <option value="">Vel utfall</option>
          <option value="create">Opprett ny GitHub-sak</option>
          <option value="existing">Knyt til eksisterande GitHub-sak</option>
          <option value="dismiss">Avslutt utan GitHub-sak</option>
        </select></label>}
      {choice === "existing" && <>
        {intake.candidates.length > 0 && <ul aria-label="Føreslåtte GitHub-saker">{intake.candidates.map(candidate =>
          <li key={candidate.number}><Button variant="quiet" onClick={() => setNumber(String(candidate.number))}>
            #{candidate.number} {candidate.title}</Button></li>)}</ul>}
        <label>Saksnummer i GitHub<input required type="text" inputMode="numeric" pattern="[1-9][0-9]*" value={number}
          onChange={event => setNumber(event.currentTarget.value)} /></label>
      </>}
      <label>Grunngjeving<textarea required rows={3} maxLength={2000} value={note}
        onChange={event => setNote(event.currentTarget.value)} /></label>
    </fieldset>
    <Button type="submit" busy={busy} disabled={busy || !valid}>{retry ? "Prøv same avklaring igjen" : "Registrer avklaring"}</Button>
    {error && <Status tone="error">{error}</Status>}
  </form>;
}

export function FeedbackIssueDialog({ api, channelId, messageId, initialPanel, onClose, onCompleted }: Source & {
  initialPanel: FeedbackPanel; onClose: () => void; onCompleted?: () => void;
}) {
  const [{ issue, intake }, setPanel] = useState(initialPanel);
  const [pending, setPending] = useState(() => api.pendingFeedbackReview(channelId, messageId));
  const [assessment, setAssessment] = useState(pending ?? "");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  async function refresh() {
    setBusy(true); setError("");
    try {
      const current = await api.feedbackPanel(channelId, messageId);
      if (current) { setPanel(current); setPending(api.pendingFeedbackReview(channelId, messageId)); }
      else onClose();
    } catch { setError("Kunne ikkje hente fersk saksstatus."); }
    finally { setBusy(false); }
  }
  async function submit() {
    if (busy || !issue?.can_review || !assessment.trim()) return;
    setBusy(true); setError("");
    try {
      const updated = await api.reviewFeedback(channelId, messageId, issue, assessment, intake?.review_task_id ?? undefined);
      setPanel({ issue: updated, intake: intake && { ...intake, can_review: false } });
      setAssessment(""); setPending(null); setNotice("Vurderinga er registrert."); onCompleted?.();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Kunne ikkje registrere vurderinga.");
      setPending(api.pendingFeedbackReview(channelId, messageId));
    } finally { setBusy(false); }
  }
  const stageLabel = intake ? intakeStage[intake.stage] : issue && feedbackStage[issue.stage];
  return <Dialog open title={issue ? `GitHub-sak #${issue.issue_number}` : "Tilbakemeldingsprosess"} closeLabel="Lukk saka" onClose={onClose}>
    <div className="sp-work-item-form">
      {intake && <div><h3>Tilbakemeldinga</h3><p>{intake.source_body}</p></div>}
      {issue && <a href={issue.issue_url} target="_blank" rel="noopener noreferrer">Opne sak #{issue.issue_number} i GitHub</a>}
      <dl>
        <dt>Prosessteg</dt><dd>{stageLabel}</dd>
        {issue?.github_state && <><dt>GitHub-status</dt><dd>{issue.github_state === "closed" ? "Lukka" : issue.github_proposed ? "Føreslått" : issue.github_status_name ?? "Open"}</dd></>}
      </dl>
      <Button variant="quiet" disabled={busy} onClick={() => void refresh()}>Oppdater saksstatus</Button>
      {issue?.published_title && <h3>{issue.published_title}</h3>}
      {issue?.published_body && <details><summary>{issue.publication_kind === "comment" ? "Publisert kommentar" : "Publisert sakstekst"}</summary>
        <div className="sp-work-item-information"><p>{issue.published_body}</p></div>
      </details>}
      {intake?.reason && <p>{intake.reason}</p>}
      {intake?.can_clarify && <ClarificationForm key={intake.clarification_task_id} intake={intake} api={api}
        channelId={channelId} messageId={messageId} onAccepted={result => {
          setPanel({ issue, intake: result }); setNotice("Avklaringa er registrert."); onCompleted?.();
        }} />}
      {issue?.can_review && issue.stage === "review" && (!intake || intake.can_review) && <form className="sp-work-item-form" onSubmit={event => { event.preventDefault(); void submit(); }}>
        <label>Produkteigarvurdering<textarea required rows={4} maxLength={2000} value={assessment} disabled={busy || pending !== null}
          onChange={event => setAssessment(event.currentTarget.value)} /></label>
        <Button type="submit" disabled={!assessment.trim() || busy} busy={busy}>{pending ? "Prøv same vurdering igjen" : "Utført"}</Button>
      </form>}
      {notice && <Status>{notice}</Status>}
      {error && <Status tone="error">{error}</Status>}
    </div>
  </Dialog>;
}
