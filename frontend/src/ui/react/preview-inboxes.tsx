import { Button, Status, TextField } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import { DestinationPage } from "./workspace-page";
import { inboxCounts, approximateCount } from "./inbox-counts";
import inboxCss from "../preview-inboxes.css";
import type { Channel, Mention, UserTask } from "../../types";

export type PreviewInboxKind = "unread" | "mentions" | "tasks";

export interface PreviewInboxState {
  readonly channels: readonly Channel[];
  readonly mentions: readonly Mention[];
  readonly tasks: readonly UserTask[];
  readonly participantId: string | null;
  readonly circleNames: Readonly<Record<string, string>>;
  readonly directoryReady?: boolean;
  readonly directoryError?: string;
  readonly mentionsReady?: boolean;
  readonly mentionsError?: string;
  readonly tasksReady?: boolean;
  readonly tasksError?: string;
}

export interface PreviewInboxHost {
  load(kind: "mentions" | "tasks"): Promise<void>;
  retryDirectory?(): void;
  openChannel(channelId: string): void;
  openMentionSource(mention: Mention): void;
  openTaskSource(task: UserTask): void;
  markMentionRead(messageId: string): Promise<void>;
  createTask(input: Readonly<{ sourceMessageId: string; title: string; processLinkId: string | null }>): Promise<void>;
  setTaskDone(taskId: string, done: boolean): Promise<void>;
}

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

function MentionRow({ mention, host, onTaskCreated, onOpenSource, context }: {
  readonly mention: Mention;
  readonly host: PreviewInboxHost;
  readonly onTaskCreated: () => void;
  readonly onOpenSource?: () => void;
  readonly context: string;
}) {
  const [editingTask, setEditingTask] = useState(false);
  const [title, setTitle] = useState(() => mention.message.body.replace(/@\S+/gu, "").trim());
  const [processLinkId, setProcessLinkId] = useState("");
  const [busy, setBusy] = useState<"read" | "task">();
  const [error, setError] = useState("");
  const run = async (kind: "read" | "task", action: () => Promise<void>) => {
    if (busy) return;
    setBusy(kind); setError("");
    try { await action(); }
    catch (error) { setError(errorText(error)); }
    finally { setBusy(undefined); }
  };
  return <article className="sp-inbox-entry">
    <p className="sp-inbox-entry-meta">{context} · {mention.message.sender_display_name} · {new Date(mention.message.sent_at).toLocaleString("nn-NO", { dateStyle: "short", timeStyle: "short" })} · {mention.read ? "Lesen" : "Ulest"}</p>
    <p className="sp-inbox-excerpt">{mention.message.body}</p>
    <div className="sp-inbox-entry-actions">
      <Button variant="quiet" onClick={onOpenSource ?? (() => host.openMentionSource(mention))}>Opne kjelda</Button>
      {!mention.read && <Button variant="quiet" busy={busy === "read"}
        onClick={() => void run("read", () => host.markMentionRead(mention.message.id))}>Marker lesen</Button>}
      <Button variant="quiet" aria-expanded={editingTask} onClick={() => setEditingTask(value => !value)}>Lag oppgåve</Button>
    </div>
    {editingTask && <form aria-label={`Ny oppgåve frå omtale i ${mention.channel_name}`} onSubmit={event => {
      event.preventDefault();
      const trimmed = title.trim();
      if (!trimmed) return;
      void run("task", async () => {
        await host.createTask({ sourceMessageId: mention.message.id, title: trimmed, processLinkId: processLinkId.trim() || null });
        setEditingTask(false); onTaskCreated();
      });
    }} style={{ display: "grid", gap: 8 }}>
      <TextField label="Oppgåvetittel" value={title} maxLength={240} required autoFocus disabled={Boolean(busy)}
        onChange={event => setTitle(event.target.value)} />
      <TextField label="Heart-prosess-ID (valfritt)" value={processLinkId} disabled={Boolean(busy)}
        onChange={event => setProcessLinkId(event.target.value)} />
      <div><Button type="submit" busy={busy === "task"} disabled={!title.trim()}>Lagre oppgåve</Button>
        <Button disabled={Boolean(busy)} onClick={() => setEditingTask(false)}>Avbryt</Button></div>
    </form>}
    {error && <Status tone="error">{error}</Status>}
  </article>;
}

function TaskRow({ task, host, onOpenSource, context }: { readonly task: UserTask; readonly host: PreviewInboxHost; readonly onOpenSource: () => void; readonly context: string }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const done = task.status === "done";
  return <article className="sp-inbox-entry" data-done={done || undefined}>
    <h3>{task.title}</h3>
    <p className="sp-inbox-entry-meta">{context}{task.process_link_id ? ` · Heart ${task.process_link_id}` : ""}</p>
    <div className="sp-inbox-entry-actions"><Button variant="quiet" busy={busy} onClick={() => {
      if (busy) return;
      setBusy(true); setError("");
      void host.setTaskDone(task.id, !done).catch(error => setError(errorText(error))).finally(() => setBusy(false));
    }}>{done ? "Opne igjen" : "Ferdig"}</Button>
    <Button variant="quiet" onClick={onOpenSource}>Opne kjelda</Button></div>
    {error && <Status tone="error">{error}</Status>}
  </article>;
}

/** React-owned personal inbox. It leaves the selected conversation mounted,
 * so channel and thread drafts, reading position and composer focus survive. */
export function PreviewInboxes({ state, host, activeKind, onNavigate, onBack }: {
  readonly state: PreviewInboxState;
  readonly host: PreviewInboxHost;
  readonly activeKind?: PreviewInboxKind | null;
  readonly onNavigate?: (kind: PreviewInboxKind) => void;
  readonly onBack?: () => void;
}) {
  const [localOpen, setLocalOpen] = useState(false);
  const [localKind, setLocalKind] = useState<PreviewInboxKind>("unread");
  const controlled = activeKind !== undefined;
  const open = controlled ? activeKind !== null : localOpen;
  const kind = activeKind ?? localKind;
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const trigger = useRef<HTMLButtonElement>(null);
  const latestHost = useRef(host); latestHost.current = host;
  const scope = `${state.participantId}:${open ? kind : "closed"}`;
  const currentScope = useRef({ key: scope, generation: 0 });
  if (currentScope.current.key !== scope) currentScope.current = { key: scope, generation: currentScope.current.generation + 1 };
  const generation = currentScope.current.generation;
  const focusKey = JSON.stringify([state.participantId, state.channels.map(channel =>
    [channel.id, channel.role, channel.kind, channel.circle_id]).sort((a, b) => String(a[0]).localeCompare(String(b[0])))]);
  const focusScope = useRef({ key: focusKey, epoch: 0 });
  if (focusScope.current.key !== focusKey) focusScope.current = { key: focusKey, epoch: focusScope.current.epoch + 1 };
  const focusEpoch = focusScope.current.epoch;
  const activity = useRef(0);
  const suppressSourceReturn = useRef(false);
  const focusTicket = useRef<{ owner: string | null; generation: number; epoch: number; activity: number } | null>(null);
  const previousOpen = useRef({ owner: state.participantId, open, generation, epoch: focusEpoch });
  useEffect(() => {
    const interacted = () => { activity.current++; };
    document.addEventListener("pointerdown", interacted, true);
    document.addEventListener("keydown", interacted, true);
    return () => {
      document.removeEventListener("pointerdown", interacted, true);
      document.removeEventListener("keydown", interacted, true);
    };
  }, []);
  useEffect(() => {
    const previous = previousOpen.current;
    previousOpen.current = { owner: state.participantId, open, generation, epoch: focusEpoch };
    if (open || !previous.open || previous.owner !== state.participantId || previous.epoch !== focusEpoch) {
      focusTicket.current = null;
      suppressSourceReturn.current = false;
      return;
    }
    // Browser Back also closes the controlled page. Source navigation owns its
    // destination focus and deliberately opts out of this return path.
    const ticket = focusTicket.current ?? { owner: previous.owner, generation: previous.generation, epoch: previous.epoch, activity: activity.current };
    focusTicket.current = null;
    if (suppressSourceReturn.current) { suppressSourceReturn.current = false; return; }
    if (ticket.owner !== state.participantId || ticket.generation !== previous.generation || ticket.epoch !== focusEpoch) return;
    const frame = requestAnimationFrame(() => {
      const button = trigger.current;
      if (currentScope.current.generation !== generation || focusScope.current.epoch !== ticket.epoch
        || activity.current !== ticket.activity || !button?.isConnected || !button.getClientRects().length
        || document.querySelector("[data-workspace-page], dialog[open]")) return;
      const active = document.activeElement;
      // The header's gear is only its generic navigation fallback. A newly
      // focused composer or another visible control keeps the user's focus.
      if (active !== document.body && active instanceof HTMLElement && active.getClientRects().length
        && !active.matches(".sp-header-toggle")) return;
      button.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [open, state.participantId, generation, focusEpoch]);
  useEffect(() => {
    setError("");
    if (!open || kind === "unread") { setLoading(false); return; }
    let live = true;
    setLoading(true);
    void latestHost.current.load(kind).catch(error => {
      if (live) setError(errorText(error));
    }).finally(() => { if (live) setLoading(false); });
    return () => { live = false; };
  }, [open, kind, attempt, state.participantId]);
  const unread = state.channels
    .map(channel => ({ channel, count: Math.max(0, channel.latest_sequence - channel.last_read_sequence) }))
    .filter(item => item.count > 0)
    .sort((left, right) => right.count - left.count);
  const unreadTotal = unread.reduce((total, item) => total + item.count, 0);
  const { mentionCount, taskCount } = inboxCounts(state.mentions, state.tasks);
  const mentionsReady = state.mentionsReady !== false && !state.mentionsError;
  const tasksReady = state.tasksReady !== false && !state.tasksError;
  const ready = kind === "unread" ? state.directoryReady !== false : kind === "mentions" ? mentionsReady : tasksReady;
  const stateError = kind === "unread" ? state.directoryError : kind === "mentions" ? state.mentionsError : state.tasksError;
  const previousReadiness = useRef({ owner: state.participantId, mentions: state.mentionsReady, tasks: state.tasksReady });
  useEffect(() => {
    const previous = previousReadiness.current;
    if (previous.owner === state.participantId && ((kind === "mentions" && previous.mentions === false && state.mentionsReady === true && !state.mentionsError)
      || (kind === "tasks" && previous.tasks === false && state.tasksReady === true && !state.tasksError))) setError("");
    previousReadiness.current = { owner: state.participantId, mentions: state.mentionsReady, tasks: state.tasksReady };
  }, [kind, state.participantId, state.mentionsReady, state.mentionsError, state.tasksReady, state.tasksError]);
  const visibleError = error || stateError;
  const waiting = !visibleError && (loading || !ready);
  const contextFor = (channelId: string, name: string) => {
    const channel = state.channels.find(item => item.id === channelId);
    const scope = channel?.circle_id ? state.circleNames[channel.circle_id] || "Vennekrets" : channel?.is_direct ? "Direktemelding" : "Felles";
    return `${scope} / ${name}`;
  };
  const select = (next: PreviewInboxKind) => {
    setLocalKind(next);
    if (!controlled) setLocalOpen(true);
    onNavigate?.(next);
  };
  const close = () => {
    if (currentScope.current.generation !== generation || focusScope.current.epoch !== focusEpoch) return;
    focusTicket.current = { owner: state.participantId, generation, epoch: focusEpoch, activity: activity.current };
    if (controlled) onBack?.(); else setLocalOpen(false);
  };
  // Source commands own their next route. Closing the old controlled route
  // afterwards would traverse history and undo the source navigation.
  const openSource = (action: () => void) => {
    focusTicket.current = null; suppressSourceReturn.current = true;
    action();
    if (!controlled) setLocalOpen(false);
  };
  return <>
    <style>{inboxCss}</style>
    <Button ref={trigger} variant="quiet" className="sp-inbox-trigger" data-has-items={mentionsReady && mentionCount > 0 || undefined} onClick={() => { if (!open) select(mentionsReady && mentionCount > 0 ? "mentions" : "unread"); }}
      aria-label={mentionsReady ? `Innboks, ${mentionCount} uleste omtalar` : "Innboks"} title={mentionsReady ? `Innboks: ${mentionCount} uleste omtalar` : "Innboks"}>
      <svg className="sp-inbox-icon" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M4 4h16l2 10v6H2v-6L4 4Z"/><path d="M2 14h6l2 3h4l2-3h6"/></svg><span className="sp-inbox-label">Innboks</span>
      {mentionsReady && mentionCount > 0 && <span className="sp-badge" aria-hidden="true">{approximateCount(mentionCount)}</span>}
    </Button>
    <DestinationPage open={open} title="Innboks" closeLabel="Tilbake til samtalen" onClose={close}>
      {open && <>
      <nav className="sp-inbox-nav" aria-label="Innboksvising">
        <Button aria-pressed={kind === "unread"} onClick={() => select("unread")}>Uleste{state.directoryReady !== false && !state.directoryError && unreadTotal ? ` (${approximateCount(unreadTotal)})` : ""}</Button>
        <Button aria-pressed={kind === "mentions"} onClick={() => select("mentions")}>Omtalar{mentionsReady && mentionCount ? ` (${approximateCount(mentionCount)})` : ""}</Button>
        <Button aria-pressed={kind === "tasks"} onClick={() => select("tasks")}>Personlege oppgåver{tasksReady && taskCount ? ` (${approximateCount(taskCount)})` : ""}</Button>
      </nav>
      {waiting && <Status>Lastar {kind === "unread" ? "samtalar" : kind === "mentions" ? "omtalar" : "oppgåver"}.</Status>}
      {visibleError && <Status tone="error">{visibleError} {kind === "unread"
        ? host.retryDirectory && <Button onClick={() => host.retryDirectory?.()}>Prøv igjen</Button>
        : <Button disabled={loading} onClick={() => setAttempt(value => value + 1)}>Prøv igjen</Button>}</Status>}
      {!waiting && !visibleError && ready && kind === "unread" && <section aria-label="Uleste meldingar">
        {unread.length === 0 ? <><h3>Alt er lese</h3><p>Du har ingen uleste meldingar akkurat no.</p></>
          : <><p>{unreadTotal} uleste meldingar i {unread.length} {unread.length === 1 ? "samtale" : "samtalar"}</p>
            <div className="sp-inbox-list">{unread.map(({ channel, count }) => <Button className="sp-inbox-channel" variant="quiet" key={channel.id}
              aria-label={`${channel.is_direct ? channel.name : `# ${channel.name}`}, ${count} uleste meldingar`}
              onClick={() => openSource(() => host.openChannel(channel.id))}>
              <span>{channel.is_direct ? channel.name : `# ${channel.name}`}</span>
              <small>{channel.circle_id ? state.circleNames[channel.circle_id] || "Vennekrets" : channel.is_direct ? "Direktemelding" : "Felles"}</small>
              <span className="sp-badge" aria-hidden="true">{approximateCount(count)}</span>
            </Button>)}</div></>}
      </section>}
      {!waiting && !visibleError && ready && kind === "mentions" && <section aria-label="Omtalar">
        {state.mentions.length === 0 ? <><h3>Ingen omtalar</h3><p>Når nokon skriv @namnet-ditt, kjem meldinga hit.</p></>
          : state.mentions.map(mention => <MentionRow key={mention.message.id} mention={mention} host={host} context={contextFor(mention.message.channel_id, mention.channel_name)}
            onOpenSource={() => openSource(() => host.openMentionSource(mention))}
            onTaskCreated={() => { if (currentScope.current.generation === generation) select("tasks"); }} />)}
      </section>}
      {!waiting && !visibleError && ready && kind === "tasks" && <section aria-label="Oppgåver">
        {state.tasks.length === 0 ? <><h3>Ingen oppgåver</h3><p>Du kan gjere ei @omtale om til ei oppgåve.</p></>
          : state.tasks.map(task => <TaskRow key={task.id} task={task} host={host} context={contextFor(task.channel_id, task.channel_name)}
            onOpenSource={() => openSource(() => host.openTaskSource(task))} />)}
      </section>}
      </>}
    </DestinationPage>
  </>;
}
