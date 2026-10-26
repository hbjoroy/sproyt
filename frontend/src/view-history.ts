import { validAgentSettingsRoute, type AgentSettingsPanel, type ManagementTarget, type CommunityDestination } from "./application/management-target";
import type { ReadingPosition } from "./ui/react/timeline-scroll";

export type ViewTarget =
  | { kind: "directory"; scope: "circles" | "direct" }
  | { kind: "conversation"; channelId: string; threadId?: string; messageId?: string; sequence?: number }
  | { kind: "inbox"; inbox: "unread" | "mentions" | "tasks" }
  | { kind: "management"; page: ManagementTarget };
export type ViewPosition = { key: string; position: ReadingPosition };
type Entry = { version: 1; owner: string; session: string; id: string; index: number; target: ViewTarget; positions: ViewPosition[] };
type Recent = { target: ViewTarget; positions: ViewPosition[]; visitedAt: number };
const namespace = "sproytView";
const id = (v: unknown): v is string => typeof v === "string" && v.length > 0 && v.length <= 128 && !/[\x00-\x1f]/.test(v);
const record = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === "object" && !Array.isArray(v);
const integer = (v: unknown) => typeof v === "number" && Number.isSafeInteger(v) && v > 0;

export function readViewTarget(href: string): ViewTarget | null {
  let p: URLSearchParams;
  try { p = new URL(href).searchParams; } catch { return null; }
  const channelId = p.get("channel"), circleId = p.get("circle");
  if (p.get("view") === "circles" || p.get("view") === "direct") return { kind: "directory", scope: p.get("view") as "circles" | "direct" };
  if (p.get("view") === "inbox") {
    const inbox = p.get("inbox");
    return inbox === "unread" || inbox === "mentions" || inbox === "tasks" ? { kind: "inbox", inbox } : null;
  }
  if (p.get("view") === "settings") {
    const kind = p.get("section") ?? "overview";
    if (kind === "overview" || kind === "profile" || kind === "notifications" || kind === "welcome") return { kind: "management", page: { kind } };
    if ((kind === "circle" || kind === "memory") && id(circleId)) return { kind: "management", page: { kind, circleId } };
    if (kind === "chat-agents" && id(circleId)) {
      const agentId = p.get("agent") ?? undefined, panel = p.get("panel") ?? undefined, channel = channelId ?? undefined;
      return validAgentSettingsRoute(agentId, panel, channel) ? { kind: "management", page: { kind, circleId,
        ...(agentId !== undefined ? { agentId } : {}), ...(panel !== undefined ? { panel: panel as AgentSettingsPanel } : {}),
        ...(channel !== undefined ? { channelId: channel } : {}) } } : null;
    }
    if (kind === "advanced" && (p.get("feature") === "agent" || p.get("feature") === "heart")) {
      return { kind: "management", page: { kind, feature: p.get("feature") as "agent" | "heart", ...(id(channelId) ? { channelId } : {}) } };
    }
    if (kind === "community") {
      const task = p.get("task");
      let destination: CommunityDestination | null = null;
      if (task === "people" || task === "create-circle" || task === "circles" || task === "global-channels" || task === "global-invite" || task === "accept-invitation") destination = { kind: task };
      if (task === "create-channel" && (circleId === null || id(circleId))) destination = { kind: task, circleId };
      if (task === "channel" && id(channelId)) destination = { kind: task, channelId };
      if ((task === "channels" || task === "invite" || task === "rename-circle" || task === "circle-members") && id(circleId)) destination = { kind: task, circleId };
      return destination ? { kind: "management", page: { kind, destination } } : null;
    }
    return null;
  }
  if (!id(channelId)) return null;
  const messageId = p.get("message"), threadId = p.get("thread"), sequence = Number(p.get("sequence"));
  return { kind: "conversation", channelId, ...(id(messageId) ? { messageId } : {}),
    ...(id(threadId) ? { threadId } : {}), ...(id(messageId) && integer(sequence) ? { sequence } : {}) };
}

/** Shareable destinations contain no account identity, auth flow or invitation token. */
export function viewHref(target: ViewTarget, base: string, internal = false): string {
  const current = new URL(base), url = new URL("/", current);
  if (internal && ["localhost", "127.0.0.1", "[::1]"].includes(current.hostname)) {
    for (const key of ["ui", "participant"]) if (current.searchParams.has(key)) url.searchParams.set(key, current.searchParams.get(key)!);
  }
  const p = url.searchParams;
  if (target.kind === "directory") p.set("view", target.scope);
  else if (target.kind === "conversation") {
    p.set("channel", target.channelId);
    if (target.messageId) p.set("message", target.messageId);
    if (target.sequence && target.messageId) p.set("sequence", String(target.sequence));
    if (target.threadId) p.set("thread", target.threadId);
  } else if (target.kind === "inbox") { p.set("view", "inbox"); p.set("inbox", target.inbox); }
  else {
    const page = target.page;
    p.set("view", "settings"); p.set("section", page.kind);
    if ("circleId" in page) p.set("circle", page.circleId);
    if (page.kind === "chat-agents") {
      if (!validAgentSettingsRoute(page.agentId, page.panel, page.channelId)) throw new TypeError("Invalid agent settings destination");
      if (page.agentId !== undefined) p.set("agent", page.agentId);
      if (page.panel !== undefined) p.set("panel", page.panel);
      if (page.channelId !== undefined) p.set("channel", page.channelId);
    }
    if (page.kind === "advanced") { p.set("feature", page.feature); if (page.channelId) p.set("channel", page.channelId); }
    if (page.kind === "community") {
      const d = page.destination; p.set("task", d.kind);
      if ("circleId" in d && d.circleId) p.set("circle", d.circleId);
      if ("channelId" in d) p.set("channel", d.channelId);
    }
  }
  return url.href;
}

const targetKey = (target: ViewTarget) => viewHref(target, "https://navigation.invalid/");
function safeTarget(value: unknown): ViewTarget | null {
  if (!record(value)) return null;
  if (value.kind === "conversation" && (!id(value.channelId) || (value.messageId !== undefined && !id(value.messageId))
    || (value.threadId !== undefined && !id(value.threadId)) || (value.sequence !== undefined && !integer(value.sequence)))) return null;
  try {
    const parsed = readViewTarget(viewHref(value as ViewTarget, "https://navigation.invalid/"));
    return parsed && targetKey(parsed) === targetKey(value as ViewTarget) ? parsed : null;
  } catch { return null; }
}
function positions(value: unknown): ViewPosition[] {
  if (!Array.isArray(value)) return [];
  return value.slice(0, 2).flatMap(v => {
    if (!record(v) || typeof v.key !== "string" || !/^(channel|thread):.{1,128}$/.test(v.key) || !record(v.position)) return [];
    const p = v.position;
    if (!(p.anchorId === null || id(p.anchorId)) || typeof p.anchorOffset !== "number" || !Number.isFinite(p.anchorOffset)
      || Math.abs(p.anchorOffset) > 1e7 || typeof p.distanceFromBottom !== "number" || !Number.isFinite(p.distanceFromBottom)
      || p.distanceFromBottom < 0 || p.distanceFromBottom > 1e9 || (p.sequence !== undefined && !integer(p.sequence))) return [];
    return [{ key: v.key, position: { anchorId: p.anchorId, anchorOffset: p.anchorOffset,
      distanceFromBottom: p.distanceFromBottom, ...(integer(p.sequence) ? { sequence: p.sequence as number } : {}) } }];
  });
}
function entry(value: unknown, owner: string): Entry | null {
  if (!record(value) || value.version !== 1 || value.owner !== owner || !id(value.id) || !id(value.session)
    || !Number.isSafeInteger(value.index) || Number(value.index) < 0 || Number(value.index) > 1e6) return null;
  const target = safeTarget(value.target);
  return target ? { version: 1, owner, id: value.id, session: value.session, index: Number(value.index), target, positions: positions(value.positions) } : null;
}

/** Browser history owns Back/Forward. Recent views are a separate, bounded account-only directory. */
export class ViewHistory {
  #owner: string | null = null;
  #current: Entry | null = null;
  #maximum = 0;
  #recent: Recent[] = [];
  #generation = 0;
  #historyAvailable = true;
  #listeners = new Set<() => void>();
  #snapshot = { generation: 0, entryId: "", target: null as ViewTarget | null, positions: [] as readonly ViewPosition[], canBack: false, canForward: false, recent: [] as readonly Recent[] };
  constructor(private readonly port: { history: Pick<History, "state" | "pushState" | "replaceState" | "back" | "forward">; href(): string; storage?: Storage }) {}
  getSnapshot = () => this.#snapshot;
  subscribe = (fn: () => void) => { this.#listeners.add(fn); return () => { this.#listeners.delete(fn); }; };
  #publish() {
    const e = this.#current;
    this.#snapshot = { generation: this.#generation, entryId: e?.id ?? "", target: e?.target ?? null,
      positions: e?.positions ?? [], canBack: this.#historyAvailable && (e?.index ?? 0) > 0,
      canForward: this.#historyAvailable && !!e && e.index < this.#maximum, recent: [...this.#recent] };
    for (const fn of this.#listeners) fn();
  }
  bindOwner(owner: string) {
    if (!id(owner) || owner === this.#owner) return;
    this.#owner = owner; this.#generation++; this.#recent = []; this.#current = null; this.#maximum = 0;
    try {
      const raw = JSON.parse(this.port.storage?.getItem(`sproyt.recent-views.v1.${encodeURIComponent(owner)}`) ?? "null");
      if (record(raw) && raw.version === 1 && Array.isArray(raw.items)) this.#recent = raw.items.slice(0, 30).flatMap(item => {
        const target = record(item) ? safeTarget(item.target) : null;
        const visitedAt = record(item) ? item.visitedAt : null;
        return target && typeof visitedAt === "number" && visitedAt > Date.now() - 30 * 86400_000 && visitedAt < Date.now() + 60_000
          ? [{ target, positions: positions(item.positions), visitedAt }] : [];
      });
    } catch { /* optional local metadata */ }
    const state = this.port.history.state;
    const existing = record(state) ? entry(state[namespace], owner) : null;
    const target = readViewTarget(this.port.href());
    if (existing && target && targetKey(existing.target) === targetKey(target)) { this.#current = existing; this.#maximum = existing.index; }
    this.#publish();
  }
  #write(mode: "push" | "replace", href?: string) {
    const state = this.port.history.state;
    const next = { ...(record(state) ? state : {}), [namespace]: this.#current };
    try { this.port.history[mode === "push" ? "pushState" : "replaceState"](next, "", href ?? this.port.href()); }
    catch { this.#historyAvailable = false; /* navigation works; traversal cannot safely be advertised */ }
  }
  capture(value: readonly ViewPosition[]) {
    if (!this.#current) return;
    const state = this.port.history.state;
    if (!record(state) || !record(state[namespace]) || state[namespace].id !== this.#current.id) return;
    this.#current = { ...this.#current, positions: positions(value) };
    this.#write("replace");
  }
  navigate(target: ViewTarget, mode: "push" | "replace" = "push") {
    const normalized = safeTarget(target);
    if (!this.#owner || !normalized) return;
    target = normalized;
    const previous = this.#current;
    if (previous && targetKey(previous.target) === targetKey(target)) { this.#publish(); return; }
    if (previous) this.#remember(previous);
    const replace = mode === "replace" || !previous;
    this.#current = { version: 1, owner: this.#owner, session: previous?.session ?? crypto.randomUUID(), id: crypto.randomUUID(),
      index: replace ? previous?.index ?? 0 : (previous?.index ?? 0) + 1, target, positions: [] };
    this.#maximum = this.#current.index; this.#generation++;
    this.#write(replace ? "replace" : "push", viewHref(target, this.port.href(), true));
    this.#publish();
  }
  restore(state: unknown): ViewTarget | null {
    if (!this.#owner) return null;
    const next = record(state) ? entry(state[namespace], this.#owner) : null;
    if (!next || !this.#current || next.session !== this.#current.session) return null;
    this.#current = next; this.#generation++; this.#publish(); return next.target;
  }
  adopt(target: ViewTarget) { this.#current = null; this.#maximum = 0; this.navigate(target, "replace"); }
  openRecent(target: ViewTarget) {
    const saved = this.recent(target);
    this.navigate(target);
    if (this.#current) { this.#current = { ...this.#current, positions: positions(saved) }; this.#write("replace"); this.#publish(); }
  }
  recent(target: ViewTarget): readonly ViewPosition[] { return this.#recent.find(r => targetKey(r.target) === targetKey(target))?.positions ?? []; }
  #remember(e: Entry) {
    this.#recent = [{ target: e.target, positions: e.positions, visitedAt: Date.now() }, ...this.#recent.filter(r => targetKey(r.target) !== targetKey(e.target))].slice(0, 30);
    try { this.port.storage?.setItem(`sproyt.recent-views.v1.${encodeURIComponent(e.owner)}`, JSON.stringify({ version: 1, items: this.#recent })); } catch { /* optional */ }
  }
  back() { if (this.#snapshot.canBack) this.port.history.back(); }
  forward() { if (this.#snapshot.canForward) this.port.history.forward(); }
}
