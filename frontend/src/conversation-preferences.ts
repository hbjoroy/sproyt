/** Presentation preferences only. Never store conversation names or message data. */
export const CONVERSATION_PREFERENCES_VERSION = 1;
export const MAX_NAVIGATION_ENTRIES = 128;
export const MAX_PINNED_CIRCLES = 12;
export const MAX_CIRCLE_USES = 999;
const MAX_ID_LENGTH = 256;
const MAX_STORED_LENGTH = 100_000;

export interface CircleNavigationPreference {
  readonly id: string;
  readonly pinned: boolean;
  readonly uses: number;
  readonly expanded?: boolean;
}

export interface ConversationPreferences {
  readonly version: 1;
  readonly entries: readonly CircleNavigationPreference[];
  readonly othersExpanded: boolean;
}

export interface ConversationPreferenceStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export function emptyConversationPreferences(): ConversationPreferences {
  return { version: CONVERSATION_PREFERENCES_VERSION, entries: [], othersExpanded: false };
}

function validId(id: unknown): id is string {
  return typeof id === "string" && id.length > 0 && id.length <= MAX_ID_LENGTH;
}

/** An absent account never falls back to a shared or legacy key. */
export function conversationPreferenceKey(ownerId?: string): string | null {
  if (!validId(ownerId)) return null;
  try { return `sproyt:conversation-navigation:v1:${encodeURIComponent(ownerId)}`; }
  catch { return null; }
}

export function decodeConversationPreferences(value: unknown): ConversationPreferences {
  if (!value || typeof value !== "object") return emptyConversationPreferences();
  const record = value as Record<string, unknown>;
  if (record.version !== CONVERSATION_PREFERENCES_VERSION || !Array.isArray(record.entries)) return emptyConversationPreferences();
  const entries: CircleNavigationPreference[] = [];
  const seen = new Set<string>();
  let pins = 0;
  for (const item of record.entries.slice(0, MAX_NAVIGATION_ENTRIES)) {
    if (!item || typeof item !== "object" || !validId(item.id) || seen.has(item.id)) continue;
    seen.add(item.id);
    const pinned = item.pinned === true && pins < MAX_PINNED_CIRCLES;
    if (pinned) pins++;
    entries.push({ id: item.id, pinned,
      uses: typeof item.uses === "number" && Number.isFinite(item.uses)
        ? Math.min(MAX_CIRCLE_USES, Math.max(0, Math.floor(item.uses))) : 0,
      ...(typeof item.expanded === "boolean" ? { expanded: item.expanded } : {}) });
  }
  return { version: CONVERSATION_PREFERENCES_VERSION, entries, othersExpanded: record.othersExpanded === true };
}

export function readConversationPreferences(storage: ConversationPreferenceStorage | null | undefined, ownerId?: string): ConversationPreferences {
  const key = conversationPreferenceKey(ownerId);
  if (!storage || !key) return emptyConversationPreferences();
  try {
    const raw = storage.getItem(key);
    if (!raw || raw.length > MAX_STORED_LENGTH) return emptyConversationPreferences();
    return decodeConversationPreferences(JSON.parse(raw));
  } catch { return emptyConversationPreferences(); }
}

export function writeConversationPreferences(storage: ConversationPreferenceStorage | null | undefined, ownerId: string | undefined,
  preferences: ConversationPreferences): void {
  const key = conversationPreferenceKey(ownerId);
  if (!storage || !key) return;
  try { storage.setItem(key, JSON.stringify(decodeConversationPreferences(preferences))); }
  catch { /* Quota/private browsing failures must not block navigation. */ }
}

export function resetConversationPreferences(storage: ConversationPreferenceStorage | null | undefined, ownerId?: string): void {
  const key = conversationPreferenceKey(ownerId);
  if (!storage || !key) return;
  try { storage.removeItem(key); } catch { /* Optional local preference storage. */ }
}

function updateCircle(preferences: ConversationPreferences, id: string,
  change: (entry: CircleNavigationPreference) => CircleNavigationPreference): ConversationPreferences {
  if (!validId(id)) return preferences;
  const current = preferences.entries.find(entry => entry.id === id) ?? { id, pinned: false, uses: 0 };
  const next = change(current);
  const entries = preferences.entries.filter(entry => entry.id !== id);
  // Keep pins when a very large account evicts old local preferences.
  if (entries.length >= MAX_NAVIGATION_ENTRIES) {
    const evict = entries.findIndex(entry => !entry.pinned);
    entries.splice(evict < 0 ? 0 : evict, 1);
  }
  entries.push(next);
  return { ...preferences, entries };
}

export function setCircleExpanded(preferences: ConversationPreferences, id: string, expanded: boolean): ConversationPreferences {
  return updateCircle(preferences, id, entry => ({ ...entry, expanded }));
}

export function toggleCirclePinned(preferences: ConversationPreferences, id: string): ConversationPreferences {
  const existing = preferences.entries.find(entry => entry.id === id);
  if (!existing?.pinned && preferences.entries.filter(entry => entry.pinned).length >= MAX_PINNED_CIRCLES) return preferences;
  return updateCircle(preferences, id, entry => ({ ...entry, pinned: !entry.pinned }));
}

/** Call only from an explicit channel-selection gesture, never from rendering. */
export function recordCircleSelection(preferences: ConversationPreferences, id: string): ConversationPreferences {
  return updateCircle(preferences, id, entry => ({ ...entry, uses: Math.min(MAX_CIRCLE_USES, entry.uses + 1) }));
}

/** A short preferred set; preserve server order within each partition so the
 * sidebar does not reshuffle on every use. Missing groups never become links. */
export function partitionNavigationCircles<T extends { readonly id: string }>(groups: readonly T[], preferences: ConversationPreferences,
  activeGroupId?: string): { readonly preferred: readonly T[]; readonly others: readonly T[] } {
  const available = new Set(groups.map(group => group.id));
  const preferredIds = new Set(preferences.entries.filter(entry => entry.pinned && available.has(entry.id)).map(entry => entry.id));
  const frequent = preferences.entries.filter(entry => entry.uses > 0 && !entry.pinned && available.has(entry.id))
    .slice().sort((a, b) => b.uses - a.uses || a.id.localeCompare(b.id)).slice(0, 4);
  for (const entry of frequent) preferredIds.add(entry.id);
  if (activeGroupId) preferredIds.add(activeGroupId);
  if (preferredIds.size === 0) for (const group of groups.slice(0, 4)) preferredIds.add(group.id);
  return { preferred: groups.filter(group => preferredIds.has(group.id)), others: groups.filter(group => !preferredIds.has(group.id)) };
}
