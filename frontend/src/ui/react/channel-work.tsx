import { Button, Status } from "@sproyt/ui/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { HttpError } from "../../api";
import { assignedTaskKey, type AssignedProcessTask, type ChannelProcess, type ChannelWorkApi } from "../../channel-work";
import type { CircleProcess } from "../../circle-processes";
import type { ProcessPilotApi } from "../../process-pilot";
import type { WorkItemApi } from "../../work-items";
import { ProcessRow } from "./circle-processes";
import processCss from "./circle-processes.css";
import { ProcessTaskMessage } from "./process-pilot";
import { WorkItemTaskMessage } from "./work-item-task";
import css from "./channel-work.css";

type WorkScope = {
  api: ChannelWorkApi;
  owner: string;
  admitted: () => boolean;
};
type ChannelWorkProps = WorkScope & {
  channelId?: string;
  processPilot: ProcessPilotApi;
  workItems: WorkItemApi;
  onOpenChannel?: (channelId: string) => void;
  canOpenOrigin?: (channelId: string) => boolean;
};

type Page<T> = { items: readonly T[]; cursor: string | null };
type PageReader<T> = (after: string | null, signal: AbortSignal, live: () => boolean) => Promise<Page<T>>;

function lostAccess(cause: unknown): boolean {
  return cause instanceof HttpError && [401, 403, 404].includes(cause.status);
}

function usePagedList<T>(read: PageReader<T>, admitted: () => boolean, keyOf: (item: T) => string) {
  const latestAdmission = useRef(admitted);
  latestAdmission.current = admitted;
  const request = useRef<AbortController | null>(null);
  const [loaded, setLoaded] = useState<{ page: Page<T>; reader: PageReader<T> }>();
  const page = loaded?.reader === read ? loaded.page : { items: [], cursor: null };
  const [loading, setLoading] = useState(true);
  const [failure, setFailure] = useState<{ after: string | null; denied: boolean } | null>(null);

  const load = useCallback((after: string | null, preserveItems = false) => {
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    const live = () => !controller.signal.aborted && latestAdmission.current();
    setLoading(true);
    setFailure(null);
    void read(after, controller.signal, live).then(next => {
      if (!live()) return;
      setLoaded(current => ({ reader: read, page: {
        items: (after || preserveItems) && current?.reader === read ? [...new Map([...current.page.items, ...next.items].map(item => [keyOf(item), item])).values()] : next.items,
        cursor: preserveItems && current?.reader === read ? current.page.cursor ?? next.cursor : next.cursor,
      } }));
    }, cause => {
      if (!live()) return;
      if (lostAccess(cause)) setLoaded(undefined);
      setFailure({ after: lostAccess(cause) ? null : after, denied: lostAccess(cause) });
    }).finally(() => {
      if (live()) setLoading(false);
    });
  }, [read, keyOf]);

  useEffect(() => {
    load(null);
    return () => request.current?.abort();
  }, [load]);

  return { ...page, loading, failure, load };
}

function AssignedTaskRow({ item, personal, processPilot, workItems, onOpenChannel, onCompleted, retain }: {
  item: AssignedProcessTask; personal: boolean;
  processPilot: ProcessPilotApi; workItems: WorkItemApi;
  onOpenChannel?: (channelId: string) => void;
  onCompleted: (item: AssignedProcessTask, focus: HTMLElement | null) => void;
  retain: (item: AssignedProcessTask) => void;
}) {
  const [opened, setOpened] = useState(false);
  const row = useRef<HTMLLIElement>(null);
  const hadFocus = useRef(false);
  const origin = [item.circle_name, `# ${item.channel_name}`].filter(Boolean).join(" · ");
  const { id, message_id: messageId, title } = item.task;
  const complete = useCallback(() => {
    const element = row.current;
    const active = document.activeElement;
    const restoreFocus = element && (element.contains(active) || (active === document.body && hadFocus.current));
    const next = restoreFocus ? (element.nextElementSibling ?? element.previousElementSibling)?.querySelector("summary") : null;
    onCompleted(item, restoreFocus ? next ?? element : null);
  }, [onCompleted, item]);
  return <li ref={row} onFocusCapture={() => { hadFocus.current = true; }}
    onBlurCapture={event => { if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) hadFocus.current = false; }}><details className="sp-channel-work-task" onToggle={event => {
    if (event.currentTarget.open) setOpened(true);
  }}>
    <summary>
      <strong>{title}</strong>
      {personal && <small>{origin}</small>}
    </summary>
    {opened && <div className="sp-channel-work-task-body">
      {item.kind === "process_pilot"
        ? <ProcessTaskMessage api={processPilot} taskId={id} messageId={messageId} inline onCompleted={complete} onActionAttempt={() => retain(item)} />
        : <WorkItemTaskMessage api={workItems} taskId={id} messageId={messageId} inline onCompleted={complete} onActionAttempt={() => retain(item)} />}
      {personal && onOpenChannel && <Button variant="quiet" onClick={() => onOpenChannel(item.channel_id)}>
        Opne kanalen
      </Button>}
    </div>}
  </details></li>;
}

function AssignedTasks({ api, owner, channelId, admitted, processPilot, workItems, onOpenChannel, canOpenOrigin }: ChannelWorkProps) {
  const read = useCallback<PageReader<AssignedProcessTask>>(async (after, signal, live) => {
    const page = await api.tasks(owner, channelId, after, signal, live);
    return { items: page.tasks, cursor: page.next_cursor };
  }, [api, owner, channelId]);
  const { items, cursor, loading, failure, load } = usePagedList(read, admitted, assignedTaskKey);
  const heading = useRef<HTMLHeadingElement>(null);
  const confirmed = useRef(new Set<string>());
  const [retained, setRetained] = useState<ReadonlyMap<string, AssignedProcessTask>>(new Map());
  const [completedTitle, setCompletedTitle] = useState("");
  const retain = useCallback((item: AssignedProcessTask) => {
    setRetained(previous => new Map(previous).set(assignedTaskKey(item), item));
  }, []);
  const complete = useCallback((item: AssignedProcessTask, focus: HTMLElement | null) => {
    const key = assignedTaskKey(item);
    if (confirmed.current.has(key)) return;
    confirmed.current.add(key);
    setRetained(previous => { const next = new Map(previous); next.delete(key); return next; });
    setCompletedTitle(item.task.title);
    load(null, true);
    if (focus) requestAnimationFrame(() => {
      if (document.activeElement === document.body) (focus.isConnected ? focus : heading.current)?.focus();
    });
  }, [load]);
  useEffect(() => { if (failure?.denied) setRetained(new Map()); }, [failure]);
  const visible = failure?.denied ? [] : [...new Map([...items, ...retained.values()].map(item => [assignedTaskKey(item), item])).values()]
    .filter(item => !confirmed.current.has(assignedTaskKey(item)) && (canOpenOrigin?.(item.channel_id) ?? true));
  return <section aria-label="Mine oppgåver">
    <div className="sp-channel-work-heading">
      <h2 ref={heading} tabIndex={-1}>Mine oppgåver</h2>
      <Button variant="quiet" disabled={loading} onClick={() => load(null)}>Oppdater oppgåver</Button>
    </div>
    <div className="sp-channel-work-announcement" role="status">{completedTitle && `${completedTitle} er fullført.`}</div>
    {loading && <Status>Hentar oppgåver …</Status>}
    {failure && <Status tone="error">Kunne ikkje hente oppgåvene. <Button variant="quiet" onClick={() => load(failure.after)}>Prøv igjen</Button></Status>}
    {!loading && !failure && !visible.length && !cursor && <p>{channelId ? "Du har ingen opne oppgåver i denne kanalen." : "Du har ingen opne oppgåver."}</p>}
    {visible.length > 0 && <ul className="sp-channel-work-tasks">{visible.map(item => <AssignedTaskRow key={JSON.stringify([assignedTaskKey(item), item.task.message_id])}
      item={item} personal={!channelId} processPilot={processPilot} workItems={workItems} onOpenChannel={onOpenChannel} onCompleted={complete} retain={retain} />)}</ul>}
    {cursor && !failure && <Button disabled={loading} onClick={() => load(cursor)}>Vis fleire oppgåver</Button>}
  </section>;
}

function Definition({ process }: { process: ChannelProcess }) {
  const steps = process.definition_steps;
  const labels = new Map(steps?.map(step => [step.id, step.name]));
  const active = new Set(process.active_node_ids ?? []);
  const completed = new Set([...process.steps, ...process.history].filter(step => step.status === "completed").map(step => step.node_id));
  const activeNames = [...active].map(id => labels.get(id)).filter((name): name is string => !!name);
  return <div className="sp-channel-work-definition">
    <h4>Prosessdefinisjon</h4>
    <p><strong>{process.definition_name}</strong>{process.definition_version && <> · versjon {process.definition_version}</>}</p>
    {activeNames.length > 0 && <p>Aktive steg: {activeNames.join(", ")}</p>}
    {!steps && <p>Stega i den festa prosessversjonen er utilgjengelege.</p>}
    {steps && <div><p>Moglege steg og overgangar i denne versjonen.</p>
      <ul>{steps.map(step => <li key={step.id}>
        <strong>{step.name}</strong>{active.has(step.id) ? <span className="sp-channel-work-active">Aktiv</span>
          : completed.has(step.id) && <span className="sp-channel-work-active">Fullført</span>}
        {step.next.length > 0 && <small>Kan gå vidare til {step.next.map(id => labels.get(id) ?? id).join(", ")}</small>}
      </li>)}</ul>
    </div>}
  </div>;
}

function ChannelProcessRow({ api, owner, admitted, channelId, process, canOpenOrigin }: WorkScope & {
  channelId: string; process: CircleProcess; canOpenOrigin?: ChannelWorkProps["canOpenOrigin"];
}) {
  const [open, setOpen] = useState(false);
  const [detail, setDetail] = useState<ChannelProcess>();
  const [failed, setFailed] = useState(false);
  const [revision, setRevision] = useState(0);
  const latestAdmission = useRef(admitted);
  latestAdmission.current = admitted;

  useEffect(() => {
    if (!open) return;
    const controller = new AbortController();
    const live = () => !controller.signal.aborted && latestAdmission.current();
    setDetail(undefined);
    setFailed(false);
    void api.process(owner, channelId, process.id, controller.signal, live).then(result => {
      if (live()) setDetail(result);
    }, () => { if (live()) setFailed(true); });
    return () => controller.abort();
  }, [api, owner, channelId, process, open, revision]);

  const definition = !open ? null : detail ? <Definition process={detail} />
    : failed ? <Status tone="error">Kunne ikkje hente prosessdetaljane. <Button variant="quiet" onClick={() => setRevision(value => value + 1)}>Prøv igjen</Button></Status>
    : <Status>Hentar prosessdetaljane …</Status>;
  return <ProcessRow process={detail ?? process} definition={definition} onOpenChange={setOpen}
    canReadAssignee={id => id === channelId || Boolean(canOpenOrigin?.(id))} />;
}

const processKey = (process: CircleProcess) => process.id;

function ChannelProcesses({ api, owner, channelId, admitted, all, setAll, canOpenOrigin }: WorkScope & {
  channelId: string; all: boolean; setAll(value: boolean): void; canOpenOrigin?: ChannelWorkProps["canOpenOrigin"];
}) {
  const read = useCallback<PageReader<CircleProcess>>(async (after, signal, live) => {
    const page = await api.processes(owner, channelId, all ? "all" : "open", after, signal, live);
    return { items: page.processes, cursor: page.next_cursor };
  }, [api, owner, channelId, all]);
  const { items, cursor, loading, failure, load } = usePagedList(read, admitted, processKey);
  return <section aria-label="Prosessar i kanalen">
    <div className="sp-channel-work-heading">
      <h2>Prosessar</h2>
      <div className="sp-channel-work-filters">
        <label><input type="checkbox" checked={all} onChange={event => setAll(event.currentTarget.checked)} /> Vis òg avslutta prosessar</label>
        <Button variant="quiet" disabled={loading} onClick={() => load(null)}>Oppdater prosessar</Button>
      </div>
    </div>
    {loading && <Status>Hentar prosessar …</Status>}
    {failure && <Status tone="error">Kunne ikkje hente prosessane. <Button variant="quiet" onClick={() => load(failure.after)}>Prøv igjen</Button></Status>}
    {!loading && !failure && !items.length && !cursor && <p>{all ? "Ingen prosessar er registrerte i kanalen." : "Ingen opne prosessar i kanalen."}</p>}
    {items.map(process => <ChannelProcessRow key={process.id} api={api} owner={owner} channelId={channelId} admitted={admitted}
      process={process} canOpenOrigin={canOpenOrigin} />)}
    {cursor && !failure && <Button disabled={loading} onClick={() => load(cursor)}>Vis fleire prosessar</Button>}
  </section>;
}

function ChannelWorkPane(props: ChannelWorkProps) {
  const { channelId } = props;
  const [all, setAll] = useState(false);
  return <div className="sp-channel-work">
    <style>{css}</style><style>{processCss}</style>
    <AssignedTasks {...props} />
    {channelId && <ChannelProcesses api={props.api} owner={props.owner} channelId={channelId}
      admitted={props.admitted} all={all} setAll={setAll} canOpenOrigin={props.canOpenOrigin} />}
  </div>;
}

export function ChannelWork(props: ChannelWorkProps) {
  if (!props.admitted()) return null;
  return <ChannelWorkPane key={JSON.stringify([props.owner, props.channelId])} {...props} />;
}
