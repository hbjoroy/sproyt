import type { AgentPlaceLookupApi } from "../../agent-place-lookup";

type Candidate = Readonly<{ id: string; revision: string }>;
type Entry = { revision: string; status: "pending" | "yes" | "no" | "error" };

/** Discovers server-approved cards in bounded batches; never fetches Maps data. */
export class AgentPlaceDiscovery {
  private scope = "";
  private generation = 0;
  private entries = new Map<string, Entry>();
  private controllers = new Set<AbortController>();
  private queue: string[] = [];
  private scheduled = false;
  private isAdmitted: () => boolean = () => false;
  private owner = "";
  private channel = "";
  private disposed = false;

  constructor(private readonly api: AgentPlaceLookupApi, private readonly changed: () => void) {}

  sync(owner: string | null, channel: string | null, accountEpoch: number, candidates: readonly Candidate[], admitted: () => boolean): void {
    if (this.disposed) return;
    const scope = owner && channel ? `${accountEpoch}:${owner}:${channel}` : "";
    if (scope !== this.scope) {
      this.clear(); this.scope = scope; this.owner = owner ?? ""; this.channel = channel ?? "";
    }
    this.isAdmitted = admitted;
    if (!scope) return;
    const present = new Set(candidates.map(candidate => candidate.id));
    for (const id of this.entries.keys()) if (!present.has(id)) this.entries.delete(id);
    for (const candidate of candidates) {
      const previous = this.entries.get(candidate.id);
      if (!previous || previous.revision !== candidate.revision) {
        this.entries.set(candidate.id, { revision: candidate.revision, status: "pending" });
        this.queue.push(candidate.id);
      }
    }
    this.schedule();
  }

  status(id: string): "pending" | "yes" | "no" | "error" | undefined { return this.entries.get(id)?.status; }

  retry(id: string): void {
    const entry = this.entries.get(id);
    if (!entry || entry.status !== "error") return;
    entry.status = "pending"; this.queue.push(id); this.schedule(); this.changed();
  }

  private schedule(): void {
    if (!this.queue.length || this.scheduled) return;
    this.scheduled = true;
    queueMicrotask(() => {
      this.scheduled = false;
      if (this.disposed || !this.scope || !this.isAdmitted()) return;
      const ids = [...new Set(this.queue)].filter(id => this.entries.get(id)?.status === "pending");
      this.queue = [];
      for (let offset = 0; offset < ids.length; offset += 30) this.request(ids.slice(offset, offset + 30));
    });
  }

  private request(ids: readonly string[]): void {
    if (!ids.length) return;
    const epoch = this.generation, scope = this.scope, owner = this.owner, channel = this.channel;
    const revisions = new Map(ids.map(id => [id, this.entries.get(id)?.revision]));
    const controller = new AbortController(); this.controllers.add(controller);
    const allowed = () => !this.disposed && !controller.signal.aborted && this.generation === epoch
      && this.scope === scope && this.isAdmitted();
    void this.api.list(owner, channel, ids, allowed, controller.signal).then(found => {
      if (!allowed()) return;
      const approved = new Set(found);
      for (const id of ids) {
        const entry = this.entries.get(id);
        if (entry?.status === "pending" && entry.revision === revisions.get(id)) entry.status = approved.has(id) ? "yes" : "no";
      }
      this.changed();
    }).catch(() => {
      if (!allowed()) return;
      for (const id of ids) {
        const entry = this.entries.get(id);
        if (entry?.status === "pending" && entry.revision === revisions.get(id)) entry.status = "error";
      }
      this.changed();
    }).finally(() => this.controllers.delete(controller));
  }

  private clear(): void {
    this.generation++; this.controllers.forEach(controller => controller.abort()); this.controllers.clear();
    this.entries.clear(); this.queue = []; this.scheduled = false;
  }
  dispose(): void { this.disposed = true; this.clear(); this.scope = ""; }
}
