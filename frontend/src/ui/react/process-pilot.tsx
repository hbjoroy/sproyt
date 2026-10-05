import { Button, Status } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { PilotConfiguration, PilotTask, ProcessPilotApi } from "../../process-pilot";

const errorMessage = (error: unknown) => error instanceof Error ? error.message : "Kunne ikkje kontakte prosessen. Prøv igjen.";

export function ProcessPilotChannelAction({ api, channelId }: { api: ProcessPilotApi; channelId: string }) {
  const [configuration, setConfiguration] = useState<PilotConfiguration>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [started, setStarted] = useState(false);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    void api.configuration(channelId, controller.signal).then(setConfiguration).catch(error => {
      if (!controller.signal.aborted) setError(errorMessage(error));
    });
    return () => controller.abort();
  }, [api, channelId, reload]);
  const run = async (configure: boolean) => {
    if (busy) return;
    setBusy(true); setError("");
    try {
      if (configure) { await api.configure(channelId); setConfiguration(await api.configuration(channelId)); }
      else { await api.start(channelId); setStarted(true); }
    } catch (error) { setError(errorMessage(error)); }
    finally { setBusy(false); }
  };
  return <>
    {configuration?.can_configure && !configuration.configured && <div>
      <p>{configuration.runtime_model === "v2" ? "Prosesspiloten gir deg to parallelle vurderingar og deretter ei siste stadfesting i denne kanalen." : "Prosesspiloten gir deg to oppgåver etter kvarandre i denne kanalen."}</p>
      <Button busy={busy} onClick={() => void run(true)}>Aktiver prosesspilot for meg</Button>
    </div>}
    {configuration?.can_start && <div>
      <p>{configuration.runtime_model === "v2" ? "To parallelle vurderingar, deretter ei siste stadfesting" : "To oppgåver etter kvarandre"}{configuration.assignee_name ? ` for ${configuration.assignee_name}` : ""}.</p>
      <Button busy={busy} onClick={() => void run(false)}>{started ? "Start ein ny prosessgjennomgang" : configuration.runtime_model === "v2" ? "Start parallell prosessgjennomgang" : "Start testprosess"}</Button>
      {started && <Status>Prosessen er starta. Oppgåva kjem som ei melding i kanalen.</Status>}
    </div>}
    {error && <Status tone="error">{error} <Button disabled={busy} onClick={() => { setError(""); setReload(value => value + 1); }}>Hent status på nytt</Button></Status>}
  </>;
}

export function ProcessTaskDetails({ task, busy, onComplete }: { task: PilotTask; busy: boolean; onComplete(): void }) {
  const terminal = task.status !== "pending" || ["completed", "cancelled", "failed"].includes(task.process_status);
  return <div className="sp-process-task-details">
    <p>{task.assignee_name ? `Tildelt ${task.assignee_name}` : task.can_complete ? "Tildelt deg" : "Tildelt ein annan deltakar"}</p>
    {task.status === "completed" ? <p>Oppgåva er fullført.</p>
      : task.process_status === "failed" ? <Status tone="error">Prosessen feila. Oppgåva kan ikkje fullførast.</Status>
      : task.status === "cancelled" ? <Status tone="error">Oppgåva er avbroten.</Status>
      : task.process_status === "cancelled" ? <Status tone="error">Prosessen vart avbroten før oppgåva kunne fullførast.</Status>

      : task.process_status === "completed" ? <p>Prosessen er fullført.</p>
      : !terminal && task.can_complete ? <Button busy={busy} disabled={task.delivery_status === "pending"} onClick={onComplete}>
        {task.delivery_status === "pending" ? "Ventar på stadfesting" : "Fullfør oppgåva"}</Button>
        : <p>Berre personen som har fått oppgåva, kan fullføre henne.</p>}
    {task.delivery_status === "pending" && <Status>Fullføringa er send. Ventar på stadfesting frå prosessen.</Status>}
    {task.delivery_status === "failed" && <Status tone="error">Prosessen kunne ikkje levere neste steg. Prøv å hente status igjen.</Status>}
  </div>;
}

export function ProcessTaskMessage({ api, taskId, messageId }: { api: ProcessPilotApi; taskId: string; messageId: string }) {
  const root = useRef<HTMLDetailsElement>(null);
  const [task, setTask] = useState<PilotTask>();
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [completionAttempted, setCompletionAttempted] = useState(false);
  const [reload, setReload] = useState(0);
  const mounted = useRef(true);
  const mutation = useRef(false);
  const revision = useRef(0);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => {
    let disposed = false;
    let visible = typeof IntersectionObserver === "undefined";
    let timer: ReturnType<typeof setTimeout> | undefined;
    let inFlight: AbortController | undefined;
    const refresh = async () => {
      if (disposed || !visible || document.hidden || inFlight || mutation.current) return;
      const controller = new AbortController();
      const fetchedRevision = revision.current;
      inFlight = controller;
      try {
        const next = await api.task(taskId, messageId, controller.signal);
        if (!disposed && !mutation.current && fetchedRevision === revision.current) setTask(next);
      } catch (error) { if (!disposed && !controller.signal.aborted) setError(errorMessage(error)); }
      finally {
        inFlight = undefined;
        if (!disposed) timer = setTimeout(() => void refresh(), 5_000);
      }
    };
    const restart = () => { clearTimeout(timer); void refresh(); };
    const observer = typeof IntersectionObserver === "undefined" ? undefined : new IntersectionObserver(entries => {
      visible = entries.some(entry => entry.isIntersecting);
      if (visible) restart(); else { clearTimeout(timer); inFlight?.abort(); }
    });
    if (root.current) observer?.observe(root.current);
    document.addEventListener("visibilitychange", restart);
    void refresh();
    return () => { disposed = true; clearTimeout(timer); inFlight?.abort(); observer?.disconnect(); document.removeEventListener("visibilitychange", restart); };
  }, [api, taskId, messageId, reload]);
  const complete = async () => {
    if (mutation.current || !task?.can_complete || task.status !== "pending" || task.delivery_status === "pending"
      || ["completed", "cancelled", "failed"].includes(task.process_status)) return;
    mutation.current = true; revision.current++; setBusy(true); setCompletionAttempted(true); setError("");
    try {
      const updated = await api.complete(taskId, messageId);
      if (mounted.current) setTask(updated);
    } catch (error) { if (mounted.current) setError(errorMessage(error)); }
    finally {
      mutation.current = false;
      if (mounted.current) { setBusy(false); setReload(value => value + 1); }
    }
  };
  const statusLabel = task?.status === "completed" || task?.process_status === "completed" ? "Fullført"
    : task?.process_status === "failed" ? "Feila"
      : task?.status === "cancelled" || task?.process_status === "cancelled" ? "Avbroten" : task ? "Ventar" : error ? "Kunne ikkje hente status" : "Hentar status …";
  const reloadStatus = task?.status !== "pending" ? task?.status
    : ["completed", "cancelled", "failed"].includes(task.process_status) ? task.process_status : "pending";
  return <details className="sp-process-task" ref={root} data-task-status={reloadStatus}
    data-app-update-recoverable={completionAttempted && !busy && api.canReload()}>
    <summary>{task?.title ?? "Prosessoppgåve"}<span className="sp-process-task-status">{statusLabel}</span></summary>
    {task && <ProcessTaskDetails task={task} busy={busy} onComplete={() => void complete()} />}
    {error && <Status tone="error">{error}</Status>}
    <Button variant="quiet" disabled={busy} onClick={() => { setError(""); setReload(value => value + 1); }}>Hent status på nytt</Button>
  </details>;
}
