import { Button, Status } from "@sproyt/ui/react";
import { useEffect, useState } from "react";
import type { PublicWorkItemStatus, WorkItemApi } from "../../work-items";

const statusName = (value: string): string => ({ planned: "Planlagt", in_development: "Under utvikling",
  resolved: "Løyst", rejected: "Avvist" })[value as "planned" | "in_development" | "resolved" | "rejected"] ?? value;

export function WorkItemStatusMessage({ api, itemId, messageId }: {
  readonly api: WorkItemApi; readonly itemId: string; readonly messageId: string;
}) {
  const [status, setStatus] = useState<PublicWorkItemStatus>();
  const [error, setError] = useState("");
  const [expanded, setExpanded] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    const refresh = () => void api.publicStatus(itemId, messageId, controller.signal)
      .then(value => { if (!controller.signal.aborted) { setStatus(value); setError(""); } })
      .catch(cause => { if (!controller.signal.aborted) setError(cause instanceof Error ? cause.message : "Kunne ikkje lese statusoppdateringa."); });
    refresh();
    const timer = window.setInterval(refresh, 5000);
    return () => { controller.abort(); window.clearInterval(timer); };
  }, [api, itemId, messageId]);
  if (!status) return <div className="sp-work-item-task"><Status tone={error ? "error" : undefined}>{error || "Hentar statusoppdateringa …"}</Status></div>;
  if (!status.visible) return <div className="sp-work-item-task"><span>Statusoppdatering til innmeldaren</span></div>;
  return <section className="sp-work-item-task" aria-label={`Statusoppdatering: ${status.title}`}>
    <Button className="sp-work-item-task-summary" variant="quiet" aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>
      <span aria-hidden="true">{expanded ? "▾" : "▸"}</span>
      <span><strong>{status.title}</strong><small>{status.application_name} · {statusName(status.status)}</small></span>
    </Button>
    {expanded && <div className="sp-work-item-task-details">
      <p>Gjeldande status: <strong>{statusName(status.status)}</strong></p>
      {status.public_feedback && <p>Tilbakemelding frå behandlar: {status.public_feedback}</p>}
      {status.history.length > 0 && <div className="sp-work-item-information"><strong>Statushistorikk</strong>
        <ol>{status.history.map((entry, index) => <li key={`${entry.created_at}-${index}`}>
          {statusName(entry.from_status)} → {statusName(entry.to_status)}
          {entry.public_feedback && <p>{entry.public_feedback}</p>}
        </li>)}</ol>
      </div>}
      {error && <Status tone="error">{error}</Status>}
    </div>}
  </section>;
}
