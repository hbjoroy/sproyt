import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { conversationPreferenceKey, decodeConversationPreferences, emptyConversationPreferences, MAX_CIRCLE_USES,
  MAX_NAVIGATION_ENTRIES, MAX_PINNED_CIRCLES, partitionNavigationCircles, readConversationPreferences, recordCircleSelection,
  resetConversationPreferences, setCircleExpanded, toggleCirclePinned, writeConversationPreferences } from "./conversation-preferences";
import type { ConversationPreferenceStorage } from "./conversation-preferences";
import { ProductConversationNavigation } from "./ui/react/conversation-navigation";
import type { ConversationGroup } from "./ui/react/conversation-view";

class MemoryPreferences implements ConversationPreferenceStorage {
  readonly values = new Map<string, string>();
  getItem(key: string) { return this.values.get(key) ?? null; }
  setItem(key: string, value: string) { this.values.set(key, value); }
  removeItem(key: string) { this.values.delete(key); }
}

/** Call from the boundary entry point to register this bounded feature's tests. */
export function runConversationPreferenceTests() {
  test("conversation preferences never borrow another account or an anonymous legacy value", () => {
    const storage = new MemoryPreferences();
    const alice = toggleCirclePinned(setCircleExpanded(emptyConversationPreferences(), "circle:one", true), "circle:one");
    writeConversationPreferences(storage, "alice", alice);
    storage.setItem("sproyt:conversation-navigation", JSON.stringify(alice));
    assert.deepEqual(readConversationPreferences(storage, "alice"), alice);
    assert.deepEqual(readConversationPreferences(storage, "bob"), emptyConversationPreferences());
    assert.deepEqual(readConversationPreferences(storage), emptyConversationPreferences());
    writeConversationPreferences(storage, undefined, alice);
    assert.equal(storage.values.size, 2);
    assert.notEqual(conversationPreferenceKey("alice:a"), conversationPreferenceKey("alice%3Aa"));
    resetConversationPreferences(storage, "alice");
    assert.deepEqual(readConversationPreferences(storage, "alice"), emptyConversationPreferences());
    assert.equal(storage.values.size, 1);
  });

  test("navigation survives unavailable storage, quota failures, malformed JSON and unknown versions", () => {
    const unavailable: ConversationPreferenceStorage = {
      getItem() { throw new Error("denied"); }, setItem() { throw new Error("quota"); }, removeItem() { throw new Error("denied"); }
    };
    assert.deepEqual(readConversationPreferences(unavailable, "owner"), emptyConversationPreferences());
    assert.doesNotThrow(() => writeConversationPreferences(unavailable, "owner", emptyConversationPreferences()));
    assert.doesNotThrow(() => resetConversationPreferences(unavailable, "owner"));
    assert.deepEqual(readConversationPreferences(null, "owner"), emptyConversationPreferences());
    const storage = new MemoryPreferences();
    for (const raw of ["{", "null", "[]", "x".repeat(100_001), JSON.stringify({ version: 2, entries: [] })]) {
      storage.setItem(conversationPreferenceKey("owner")!, raw);
      assert.deepEqual(readConversationPreferences(storage, "owner"), emptyConversationPreferences());
    }
    assert.equal(conversationPreferenceKey("x".repeat(257)), null);
    assert.equal(conversationPreferenceKey("\ud800"), null);
  });

  test("stored navigation data is bounded, sanitized, deduplicated and carries only presentation fields", () => {
    const value = decodeConversationPreferences({ version: 1, othersExpanded: "yes", entries: [
      { id: "circle:a", pinned: true, uses: 1e50, expanded: false, body: "private text" },
      { id: "circle:a", uses: 22 }, { id: "circle:b", uses: -3, expanded: "yes" },
      { id: "circle:c", uses: Number.NaN }, { id: "x".repeat(257), uses: 1 }, null
    ] });
    assert.deepEqual(value, { version: 1, othersExpanded: false, entries: [
      { id: "circle:a", pinned: true, uses: MAX_CIRCLE_USES, expanded: false },
      { id: "circle:b", pinned: false, uses: 0 }, { id: "circle:c", pinned: false, uses: 0 }
    ] });
    const large = decodeConversationPreferences({ version: 1, entries: Array.from({ length: 500 }, (_, index) =>
      ({ id: `circle:${index}`, pinned: true, uses: 1 })) });
    assert.equal(large.entries.length, MAX_NAVIGATION_ENTRIES);
    assert.equal(large.entries.filter(entry => entry.pinned).length, MAX_PINNED_CIRCLES);
    assert.ok(!JSON.stringify(value).includes("private text"));
  });

  test("explicit selection is immutable, saturates uses and preserves pins under bounded eviction", () => {
    const initial = toggleCirclePinned(emptyConversationPreferences(), "circle:pinned");
    let current = initial;
    for (let index = 0; index < 200; index++) current = recordCircleSelection(current, `circle:${index}`);
    assert.equal(initial.entries.length, 1);
    assert.equal(initial.entries[0]?.uses, 0);
    assert.equal(current.entries.length, MAX_NAVIGATION_ENTRIES);
    assert.ok(current.entries.some(entry => entry.id === "circle:pinned" && entry.pinned));
    for (let index = 0; index < 1100; index++) current = recordCircleSelection(current, "circle:recent");
    assert.equal(current.entries.find(entry => entry.id === "circle:recent")?.uses, MAX_CIRCLE_USES);
    const expanded = setCircleExpanded(current, "circle:recent", true);
    assert.equal(expanded.entries.find(entry => entry.id === "circle:recent")?.uses, MAX_CIRCLE_USES);
    assert.equal(setCircleExpanded(initial, "", true), initial);
    let pins = emptyConversationPreferences();
    for (let index = 0; index < MAX_PINNED_CIRCLES + 3; index++) pins = toggleCirclePinned(pins, `circle:${index}`);
    assert.equal(pins.entries.filter(entry => entry.pinned).length, MAX_PINNED_CIRCLES);
    assert.equal(toggleCirclePinned(pins, "circle:extra"), pins);
    assert.equal(toggleCirclePinned(pins, "circle:0").entries.filter(entry => entry.pinned).length, MAX_PINNED_CIRCLES - 1);
  });

  test("preferred circles retain source order, include deep-link targets and exclude unavailable links", () => {
    const groups = Array.from({ length: 30 }, (_, index) => ({ id: `circle:${index}` }));
    let preferences = toggleCirclePinned(emptyConversationPreferences(), "circle:20");
    preferences = recordCircleSelection(preferences, "circle:10");
    preferences = toggleCirclePinned(preferences, "circle:missing");
    const first = partitionNavigationCircles(groups, preferences, "circle:29");
    assert.deepEqual(first.preferred.map(group => group.id), ["circle:10", "circle:20", "circle:29"]);
    assert.equal(first.others.length, 27);
    const again = partitionNavigationCircles(groups, recordCircleSelection(preferences, "circle:10"), "circle:29");
    assert.deepEqual(again, first);
    assert.equal(partitionNavigationCircles(groups, emptyConversationPreferences()).preferred.length, 4);
  });

  test("navigation renders collapsed headers, active destinations, independent menus and direct context", () => {
    const groups: ConversationGroup[] = Array.from({ length: 30 }, (_, index) => ({ id: `circle:${index}`, name: `Krets ${index}`,
      actions: createElement("button", { type: "button" }, `Meny ${index}`),
      conversations: [{ id: `channel:${index}`, name: `Kanal ${index}`, unread: index === 0 ? 3 : 0,
        action: createElement("button", { type: "button" }, `Varsling ${index}`) }] }));
    groups.push({ id: "scope:direct", name: "Direkte", conversations: [{ id: "dm", name: "Ada" }] });
    const props = { groups, selectedId: "channel:29", query: "", onQueryChange() {}, onSelect() {},
      actions: createElement("button", { type: "button" }, "Ny direktemelding") };
    const html = renderToStaticMarkup(createElement(ProductConversationNavigation, props));
    assert.ok(html.includes("Andre kretsar (29)"));
    assert.ok(html.includes('aria-label="3 uleste"'));
    assert.ok(html.includes('aria-expanded="false"'));
    assert.ok(html.includes('aria-current="page"'));
    assert.ok(html.includes('aria-label="Fest Krets 29 som favoritt"'));
    assert.ok(html.includes("Meny 29"));
    assert.ok(html.includes("Varsling 29"));
    assert.ok(!html.match(/<button[^>]*>(?:(?!<\/button>)[\s\S])*<button/));
    const direct = renderToStaticMarkup(createElement(ProductConversationNavigation, { ...props, selectedId: "dm" }));
    assert.match(direct, /data-scope="direct" aria-selected="true"/);
    assert.ok(direct.includes("Ny direktemelding"));
    const search = renderToStaticMarkup(createElement(ProductConversationNavigation, { ...props, query: "Krets" }));
    assert.ok(!search.includes('aria-expanded="false"'));
    assert.ok(search.includes('aria-label="søkjetreff"'));
  });

  test("explicit directory scope and host recency control direct presentation while preserving all conversations", () => {
    const html = renderToStaticMarkup(createElement(ProductConversationNavigation, {
      groups: [{ id: "scope:direct", name: "Direkte", conversations: [
        { id: "first", name: "Først frå verten" }, { id: "recent", name: "Sist brukt" }, { id: "last", name: "Siste frå verten" }
      ] }], selectedId: null, query: "", onQueryChange() {}, onSelect() {},
      directoryScope: "direct", recentChannelIds: ["missing", "recent", "recent"]
    }));
    assert.match(html, /data-scope="direct" aria-selected="true"/);
    assert.ok(html.indexOf("Sist brukt") < html.indexOf("Først frå verten"));
    assert.ok(html.indexOf("Først frå verten") < html.indexOf("Siste frå verten"));
    assert.equal(html.match(/class="sp-conversation-name"/g)?.length, 3);
  });

  test("empty navigation has a useful accessible state without showing a tour", () => {
    const html = renderToStaticMarkup(createElement(ProductConversationNavigation, {
      groups: [], selectedId: null, query: "", onQueryChange() {}, onSelect() {}
    }));
    assert.ok(html.includes("Ingen kretsar eller kanalar enno."));
    assert.ok(html.includes('role="tabpanel"'));
    assert.ok(html.includes("Finn samtale"));
  });
}
