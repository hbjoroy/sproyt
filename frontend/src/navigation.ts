import { parseStoredStringMap, type Identifier, type StoredStringMap } from "./types";

export type RootScope = "shared" | "circle" | "direct";
export type NavigationChannel = Readonly<{ id: Identifier; slug: string; name: string; circle_id: Identifier | null; direct_user_id: Identifier | null }>;
export type NavigationSnapshot = Readonly<{ activeChannelId: Identifier | null; activeCircleId: Identifier | null; activeRootScope: RootScope; restoredChannelId: Identifier | null; restoredCircleId: Identifier | null; lastChannelByCircle: StoredStringMap }>;

const activeChannelKey = "sproyt.active-channel.v1";
const activeCircleKey = "sproyt.active-circle.v1";
const circleChannelHistoryKey = "sproyt.active-channel-by-circle.v1";
const channelDraftPrefix = "sproyt.channel-draft.v1.";
const threadDraftPrefix = "sproyt.thread-draft.v1.";

/** Owns persistent navigation and composer-draft state; the DOM only renders it. */
export class NavigationController {
  #activeChannelId: Identifier | null = null;
  #activeCircleId: Identifier | null = null;
  #activeRootScope: RootScope = "shared";
  #restoredChannelId: Identifier | null;
  #restoredCircleId: Identifier | null;
  #lastChannelByCircle: StoredStringMap;

  constructor(private readonly storage: Storage, location: Pick<Location, "href">) {
    const linkedChannelId = storedIdentifier(new URL(location.href).searchParams.get("channel"));
    this.#restoredChannelId = linkedChannelId ?? storedIdentifier(readStorage(storage, activeChannelKey));
    this.#restoredCircleId = storedIdentifier(readStorage(storage, activeCircleKey));
    this.#lastChannelByCircle = parseStoredStringMap(readStorage(storage, circleChannelHistoryKey));
  }

  get activeChannelId(): Identifier | null { return this.#activeChannelId; }
  get activeCircleId(): Identifier | null { return this.#activeCircleId; }
  get activeRootScope(): RootScope { return this.#activeRootScope; }
  get restoredChannelId(): Identifier | null { return this.#restoredChannelId; }
  get restoredCircleId(): Identifier | null { return this.#restoredCircleId; }
  get snapshot(): NavigationSnapshot { return { activeChannelId: this.#activeChannelId, activeCircleId: this.#activeCircleId, activeRootScope: this.#activeRootScope, restoredChannelId: this.#restoredChannelId, restoredCircleId: this.#restoredCircleId, lastChannelByCircle: { ...this.#lastChannelByCircle } }; }

  setActiveChannel(channel: NavigationChannel): void {
    this.#activeChannelId = channel.id;
    this.#restoredChannelId = channel.id;
    writeStorage(this.storage, activeChannelKey, channel.id);
    if (channel.circle_id) { this.setActiveCircle(channel.circle_id); this.rememberCircleChannel(channel); }
    else { this.clearActiveCircle(); this.#activeRootScope = channel.direct_user_id ? "direct" : "shared"; }
  }
  clearActiveChannel(channelId: Identifier | null = null): void {
    if (channelId && this.#activeChannelId !== channelId && this.#restoredChannelId !== channelId) return;
    this.#activeChannelId = null; this.#restoredChannelId = null; removeStorage(this.storage, activeChannelKey);
  }
  deactivateChannel(): void { this.#activeChannelId = null; }
  setActiveCircle(circleId: Identifier): void { this.#activeCircleId = circleId; this.#restoredCircleId = circleId; this.#activeRootScope = "circle"; writeStorage(this.storage, activeCircleKey, circleId); }
  clearActiveCircle(circleId: Identifier | null = null): void {
    if (circleId && this.#activeCircleId !== circleId && this.#restoredCircleId !== circleId) return;
    this.#activeCircleId = null; this.#restoredCircleId = null; removeStorage(this.storage, activeCircleKey);
  }
  activateRootScope(scope: Exclude<RootScope, "circle">): void { this.clearActiveCircle(); this.#activeRootScope = scope; }
  restoreActiveCircle(circleIds: Iterable<Identifier>, selectedCircleId: Identifier | null): Identifier | null {
    const available = new Set(circleIds);
    const candidate = [this.#activeCircleId, this.#restoredCircleId, selectedCircleId].find((id): id is Identifier => id !== null && available.has(id));
    const fallback = candidate ?? available.values().next().value ?? null;
    if (fallback === null) { this.clearActiveCircle(); return null; }
    this.setActiveCircle(fallback); return fallback;
  }
  rememberCircleChannel(channel: NavigationChannel): void {
    if (!channel.circle_id) return;
    this.#lastChannelByCircle[channel.circle_id] = channel.id;
    writeStorage(this.storage, circleChannelHistoryKey, JSON.stringify(this.#lastChannelByCircle));
  }
  forgetCircleChannel(circleId: Identifier): void {
    if (!(circleId in this.#lastChannelByCircle)) return;
    delete this.#lastChannelByCircle[circleId]; writeStorage(this.storage, circleChannelHistoryKey, JSON.stringify(this.#lastChannelByCircle));
  }
  preferredCircleChannel<Channel extends NavigationChannel>(circleId: Identifier, channels: readonly Channel[]): Channel | undefined {
    const available = channels.filter((channel) => channel.circle_id === circleId);
    const remembered = available.find((channel) => channel.id === this.#lastChannelByCircle[circleId]);
    const primary = available.find((channel) => channel.name.trim().toLocaleLowerCase() === "prat" || channel.slug === scopedCircleChannelSlug(circleId, "prat"));
    return remembered ?? primary ?? available[0];
  }
  persistChannelDraft(channelId: Identifier | null, draft: string): void { if (channelId) this.persistDraft(`${channelDraftPrefix}${channelId}`, draft); }
  restoreChannelDraft(channelId: Identifier | null): string { return channelId ? readStorage(this.storage, `${channelDraftPrefix}${channelId}`) ?? "" : ""; }
  persistThreadDraft(channelId: Identifier | null, rootId: Identifier | null, draft: string): void { if (channelId && rootId) this.persistDraft(`${threadDraftPrefix}${channelId}.${rootId}`, draft); }
  restoreThreadDraft(channelId: Identifier, rootId: Identifier): string { return readStorage(this.storage, `${threadDraftPrefix}${channelId}.${rootId}`) ?? ""; }
  clearThreadDraft(channelId: Identifier | null, rootId: Identifier | null): void { if (channelId && rootId) removeStorage(this.storage, `${threadDraftPrefix}${channelId}.${rootId}`); }
  private persistDraft(key: string, draft: string): void { if (draft) writeStorage(this.storage, key, draft); else removeStorage(this.storage, key); }
}

export function restoreNavigation(storage: Storage, location: Pick<Location, "href">): NavigationSnapshot { return new NavigationController(storage, location).snapshot; }

function scopedCircleChannelSlug(circleId: string, value: string): string {
  const scope = circleId.replace(/-/g, "");
  const base = value.trim().toLocaleLowerCase().replace(/[^a-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "") || "kanal";
  return `${scope}-${base.slice(0, 47)}`;
}
function readStorage(storage: Storage, key: string): string | null { try { return storage.getItem(key); } catch { return null; } }
function storedIdentifier(value: string | null): Identifier | null { return value !== null && value.length > 0 && value.length <= 128 ? value : null; }
function writeStorage(storage: Storage, key: string, value: string): void { try { storage.setItem(key, value); } catch { /* storage can be unavailable */ } }
function removeStorage(storage: Storage, key: string): void { try { storage.removeItem(key); } catch { /* storage can be unavailable */ } }
