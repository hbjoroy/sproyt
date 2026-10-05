import "../../assets/share-inbox.js";
import type { DurableSend, DurableMedia } from "./durable-outbox";

export type ShareAdmission = Omit<DurableSend, "version" | "userId" | "createdAt" | "attempts">;
export type ShareReceipt = Readonly<{ id: string; owner: string | null; generation: string; createdAt: number;
  text: string; file: File | null; channelId?: string; admission: ShareAdmission | null; done: number | null; rejected?: boolean }>;
export interface ShareInbox {
  identity(): Promise<string | null>;
  generation(): Promise<string>;
  capture(form: FormData, generation?: string): Promise<string>;
  list(owner: string): Promise<readonly ShareReceipt[]>;
  claim(id: string, owner: string, generation: string): Promise<ShareReceipt>;
  edit(id: string, owner: string, generation: string, text: string, channelId: string): Promise<ShareReceipt>;
  admit(id: string, owner: string, generation: string, admission: ShareAdmission): Promise<ShareReceipt>;
  owned(id: string, owner: string, generation: string): Promise<void>;
  reject(id: string, owner: string, generation: string): Promise<void>;
  finish(id: string, owner: string, generation: string): Promise<void>;
  discard(id: string, owner: string, generation: string): Promise<void>;
  logout(): Promise<void>;
}
declare global { var SproytShareInbox: ShareInbox; }
export const shareInbox = globalThis.SproytShareInbox;
type ShareState = Readonly<{ receipts: readonly ShareReceipt[]; busy: boolean; error: string; notice: string }>;

/** The receipt stores the immutable admission before outbox ownership. A crash
 * in either DB can retry only that same request id and payload, never a new send. */
export function createShareTarget(options: {
  user: () => string | null;
  upload: (channelId: string, file: File) => Promise<DurableMedia>;
  enqueue: (admission: ShareAdmission, owner: string) => Promise<void>;
  drop: (requestId: string) => Promise<void>;
  dispatch: (admission: ShareAdmission) => void;
}, inbox: ShareInbox = shareInbox) {
  let state: ShareState = { receipts: [], busy: false, error: "", notice: "" };
  let epoch = 0;
  let draftWrites = 0;
  const localDrafts = new Map<string, { text: string; channelId: string }>();
  let shareIds = new Set<string>();
  const listeners = new Set<() => void>();
  const publish = (next: Partial<ShareState>) => { state = { ...state, ...next }; listeners.forEach(listener => listener()); };
  const refresh = async () => {
    const currentEpoch = ++epoch, user = options.user();
    if (!user) { shareIds.clear(); publish({ receipts: [] }); return; }
    const receipts = await inbox.list(user);
    if (epoch === currentEpoch && user === options.user()) {
      shareIds = new Set(receipts.filter(item => item.admission).map(item => item.id));
      publish({ receipts: receipts.filter(item => !item.done).map(item => ({ ...item, ...localDrafts.get(item.id) })) });
    }
  };
  async function authorized(): Promise<{ user: string; generation: string }> {
    const generation = await inbox.generation(), user = await inbox.identity();
    if (!user || user !== options.user()) throw new Error("Kontroller innlogginga før du tek delinga i bruk.");
    return { user, generation };
  }
  async function run(action: () => Promise<void>): Promise<void> {
    if (state.busy) return;
    const user = options.user();
    publish({ busy: true, error: "", notice: "" });
    try { await action(); }
    catch (error) { if (user === options.user()) publish({ error: error instanceof Error ? error.message : "Delinga kunne ikkje behandlast. Prøv igjen." }); }
    finally { if (user === options.user()) { await refresh().catch(() => {}); publish({ busy: false }); } }
  }
  return {
    getSnapshot: () => state,
    canReload: () => !state.busy && draftWrites === 0 && localDrafts.size === 0,
    canReplay(requestId: string) { return !state.receipts.some(item => item.id === requestId && item.rejected); },
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    refresh,
    clear() { epoch++; shareIds.clear(); localDrafts.clear(); publish({ receipts: [], busy: false, error: "", notice: "" }); },
    async edit(id: string, channelId: string, text: string) {
      const user = options.user(); if (!user) return;
      const draft = { text, channelId }; localDrafts.set(id, draft);
      publish({ receipts: state.receipts.map(item => item.id === id ? { ...item, ...draft } : item) });
      draftWrites++;
      try {
        const generation = await inbox.generation();
        if (user !== options.user()) return;
        await inbox.edit(id, user, generation, text, channelId);
        if (localDrafts.get(id) === draft) localDrafts.delete(id);
      } catch (error) {
        if (user === options.user()) publish({ error: error instanceof Error ? error.message : "Utkastet kunne ikkje lagrast lokalt." });
      } finally { draftWrites--; }
    },
    claim(id: string) { return run(async () => { const { user, generation } = await authorized(); await inbox.claim(id, user, generation); }); },
    discard(id: string) { return run(async () => {
      const { user, generation } = await authorized();
      const item = state.receipts.find(item => item.id === id);
      if (item?.rejected && item.admission) await options.drop(item.admission.requestId);
      await inbox.discard(id, user, generation);
      localDrafts.delete(id);
    }); },
    send(id: string, channelId: string, text: string) { return run(async () => {
      const { user, generation } = await authorized();
      let item = state.receipts.find(receipt => receipt.id === id && receipt.owner === user);
      if (!item) throw new Error("Ta delinga i bruk med denne kontoen først.");
      let admission = item.admission;
      if (!admission) {
        item = await inbox.edit(id, user, generation, text, channelId);
        localDrafts.delete(id);
        if (user !== options.user()) throw new Error("Innlogginga vart endra. Delinga er ikkje send.");
        const media = item.file ? [await options.upload(channelId, item.file)] : [];
        if (user !== options.user()) throw new Error("Innlogginga vart endra. Delinga er ikkje send.");
        const body = [text.trim(), ...media.map(file => `[[media:${file.id}|${file.contentType}|${encodeURIComponent(file.originalFilename)}]]`)].filter(Boolean).join("\n");
        if (!body) throw new Error("Skriv tekst eller vel eit bilete før sending.");
        admission = { requestId: id, channelId, parentMessageId: null, body, draft: text, media, source: "share-target" };
        const admitted = await inbox.admit(id, user, generation, admission);
        shareIds.add(id);
        publish({ receipts: state.receipts.map(item => item.id === id ? admitted : item) });
      }
      // No reduced-durability native fallback: the raw receipt survives a
      // failed enqueue, and recovery reuses this exact immutable admission.
      if (user !== options.user()) throw new Error("Innlogginga vart endra. Delinga er ikkje send.");
      await options.enqueue(admission, user);
      // A deliberate retry may leave the former definitive rejection behind.
      publish({ receipts: state.receipts.map(item => item.id === id ? { ...item, rejected: false } : item) });
      await inbox.owned(id, user, generation);
      if (user !== options.user()) throw new Error("Innlogginga vart endra. Meldinga er ikkje send frå denne økta.");
      options.dispatch(admission);
      publish({ notice: "Delinga er lagd i sendekøen. Ho blir stadfesta når tenesta svarar." });
    }); },
    async accepted(requestId: string) {
      const item = state.receipts.find(receipt => receipt.admission?.requestId === requestId);
      if (!item || !item.owner) return false;
      await inbox.finish(item.id, item.owner, await inbox.generation());
      await refresh(); publish({ notice: "Delinga er send." }); return true;
    },
    isShare(requestId: string) { return shareIds.has(requestId); },
    failed(message: string) { publish({ error: `Delinga er ikkje stadfesta: ${message}. Prøv den opphavlege sendinga igjen.` }); },
    async rejected(requestId: string, message: string) {
      const item = state.receipts.find(item => item.admission?.requestId === requestId);
      if (!item?.owner) return;
      await inbox.reject(item.id, item.owner, await inbox.generation());
      await refresh();
      publish({ error: `Tenesta avviste delinga: ${message}. Du kan forkaste henne eller prøve den opphavlege sendinga igjen.` });
    }
  };
}
export type ShareTarget = ReturnType<typeof createShareTarget>;
