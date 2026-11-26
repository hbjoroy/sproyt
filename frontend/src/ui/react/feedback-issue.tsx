import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useEffect, useState } from "react";
import { activationLabels, feedbackStage, type FeedbackActivation, type FeedbackIssue } from "../../feedback-issue";
import type { WorkItemApi } from "../../work-items";

type Source = { api: WorkItemApi; channelId: string; messageId: string };

export function FeedbackIssueMenuAction({ api, channelId, messageId, onOpen }: Source & {
  onOpen: (issue: FeedbackIssue) => void;
}) {
  const [issue, setIssue] = useState<FeedbackIssue | null>(null);
  const [activation, setActivation] = useState<FeedbackActivation | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setFailed(false);
    setIssue(null);
    setActivation(null);
    void api.feedbackIssue(channelId, messageId, controller.signal)
      .then(async value => {
        if (controller.signal.aborted) return;
        if (value) return setIssue(value);
        const status = await api.feedbackActivation(channelId, messageId, false, controller.signal);
        if (!controller.signal.aborted) setActivation(status);
      })
      .catch(() => { if (!controller.signal.aborted) setFailed(true); });
    return () => controller.abort();
  }, [api, channelId, messageId, attempt]);
  if (failed) return <Button variant="quiet" onClick={() => setAttempt(attempt + 1)}>Prøv saksoppslaget igjen</Button>;
  if (issue) return <Button onClick={() => onOpen(issue)}>GitHub-sak #{issue.issue_number}</Button>;
  if (activation === "available") return <Button variant="quiet" disabled={submitting} onClick={async () => {
    setSubmitting(true);
    try { setActivation(await api.feedbackActivation(channelId, messageId, true)); }
    catch { setFailed(true); }
    finally { setSubmitting(false); }
  }}>{submitting ? "Aktiverer …" : activationLabels.available}</Button>;
  return activation && <span role="status">{activationLabels[activation]}</span>;
}

export function FeedbackIssueDialog({ api, channelId, messageId, initialIssue, onClose }: Source & {
  initialIssue: FeedbackIssue; onClose: () => void;
}) {
  const [issue, setIssue] = useState(initialIssue);
  const [pending, setPending] = useState(() => api.pendingFeedbackReview(channelId, messageId));
  const [assessment, setAssessment] = useState(pending ?? "");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function refresh() {
    setBusy(true); setError("");
    try {
      const current = await api.feedbackIssue(channelId, messageId);
      if (current) { setIssue(current); setPending(api.pendingFeedbackReview(channelId, messageId)); }
      else onClose();
    } catch { setError("Kunne ikkje hente fersk saksstatus."); }
    finally { setBusy(false); }
  }
  async function submit() {
    if (busy || !issue.can_review || !assessment.trim()) return;
    setBusy(true); setError("");
    try {
      setIssue(await api.reviewFeedback(channelId, messageId, issue, assessment));
      setAssessment(""); setPending(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Kunne ikkje registrere vurderinga.");
      setPending(api.pendingFeedbackReview(channelId, messageId));
    } finally { setBusy(false); }
  }
  return <Dialog open title={`GitHub-sak #${issue.issue_number}`} closeLabel="Lukk saka" onClose={onClose}>
    <div className="sp-work-item-form">
      <a href={issue.issue_url} target="_blank" rel="noopener noreferrer">Opne sak #{issue.issue_number} i GitHub</a>
      <dl>
        <dt>Prosessteg</dt><dd>{feedbackStage[issue.stage]}</dd>
        {issue.github_state && <><dt>GitHub-status</dt><dd>{issue.github_state === "closed" ? "Lukka" : issue.github_proposed ? "Føreslått" : issue.github_status_name ?? "Open"}</dd></>}
      </dl>
      <Button variant="quiet" disabled={busy} onClick={() => void refresh()}>Oppdater saksstatus</Button>
      {issue.published_title && <h3>{issue.published_title}</h3>}
      {issue.published_body && <details><summary>{issue.publication_kind === "comment" ? "Publisert kommentar" : "Publisert sakstekst"}</summary>
        <div className="sp-work-item-information"><p>{issue.published_body}</p></div>
      </details>}
      {issue.can_review && issue.stage === "review" && <form className="sp-work-item-form" onSubmit={event => { event.preventDefault(); void submit(); }}>
        <label>Produkteigarvurdering<textarea required rows={4} maxLength={2000} value={assessment} disabled={busy || pending !== null}
          onChange={event => setAssessment(event.currentTarget.value)} /></label>
        <Button type="submit" disabled={!assessment.trim() || busy} busy={busy}>{pending ? "Prøv same vurdering igjen" : "Utført"}</Button>
      </form>}
      {error && <Status tone="error">{error}</Status>}
    </div>
  </Dialog>;
}
