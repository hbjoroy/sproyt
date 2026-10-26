import { Button, Dialog, Message, Status, TextField } from "@sproyt/ui/react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { Channel, Mention, UserTask } from "../../types";

export type PreviewInboxKind = "unread" | "mentions" | "tasks";

export interface PreviewInboxState {
  readonly channels: readonly Channel[];
  readonly mentions: readonly Mention[];
  readonly tasks: readonly UserTask[];
  readonly participantId: string | null;
  readonly circleNames: Readonly<Record<string, string>>;
}

export interface PreviewInboxHost {
  load(kind: "mentions" | "tasks"): Promise<void>;
  openChannel(channelId: string): void;
  openMentionSource(mention: Mention): void;
  openTaskSource(task: UserTask): void;
  markMentionRead(messageId: string): Promise<void>;
  createTask(input: Readonly<{ sourceMessageId: string; title: string; processLinkId: string | null }>): Promise<void>;
  setTaskDone(taskId: string, done: boolean): Promise<void>;
}

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
const approximateCount = (count: number) => count < 25 ? String(count) : count < 50 ? "25+" : count < 100 ? "50+" : "100+";

function MentionRow({ mention, host, onTaskCreated, onOpenSource }: {
  readonly mention: Mention;
  readonly host: PreviewInboxHost;
  readonly onTaskCreated: () => void;
  readonly onOpenSource?: () => void;
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
  return <Message author={`${mention.message.sender_display_name} i ${mention.channel_name}`}
    time={new Date(mention.message.sent_at).toLocaleString("nn-NO")}
    status={mention.read ? "Lesen" : "Ulest"}
    actions={<>
      <Button onClick={onOpenSource ?? (() => host.openMentionSource(mention))}>Opne kjelda</Button>
      {!mention.read && <Button busy={busy === "read"}
        onClick={() => void run("read", () => host.markMentionRead(mention.message.id))}>Marker lesen</Button>}
      <Button aria-expanded={editingTask} onClick={() => setEditingTask(value => !value)}>Lag oppgåve</Button>
    </>}>
    <p style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{mention.message.body}</p>
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
  </Message>;
}

function TaskRow({ task, host, onOpenSource }: { readonly task: UserTask; readonly host: PreviewInboxHost; readonly onOpenSource: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const done = task.status === "done";
  return <article className="sp-inbox-row" data-done={done || undefined}>
    <h3>{task.title}</h3>
    <p>{task.channel_name}{task.process_link_id ? ` · Heart ${task.process_link_id}` : ""}</p>
    <Button busy={busy} onClick={() => {
      if (busy) return;
      setBusy(true); setError("");
      void host.setTaskDone(task.id, !done).catch(error => setError(errorText(error))).finally(() => setBusy(false));
    }}>{done ? "Opne igjen" : "Ferdig"}</Button>
    <Button onClick={onOpenSource}>Opne kjelda</Button>
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
  const [portalHost, setPortalHost] = useState<Element | null>(null);
  useLayoutEffect(() => { setPortalHost(trigger.current?.closest(".sp-theme") ?? null); }, []);
  const latestHost = useRef(host); latestHost.current = host;
  const scope = `${state.participantId}:${open ? kind : "closed"}`;
  const currentScope = useRef({ key: scope, generation: 0 });
  if (currentScope.current.key !== scope) currentScope.current = { key: scope, generation: currentScope.current.generation + 1 };
  const generation = currentScope.current.generation;
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
  const mentionCount = state.mentions.filter(mention => !mention.read).length;
  const taskCount = state.tasks.filter(task => task.status !== "done").length;
  const attentionCount = unreadTotal + mentionCount + taskCount;
  const select = (next: PreviewInboxKind) => {
    setLocalKind(next);
    if (!controlled) setLocalOpen(true);
    onNavigate?.(next);
  };
  const close = () => { if (controlled) onBack?.(); else setLocalOpen(false); };
  // Source commands own their next route. Closing the old controlled route
  // afterwards would traverse history and undo the source navigation.
  const openSource = (action: () => void) => {
    action();
    if (!controlled) setLocalOpen(false);
  };
  return <>
    <Button ref={trigger} variant="quiet" className="sp-inbox-trigger" data-has-items={attentionCount > 0 || undefined} onClick={() => select(kind)}
      aria-label={`Innboks og oppgåver${attentionCount ? `, ${attentionCount} nye eller opne element` : ""}`}
      title={attentionCount ? `Innboks: ${attentionCount} nye eller opne` : "Innboks og oppgåver"}>
      <span aria-hidden="true">▤</span><span className="sp-inbox-label">Innboks</span>
      {attentionCount > 0 && <span className="sp-badge" aria-hidden="true">{approximateCount(attentionCount)}</span>}
    </Button>
    {portalHost && createPortal(<Dialog open={open} title="Innboks og oppgåver" closeLabel="Tilbake til samtalen" onClose={close}>
      {open && <>
      <nav aria-label="Innboksvising" style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        <Button aria-pressed={kind === "unread"} onClick={() => select("unread")}>Uleste{unreadTotal ? ` (${approximateCount(unreadTotal)})` : ""}</Button>
        <Button aria-pressed={kind === "mentions"} onClick={() => select("mentions")}>Omtalar{mentionCount ? ` (${approximateCount(mentionCount)})` : ""}</Button>
        <Button aria-pressed={kind === "tasks"} onClick={() => select("tasks")}>Oppgåver{taskCount ? ` (${approximateCount(taskCount)})` : ""}</Button>
      </nav>
      {loading && <Status>Lastar {kind === "mentions" ? "omtalar" : "oppgåver"} …</Status>}
      {error && <Status tone="error">{error} <Button disabled={loading} onClick={() => setAttempt(value => value + 1)}>Prøv igjen</Button></Status>}
      {!loading && !error && kind === "unread" && <section aria-label="Uleste meldingar">
        {unread.length === 0 ? <><h3>Alt er lese</h3><p>Du har ingen uleste meldingar akkurat no.</p></>
          : <><p>{unreadTotal} uleste meldingar i {unread.length} {unread.length === 1 ? "samtale" : "samtalar"}</p>
            <div style={{ display: "grid", gap: 8 }}>{unread.map(({ channel, count }) => <Button key={channel.id}
              aria-label={`${channel.is_direct ? channel.name : `# ${channel.name}`}, ${count} uleste meldingar`}
              onClick={() => openSource(() => host.openChannel(channel.id))}>
              <span>{channel.is_direct ? channel.name : `# ${channel.name}`}</span>
              <small>{channel.circle_id ? state.circleNames[channel.circle_id] || "Vennekrets" : channel.is_direct ? "Direktemelding" : "Felles"}</small>
              <span className="sp-badge" aria-hidden="true">{approximateCount(count)}</span>
            </Button>)}</div></>}
      </section>}
      {!loading && !error && kind === "mentions" && <section aria-label="Omtalar">
        {state.mentions.length === 0 ? <><h3>Ingen omtalar</h3><p>Når nokon skriv @namnet-ditt, kjem meldinga hit.</p></>
          : state.mentions.map(mention => <MentionRow key={mention.message.id} mention={mention} host={host}
            onOpenSource={() => openSource(() => host.openMentionSource(mention))}
            onTaskCreated={() => { if (currentScope.current.generation === generation) select("tasks"); }} />)}
      </section>}
      {!loading && !error && kind === "tasks" && <section aria-label="Oppgåver">
        {state.tasks.length === 0 ? <><h3>Ingen oppgåver</h3><p>Du kan gjere ei @omtale om til ei oppgåve.</p></>
          : state.tasks.map(task => <TaskRow key={task.id} task={task} host={host}
            onOpenSource={() => openSource(() => host.openTaskSource(task))} />)}
      </section>}
      </>}
    </Dialog>, portalHost)}
  </>;
}
