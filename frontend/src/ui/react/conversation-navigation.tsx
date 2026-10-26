import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react";
import type { KeyboardEvent } from "react";
import { Button, Status, TextField } from "@sproyt/ui/react";
import type { ConversationGroup, ConversationNavigationProps } from "./conversation-view";
import { conversationPreferenceKey, MAX_PINNED_CIRCLES, partitionNavigationCircles, readConversationPreferences,
  recordCircleSelection, setCircleExpanded, toggleCirclePinned, writeConversationPreferences } from "../../conversation-preferences";
import type { ConversationPreferences, ConversationPreferenceStorage } from "../../conversation-preferences";
import { emptyCircleFavorites } from "../../circle-favorites";

const noSubscription = () => () => {};
const emptyFavoritesSnapshot = () => emptyCircleFavorites;

type NavigationProps = ConversationNavigationProps & {
  readonly ownerId?: string;
  /** Host-owned recency, newest first. Remaining direct conversations stay visible. */
  readonly recentChannelIds?: readonly string[];
};
type Scope = "circles" | "direct";
const directGroupId = "scope:direct";

function browserStorage(): ConversationPreferenceStorage | null {
  try { return typeof window === "undefined" ? null : window.localStorage; } catch { return null; }
}

function unreadCount(groups: readonly ConversationGroup[]): number {
  return groups.reduce((total, group) => total + group.conversations.reduce((count, item) =>
    count + (typeof item.unread === "number" && Number.isFinite(item.unread) ? Math.max(0, item.unread) : 0), 0), 0);
}

function UnreadBadge({ count }: { readonly count: number }) {
  return count > 0 ? <span className="sp-badge" aria-label={`${count} uleste`}>{count > 999 ? "999+" : count}</span> : null;
}

/** The keyed inner component prevents the previous account's choices flashing
 * or being written under a new owner during account changes. */
export function ProductConversationNavigation(props: NavigationProps) {
  return <AccountNavigation key={conversationPreferenceKey(props.ownerId) ?? "unscoped"} {...props} />;
}

function AccountNavigation(props: NavigationProps) {
  const storage = useRef(browserStorage());
  const [localPreferences, setPreferences] = useState(() => readConversationPreferences(storage.current, props.ownerId));
  const saved = useSyncExternalStore(props.favorites?.subscribe ?? noSubscription,
    props.favorites?.getSnapshot ?? emptyFavoritesSnapshot, emptyFavoritesSnapshot);
  const profileReady = !props.favorites || (saved.ownerId === props.ownerId && saved.loaded);
  const pinned = new Set(saved.ownerId === props.ownerId ? saved.ids.map(id => `circle:${id}`) : []);
  const preferences: ConversationPreferences = props.favorites ? { ...localPreferences, entries: [
    ...localPreferences.entries.map(entry => ({ ...entry, pinned: pinned.has(entry.id) })),
    ...[...pinned].filter(id => !localPreferences.entries.some(entry => entry.id === id)).map(id => ({ id, pinned: true, uses: 0 }))
  ] } : localPreferences;
  const selectedGroup = props.groups.find(group => group.conversations.some(item => item.id === props.selectedId));
  const selectedScope: Scope | undefined = selectedGroup ? selectedGroup.id === directGroupId ? "direct" : "circles" : undefined;
  const [localScope, setScope] = useState<Scope>(() => selectedScope ?? "circles");
  const scope = props.directoryScope ?? localScope;
  const [revealedGroup, setRevealedGroup] = useState<string | undefined>(() => selectedGroup?.id);
  const selection = useRef(props.selectedId);
  const id = useId();
  const searching = props.query.trim().length > 0;
  const legacyPins = props.favorites && profileReady ? localPreferences.entries.filter(entry => entry.pinned && !pinned.has(entry.id)
    && props.groups.some(group => group.id === entry.id)).map(entry => entry.id) : [];
  const directGroups = props.groups.filter(group => group.id === directGroupId);
  const sharedGroups = props.groups.filter(group => group.id === "scope:shared");
  const circles = props.groups.filter(group => group.id !== directGroupId && group.id !== "scope:shared");
  const { preferred, others } = partitionNavigationCircles(circles, preferences, selectedGroup?.id);
  const foldOthers = circles.length > 7 && others.length > 0;
  const othersOpen = searching || preferences.othersExpanded;

  useEffect(() => {
    if (selection.current === props.selectedId) return;
    if (props.selectedId && !selectedGroup) return;
    selection.current = props.selectedId;
    if (selectedScope) setScope(selectedScope);
    setRevealedGroup(selectedGroup?.id);
  }, [props.selectedId, selectedScope, selectedGroup?.id]);

  useEffect(() => {
    const key = conversationPreferenceKey(props.ownerId);
    const changed = (event: StorageEvent) => {
      if (event.key === null || event.key === key) setPreferences(readConversationPreferences(storage.current, props.ownerId));
    };
    window.addEventListener("storage", changed);
    return () => window.removeEventListener("storage", changed);
  }, [props.ownerId]);

  useEffect(() => {
    if (!props.favorites || !profileReady || !localPreferences.entries.some(entry => entry.pinned && pinned.has(entry.id))) return;
    const local = { ...localPreferences, entries: localPreferences.entries.map(entry => ({ ...entry, pinned: entry.pinned && !pinned.has(entry.id) })) };
    setPreferences(local);
    writeConversationPreferences(storage.current, props.ownerId, local);
  }, [props.favorites, props.ownerId, profileReady, saved.ids, localPreferences]);

  function update(next: ConversationPreferences) {
    // Retain untransferred legacy pins; server pins are never written back as
    // local import candidates, which could resurrect a removal on another device.
    const local = props.favorites ? { ...next, entries: next.entries.map(entry => ({ ...entry,
      pinned: localPreferences.entries.some(old => old.id === entry.id && old.pinned) && !pinned.has(entry.id) })) } : next;
    setPreferences(local);
    writeConversationPreferences(storage.current, props.ownerId, local);
  }

  function select(group: ConversationGroup, channelId: string) {
    if (group.id.startsWith("circle:")) update(recordCircleSelection(preferences, group.id));
    props.onSelect(channelId);
  }

  function rows(group: ConversationGroup) {
    const recent = new Map<string, number>();
    if (group.id === directGroupId) for (const channelId of props.recentChannelIds ?? []) {
      if (!recent.has(channelId)) recent.set(channelId, recent.size);
    }
    const conversations = recent.size ? group.conversations.map((item, index) => ({ item, index }))
      .sort((a, b) => (recent.get(a.item.id) ?? Number.MAX_SAFE_INTEGER) - (recent.get(b.item.id) ?? Number.MAX_SAFE_INTEGER) || a.index - b.index)
      .map(({ item }) => item) : group.conversations;
    return conversations.length ? <div className="sp-conversations">
      {conversations.map(item => <div className="sp-conversation-row" key={item.id}>
        <button type="button" className="sp-conversation" aria-current={item.id === props.selectedId ? "page" : undefined}
          onClick={() => select(group, item.id)}>
          <span className="sp-conversation-name">{item.name}</span><UnreadBadge count={item.unread ?? 0} />
        </button>
        {item.action && <div className="sp-conversation-actions">{item.action}</div>}
      </div>)}
    </div> : <p className="sp-product-nav-empty">{group.id === directGroupId ? "Ingen direktemeldingar enno." : "Ingen kanalar i denne kretsen enno."}</p>;
  }

  function groupSection(group: ConversationGroup) {
    const preference = preferences.entries.find(entry => entry.id === group.id);
    const isCircle = group.id.startsWith("circle:");
    const open = searching || revealedGroup === group.id || (preference?.expanded ?? selectedGroup?.id === group.id);
    const panelId = `${id}-group-${encodeURIComponent(group.id)}`;
    const pinLimit = !preference?.pinned && preferences.entries.filter(entry => entry.pinned).length >= MAX_PINNED_CIRCLES;
    const saving = saved.ownerId === props.ownerId && saved.pending.includes(group.id.slice("circle:".length));
    return <section className="sp-product-nav-group" key={group.id} data-conversation-group={group.id}>
      <div className="sp-product-nav-heading">
        <h2><button type="button" title={group.name} className="sp-product-nav-disclosure" aria-expanded={open} aria-controls={panelId} aria-disabled={searching || undefined}
          onClick={() => { if (searching) return; setRevealedGroup(undefined); update(setCircleExpanded(preferences, group.id, !open)); }}>
          <span className="sp-product-nav-chevron" aria-hidden="true">{open ? "⌄" : "›"}</span>
          <span className="sp-product-nav-name">{group.name}</span><UnreadBadge count={unreadCount([group])} />
        </button></h2>
        {isCircle && <button type="button" className="sp-product-nav-pin" aria-pressed={preference?.pinned ?? false}
          aria-label={preference?.pinned ? `Løys ${group.name} frå favorittar` : `Fest ${group.name} som favoritt`}
          title={pinLimit ? `Du kan feste opptil ${MAX_PINNED_CIRCLES} kretsar` : undefined} disabled={pinLimit || !profileReady || saving}
          aria-busy={saving || undefined}
          onClick={() => props.favorites ? void props.favorites.set(group.id.slice("circle:".length), !preference?.pinned, props.ownerId) : update(toggleCirclePinned(preferences, group.id))}>
          <span aria-hidden="true">{preference?.pinned ? "★" : "☆"}</span>
        </button>}
        {group.actions && <div className="sp-product-nav-group-actions">{group.actions}</div>}
      </div>
      <div id={panelId} hidden={!open}>{rows(group)}</div>
    </section>;
  }

  function tabKey(event: KeyboardEvent<HTMLButtonElement>) {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const next: Scope = event.key === "Home" ? "circles" : event.key === "End" ? "direct" : scope === "circles" ? "direct" : "circles";
    setScope(next);
    props.onDirectoryScopeChange?.(next);
    event.currentTarget.parentElement?.querySelector<HTMLButtonElement>(`[data-scope="${next}"]`)?.focus();
  }

  return <div className="sp-product-navigation">
    <TextField label="Finn samtale" type="search" value={props.query}
      onChange={event => props.onQueryChange(event.currentTarget.value)} />
    <div className="sp-product-nav-tabs" role="tablist" aria-label="Samtalar">
      {(["circles", "direct"] as const).map(value => <button key={value} type="button" role="tab"
        id={`${id}-${value}-tab`} data-scope={value} aria-selected={scope === value} aria-controls={`${id}-${value}-panel`}
        tabIndex={scope === value ? 0 : -1} onKeyDown={tabKey} onClick={() => { setScope(value); props.onDirectoryScopeChange?.(value); }}>
        <span>{value === "circles" ? "Kretsar" : "Direktemeldingar"}</span>
        <UnreadBadge count={unreadCount(value === "circles" ? [...sharedGroups, ...circles] : directGroups)} />
        {searching && <span className="sp-product-nav-match-count" aria-label="søkjetreff">
          {(value === "circles" ? [...sharedGroups, ...circles] : directGroups).reduce((count, group) => count + group.conversations.length, 0)}
        </span>}
      </button>)}
    </div>
    <div role="tabpanel" id={`${id}-circles-panel`} aria-labelledby={`${id}-circles-tab`} hidden={scope !== "circles"}>
      {saved.ownerId === props.ownerId && saved.error && <Status tone="error">{saved.error} <button type="button" onClick={() => props.favorites?.retry(props.ownerId)}>Prøv favorittane på nytt</button></Status>}
      {legacyPins.length > 0 && <details className="sp-product-nav-legacy-pins"><summary>Tidlegare lokale favorittar</summary>
        <p>Desse vart berre lagra i denne nettlesaren. Du kan overføre dei til kontoen din.</p>
        <Button variant="quiet" disabled={saved.pending.length > 0} onClick={() => {
          if (props.ownerId) void props.favorites?.transfer(props.ownerId, legacyPins.map(id => id.slice("circle:".length)));
        }}>Overfør lokale favorittar</Button>
      </details>}
      {sharedGroups.map(groupSection)}
      {(foldOthers ? preferred : circles).map(groupSection)}
      {foldOthers && <div className="sp-product-nav-others">
        <button type="button" className="sp-product-nav-disclosure" aria-expanded={othersOpen} aria-controls={`${id}-others`} aria-disabled={searching || undefined}
          onClick={() => { if (!searching) update({ ...preferences, othersExpanded: !othersOpen }); }}>
          <span className="sp-product-nav-chevron" aria-hidden="true">{othersOpen ? "⌄" : "›"}</span>
          <span className="sp-product-nav-name">Andre kretsar ({others.length})</span><UnreadBadge count={unreadCount(others)} />
        </button>
        <div id={`${id}-others`} hidden={!othersOpen}>{others.map(groupSection)}</div>
      </div>}
      {sharedGroups.length + circles.length === 0 && <Status>{searching ? "Ingen kretsar eller kanalar passar søket." : "Ingen kretsar eller kanalar enno."}</Status>}
      {sharedGroups.length + circles.length > 0 && !searching && [...sharedGroups, ...circles].every(group => !group.conversations.length)
        && <p className="sp-product-nav-empty">Ingen kanalar enno. Opne menyen ved ein krets for å finne eller opprette ein kanal.</p>}
    </div>
    <div role="tabpanel" id={`${id}-direct-panel`} aria-labelledby={`${id}-direct-tab`} hidden={scope !== "direct"}>
      <div className="sp-product-nav-direct-actions">{props.actions}</div>
      {directGroups.map(group => <section key={group.id} data-conversation-group={group.id} aria-label="Direktemeldingar">
        {group.actions}{rows(group)}
      </section>)}
      {directGroups.length === 0 && <Status>{searching ? "Ingen direktemeldingar passar søket." : "Ingen direktemeldingar enno. Start ei ny samtale."}</Status>}
    </div>
  </div>;
}
