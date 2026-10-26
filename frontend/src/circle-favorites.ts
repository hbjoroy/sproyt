import { HttpClient } from "./api";

export type CircleFavoritesSnapshot = Readonly<{
  ownerId: string | null; ids: readonly string[]; pending: readonly string[];
  loaded: boolean; loading: boolean; error: string;
}>;
export const emptyCircleFavorites: CircleFavoritesSnapshot = Object.freeze({ ownerId: null, ids: [], pending: [], loaded: false, loading: false, error: "" });
export interface CircleFavoritesApi {
  list(owner: string, admitted: () => boolean): Promise<readonly string[]>;
  set(owner: string, circle: string, pinned: boolean, admitted: () => boolean): Promise<void>;
}
export class ProfileCircleFavoritesApi implements CircleFavoritesApi {
  constructor(private readonly http: HttpClient) {}
  list(owner: string, admitted: () => boolean): Promise<readonly string[]> {
    return this.http.json("/api/v1/me/favorite-circles", value => {
      if (!Array.isArray(value) || value.length > 12 || !value.every(id => typeof id === "string" && /^[\da-f]{8}(-[\da-f]{4}){3}-[\da-f]{12}$/i.test(id)))
        throw new Error("Ugyldig favorittliste frå tenaren.");
      return [...new Set(value as string[])];
    }, { headers: { "x-sproyt-account": owner }, signal: AbortSignal.timeout(15_000) }, admitted);
  }
  set(owner: string, circle: string, pinned: boolean, admitted: () => boolean): Promise<void> {
    return this.http.empty(`/api/v1/me/favorite-circles/${encodeURIComponent(circle)}`, {
      method: pinned ? "PUT" : "DELETE", headers: { "x-sproyt-account": owner }, signal: AbortSignal.timeout(15_000)
    }, admitted);
  }
}

/** Profile-owned pins. Expansion/use counters stay device-local. Never replace
 * the entire server set: another browser may have changed a different circle. */
export class CircleFavorites {
  #state: CircleFavoritesSnapshot = emptyCircleFavorites;
  #listeners = new Set<() => void>();
  #generation = 0;
  #mutation = 0;
  #read = 0;
  #lastRead = 0;
  #queue: Promise<void> = Promise.resolve();
  #failed: { circle: string; pinned: boolean } | null = null;
  constructor(private readonly api: CircleFavoritesApi, private readonly currentOwner: () => string | null) {}
  readonly getSnapshot = () => this.#state;
  readonly subscribe = (listener: () => void) => { this.#listeners.add(listener); return () => { this.#listeners.delete(listener); }; };
  #publish(change: Partial<CircleFavoritesSnapshot>) {
    this.#state = Object.freeze({ ...this.#state, ...change });
    this.#listeners.forEach(listener => listener());
  }
  #admitted(owner: string, generation: number) {
    return this.#generation === generation && this.#state.ownerId === owner && this.currentOwner() === owner;
  }
  bindOwner(owner: string | null): void {
    if (this.#state.ownerId !== owner) {
      this.#generation++; this.#read++; this.#mutation++; this.#lastRead = 0;
      this.#queue = Promise.resolve(); this.#failed = null;
      this.#state = Object.freeze({ ...emptyCircleFavorites, ownerId: owner });
      this.#listeners.forEach(listener => listener());
    }
    if (owner) void this.refresh(false);
  }
  async refresh(force = true): Promise<void> {
    const owner = this.#state.ownerId, generation = this.#generation;
    if (!owner || this.#state.pending.length || this.#state.loading || (!force && Date.now() - this.#lastRead < 5000)) return;
    const mutation = this.#mutation, read = ++this.#read;
    this.#publish({ loading: true });
    try {
      const ids = await this.api.list(owner, () => this.#admitted(owner, generation));
      if (this.#admitted(owner, generation) && mutation === this.#mutation && read === this.#read) {
        this.#lastRead = Date.now();
        this.#publish({ ids, loaded: true, error: this.#failed ? this.#state.error : "" });
      }
    } catch (error) {
      if (this.#admitted(owner, generation) && mutation === this.#mutation && read === this.#read)
        this.#publish({ error: error instanceof Error ? error.message : "Favorittane kunne ikkje hentast." });
    } finally {
      if (this.#admitted(owner, generation) && read === this.#read) this.#publish({ loading: false });
    }
  }
  set(circle: string, pinned: boolean, expectedOwner?: string): Promise<void> {
    const owner = this.#state.ownerId, generation = this.#generation;
    if (!owner || (expectedOwner !== undefined && expectedOwner !== owner) || !this.#state.loaded || this.#state.pending.includes(circle)) return Promise.resolve();
    this.#mutation++;
    this.#publish({ pending: [...this.#state.pending, circle], error: "" });
    const operation = this.#queue.then(async () => {
      if (!this.#admitted(owner, generation)) return;
      try {
        await this.api.set(owner, circle, pinned, () => this.#admitted(owner, generation));
        if (!this.#admitted(owner, generation)) return;
        const ids = this.#state.ids.filter(id => id !== circle);
        if (pinned) ids.push(circle);
        if (this.#failed?.circle === circle) this.#failed = null;
        this.#publish({ ids });
      } catch (error) {
        if (this.#admitted(owner, generation)) {
          this.#failed = { circle, pinned };
          this.#publish({ error: error instanceof Error ? error.message : "Favoritten kunne ikkje lagrast. Prøv igjen." });
        }
      } finally {
        if (this.#admitted(owner, generation)) this.#publish({ pending: this.#state.pending.filter(id => id !== circle) });
      }
    });
    this.#queue = operation;
    return operation.then(async () => {
      if (this.#admitted(owner, generation) && !this.#state.pending.length) {
        // The earlier GET may be stale; invalidate it before a final read.
        this.#read++; this.#publish({ loading: false });
        await this.refresh();
      }
    });
  }
  async transfer(owner: string, circles: readonly string[]): Promise<void> {
    const generation = this.#generation;
    for (const circle of circles.slice(0, 12)) {
      if (!this.#admitted(owner, generation)) return;
      await this.set(circle, true, owner);
      if (!this.#admitted(owner, generation) || this.#state.error) return;
    }
  }
  retry(expectedOwner?: string): void {
    if (expectedOwner !== undefined && expectedOwner !== this.#state.ownerId) return;
    const failed = this.#failed;
    if (failed) void this.set(failed.circle, failed.pinned);
    else void this.refresh();
  }
}
