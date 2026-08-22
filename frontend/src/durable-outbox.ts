/**
 * A deliberately small, user-scoped journal for messages which have crossed
 * the composer boundary but have not yet been acknowledged by the server.
 *
 * It contains no credentials or blobs.  The request id is the idempotency key:
 * replaying an entry is safe, while trying to infer delivery from its body is
 * not.  IndexedDB is best effort on iOS, so every operation is bounded and a
 * failed journal never leaves the composer permanently disabled.
 */
export type DurableMedia = Readonly<{ id: string; contentType: string; originalFilename: string }>;
export type DurableSend = Readonly<{
  version: 1;
  userId: string;
  requestId: string;
  channelId: string;
  parentMessageId: string | null;
  body: string;
  draft: string;
  media: readonly DurableMedia[];
  createdAt: number;
  attempts: number;
}>;

export type DurableOutboxProblem = "unavailable" | "quota" | "capacity" | "corrupt" | "identity";
export class DurableOutboxError extends Error {
  constructor(readonly problem: DurableOutboxProblem, message: string) { super(message); }
}

export interface DurableOutboxStorage {
  list(userId: string): Promise<readonly DurableSend[]>;
  put(entry: DurableSend): Promise<void>;
  delete(userId: string, requestId: string): Promise<void>;
}

export interface DurableOutbox {
  setUser(userId: string | null): Promise<readonly DurableSend[]>;
  enqueue(entry: Omit<DurableSend, "version" | "userId" | "createdAt" | "attempts">): Promise<DurableSend>;
  acknowledge(requestId: string): Promise<void>;
  permanentFailure(requestId: string): Promise<DurableSend | null>;
  pending(): readonly DurableSend[];
}

const maxEntries = 20;
const maxBytes = 1024 * 1024;
const maxAgeMs = 7 * 24 * 60 * 60 * 1000;
const operationTimeoutMs = 3_000;
const dbName = "sproyt-durable-outbox";
const storeName = "pending-sends";
const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const isMedia = (value: unknown): value is DurableMedia => isRecord(value) && typeof value.id === "string" && typeof value.contentType === "string" && typeof value.originalFilename === "string";

function isEntry(value: unknown): value is DurableSend {
  return isRecord(value) && value.version === 1 && typeof value.userId === "string" && typeof value.requestId === "string"
    && typeof value.channelId === "string" && (value.parentMessageId === null || typeof value.parentMessageId === "string")
    && typeof value.body === "string" && typeof value.draft === "string" && Array.isArray(value.media) && value.media.every(isMedia)
    && typeof value.createdAt === "number" && Number.isFinite(value.createdAt) && typeof value.attempts === "number" && Number.isSafeInteger(value.attempts);
}
function entryBytes(entry: DurableSend): number { return new TextEncoder().encode(JSON.stringify(entry)).byteLength; }
class IndexedDbStorage implements DurableOutboxStorage {
  private database: Promise<IDBDatabase> | null = null;
  private open(): Promise<IDBDatabase> {
    if (!("indexedDB" in globalThis)) return Promise.reject(new DurableOutboxError("unavailable", "Mellombels lagring er ikkje tilgjengeleg."));
    if (this.database) return this.database;
    this.database = new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open(dbName, 1);
      request.onupgradeneeded = () => { if (!request.result.objectStoreNames.contains(storeName)) request.result.createObjectStore(storeName, { keyPath: "key" }); };
      let settled = false;
      const timer = globalThis.setTimeout(() => { settled = true; reject(new DurableOutboxError("unavailable", "Mellombels lagring svarar ikkje.")); }, operationTimeoutMs);
      request.onsuccess = () => {
        if (settled) { request.result.close(); return; }
        settled = true; globalThis.clearTimeout(timer);
        request.result.onversionchange = () => request.result.close();
        resolve(request.result);
      };
      request.onerror = () => { if (!settled) { settled = true; globalThis.clearTimeout(timer); reject(request.error ?? new Error("IndexedDB kunne ikkje opnast.")); } };
      request.onblocked = () => { if (!settled) { settled = true; globalThis.clearTimeout(timer); reject(new Error("IndexedDB er blokkert.")); } };
    }).catch((error: unknown) => { this.database = null; throw error; });
    return this.database;
  }
  private async transaction<T>(mode: IDBTransactionMode, action: (store: IDBObjectStore) => IDBRequest<T> | void): Promise<T | void> {
    const db = await this.open();
    return new Promise<T | void>((resolve, reject) => {
      let transaction: IDBTransaction;
      try { transaction = mode === "readwrite" ? db.transaction(storeName, mode, { durability: "strict" }) : db.transaction(storeName, mode); }
      catch { transaction = db.transaction(storeName, mode); }
      let settled = false;
      const finish = (callback: () => void): void => {
        if (settled) return;
        settled = true;
        globalThis.clearTimeout(timer);
        callback();
      };
      const timer = globalThis.setTimeout(() => {
        try { transaction.abort(); } catch { /* transaction may already be complete */ }
        finish(() => reject(new DurableOutboxError("unavailable", "Mellombels lagring svarar ikkje.")));
      }, operationTimeoutMs);
      let result: T | void;
      transaction.oncomplete = () => finish(() => resolve(result));
      transaction.onerror = () => finish(() => reject(transaction.error ?? new Error("IndexedDB-transaksjon feila.")));
      transaction.onabort = () => finish(() => reject(transaction.error ?? new Error("IndexedDB-transaksjon vart avbroten.")));
      const request = action(transaction.objectStore(storeName));
      if (request) { request.onsuccess = () => { result = request.result; }; request.onerror = () => finish(() => reject(request.error ?? new Error("IndexedDB-forespurnad feila."))); }
    }).catch((error: unknown) => { try { db.close(); } catch { /* already closed */ } this.database = null; throw error; });
  }
  async list(userId: string): Promise<readonly DurableSend[]> {
    const rows = await this.transaction<unknown[]>("readonly", (store) => store.getAll());
    return (rows ?? []).filter(isRecord).map((row) => row.entry).filter(isEntry).filter((entry) => entry.userId === userId);
  }
  async put(entry: DurableSend): Promise<void> { await this.transaction("readwrite", (store) => store.put({ key: `${entry.userId}:${entry.requestId}`, entry })); }
  async delete(userId: string, requestId: string): Promise<void> { await this.transaction("readwrite", (store) => store.delete(`${userId}:${requestId}`)); }
}

export function createDurableOutbox(storage: DurableOutboxStorage = new IndexedDbStorage(), now: () => number = () => Date.now()): DurableOutbox {
  let userId: string | null = null;
  let entries = new Map<string, DurableSend>();
  let generation = 0;
  let loading: Promise<readonly DurableSend[]> | null = null;
  const removed = new Set<string>();
  const requireUser = (): string => { if (!userId) throw new DurableOutboxError("unavailable", "Ventar på innlogging før meldinga kan lagrast."); return userId; };
  const safeDelete = async (entry: DurableSend): Promise<void> => { try { await storage.delete(entry.userId, entry.requestId); } catch { /* expiry cleanup must not hide valid entries */ } };
  return {
    async setUser(nextUserId: string | null): Promise<readonly DurableSend[]> {
      if (nextUserId === userId) return loading ?? [...entries.values()].sort((left, right) => left.createdAt - right.createdAt);
      userId = nextUserId; entries = new Map(); removed.clear(); generation += 1;
      if (!nextUserId) return [];
      const loadGeneration = generation;
      const load = (async (): Promise<readonly DurableSend[]> => {
        let loaded: readonly DurableSend[];
        try { loaded = await storage.list(nextUserId); } catch (error) {
          if (loadGeneration === generation && userId === nextUserId) userId = null;
          throw new DurableOutboxError("unavailable", error instanceof Error ? error.message : "Kunne ikkje lese mellombels lagring.");
        }
        if (loadGeneration !== generation || userId !== nextUserId) return [...entries.values()];
        const cutoff = now() - maxAgeMs;
        let loadedBytes = 0;
        for (const entry of loaded) {
          if (entry.userId !== nextUserId) continue;
          if (entry.createdAt < cutoff) { void safeDelete(entry); continue; }
          if (removed.has(entry.requestId)) continue;
          loadedBytes += entryBytes(entry);
          if (entries.size >= maxEntries || loadedBytes > maxBytes) throw new DurableOutboxError("capacity", "Det ligg for mange usende meldingar. Opne att appen og avklar dei først.");
          entries.set(entry.requestId, entry);
        }
        return [...entries.values()].sort((left, right) => left.createdAt - right.createdAt);
      })();
      loading = load;
      void load.finally(() => { if (loading === load) loading = null; }).catch(() => {});
      return load;
    },
    async enqueue(input): Promise<DurableSend> {
      const owner = requireUser();
      const enqueueGeneration = generation;
      const entry: DurableSend = { ...input, version: 1, userId: owner, createdAt: now(), attempts: 0 };
      if (entryBytes(entry) > maxBytes) throw new DurableOutboxError("capacity", "Meldinga er for stor til trygg mellombels lagring.");
      const total = [...entries.values()].reduce((sum, item) => sum + entryBytes(item), entryBytes(entry));
      if (entries.size >= maxEntries || total > maxBytes) throw new DurableOutboxError("capacity", "Det ligg for mange usende meldingar. Opne att appen og avklar dei først.");
      try { await storage.put(entry); } catch (error) {
        const name = error instanceof DOMException ? error.name : "";
        throw new DurableOutboxError(name === "QuotaExceededError" ? "quota" : "unavailable", error instanceof Error ? error.message : "Kunne ikkje lagre meldinga.");
      }
      if (generation !== enqueueGeneration || userId !== owner) {
        throw new DurableOutboxError("identity", "Innlogginga vart endra medan meldinga blei lagra. Meldinga vart ikkje send.");
      }
      entries.set(entry.requestId, entry);
      return entry;
    },
    async acknowledge(requestId): Promise<void> {
      const owner = userId; const operationGeneration = generation;
      if (owner) await storage.delete(owner, requestId);
      if (operationGeneration !== generation || owner !== userId) return;
      removed.add(requestId);
      entries.delete(requestId);
    },
    async permanentFailure(requestId): Promise<DurableSend | null> {
      const owner = userId; const operationGeneration = generation; const entry = entries.get(requestId) ?? null;
      removed.add(requestId);
      entries.delete(requestId);
      if (owner) await storage.delete(owner, requestId);
      if (operationGeneration !== generation || owner !== userId) return entry;
      return entry;
    },
    pending: () => [...entries.values()].sort((left, right) => left.createdAt - right.createdAt)
  };
}
