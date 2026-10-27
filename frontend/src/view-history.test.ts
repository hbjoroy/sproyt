import assert from "node:assert/strict";
import test from "node:test";
import { ViewHistory, readViewTarget, viewHref, type ViewTarget } from "./view-history";
import { NavigationController } from "./navigation";
import { agentSettingsPanels, managementParent, type ManagementTarget } from "./application/management-target";

function fixture(storage?: Storage) {
  const entries = [{ state: { otherFeature: "keep" } as unknown, href: "https://chat.test/?invite=secret" }];
  let cursor = 0;
  const port = { storage, href: () => entries[cursor]!.href, history: {
    get state() { return entries[cursor]!.state; },
    replaceState(state: unknown, _unused: string, url?: string | URL | null) { entries[cursor] = { state: structuredClone(state), href: url ? String(url) : entries[cursor]!.href }; },
    pushState(state: unknown, _unused: string, url?: string | URL | null) { entries.splice(cursor + 1); entries.push({ state: structuredClone(state), href: String(url) }); cursor++; },
    back() { if (cursor) cursor--; }, forward() { if (cursor < entries.length - 1) cursor++; }
  } };
  return { port, entries, history: new ViewHistory(port) };
}
function storage(): Storage {
  const data = new Map<string, string>();
  return { get length() { return data.size; }, clear: () => data.clear(), key: n => [...data.keys()][n] ?? null,
    getItem: key => data.get(key) ?? null, setItem: (key, value) => { data.set(key, value); }, removeItem: key => { data.delete(key); } };
}
const channel = (channelId: string): ViewTarget => ({ kind: "conversation", channelId });
const anchor = (anchorId: string, sequence: number) => [{ key: "channel:a", position: { anchorId, sequence, anchorOffset: -12, distanceFromBottom: 300 } }];

const agentId = "00000000-0000-7000-8000-000000001001";
const agentChannel = "00000000-0000-7000-8000-000000001002";
test("agent settings panels and resource context round-trip as independent destinations", () => {
  const pages: ManagementTarget[] = [
    { kind: "chat-agents", circleId: "c" },
    { kind: "chat-agents", circleId: "c", panel: "channels", channelId: agentChannel },
    { kind: "chat-agents", circleId: "c", agentId: "new" },
    { kind: "chat-agents", circleId: "c", agentId: "new", panel: "behaviour" },
    { kind: "chat-agents", circleId: "c", agentId },
    ...agentSettingsPanels.map(panel => ({ kind: "chat-agents" as const, circleId: "c", agentId, panel, channelId: agentChannel }))
  ];
  for (const page of pages) {
    const target: ViewTarget = { kind: "management", page };
    const href = viewHref(target, "https://chat.test/?invite=secret&agent=secret&panel=secret&channel=secret");
    assert.deepEqual(readViewTarget(href), target);
    assert.ok(!href.includes("secret"));
  }
  assert.equal(viewHref({ kind: "management", page: pages[0]! }, "https://chat.test/"), "https://chat.test/?view=settings&section=chat-agents&circle=c");
});

test("invalid agent destinations are rejected in links and stored history", () => {
  for (const fields of [
    { agentId: "bad" }, { agentId: "" }, { agentId, panel: "secret" }, { panel: "skills" },
    { agentId: "new", panel: "overview" }, { agentId: "new", panel: "channels" },
    { agentId, channelId: "bad" }, { panel: "channels", channelId: "" }, { panel: "" }
  ]) {
    const params = new URLSearchParams({ view: "settings", section: "chat-agents", circle: "c" });
    for (const [key, value] of Object.entries(fields)) params.set(key === "agentId" ? "agent" : key === "channelId" ? "channel" : key, value);
    assert.equal(readViewTarget(`https://chat.test/?${params}`), null, params.toString());
    const f = fixture(); f.history.bindOwner("alice");
    f.history.navigate({ kind: "management", page: { kind: "chat-agents", circleId: "c", ...fields } } as ViewTarget);
    assert.equal(f.history.getSnapshot().target, null);
  }
});

test("agent settings parents preserve only the relevant resource context", () => {
  const list: ManagementTarget = { kind: "chat-agents", circleId: "c" };
  const overview: ManagementTarget = { ...list, agentId, panel: "overview", channelId: agentChannel };
  for (const panel of agentSettingsPanels.filter(panel => panel !== "overview")) {
    assert.deepEqual(managementParent({ ...overview, panel }, { channels: [] }), overview);
  }
  assert.deepEqual(managementParent(overview, { channels: [] }), { ...list, channelId: agentChannel });
  assert.deepEqual(managementParent({ ...list, agentId }, { channels: [] }), list);
  assert.deepEqual(managementParent({ ...list, agentId: "new", panel: "behaviour" }, { channels: [] }), list);
  assert.deepEqual(managementParent({ ...list, panel: "channels", channelId: agentChannel }, { channels: [] }), { ...list, channelId: agentChannel });
  assert.deepEqual(managementParent(list, { channels: [] }), { kind: "circle", circleId: "c" });
});

test("channel sections survive links and history without accepting unknown panels", () => {
  const f = fixture(); f.history.bindOwner("alice");
  for (const channelSection of ["about", "members", "agents", "notifications"] as const) {
    const target: ViewTarget = { kind: "management", page: { kind: "community", destination: { kind: "channel", channelId: agentChannel, channelSection } } };
    const href = viewHref(target, "https://chat.test/?invite=secret");
    assert.deepEqual(readViewTarget(href), target);
    assert.ok(!href.includes("secret"));
    f.history.navigate(target, "replace");
    assert.deepEqual(f.history.getSnapshot().target, target);
  }
  assert.equal(readViewTarget(`https://chat.test/?view=settings&section=community&task=channel&channel=${agentChannel}&panel=bad`), null);
});

test("channel thread overview round-trips through URL and account-scoped history", () => {
  const target: ViewTarget = { kind: "management", page: { kind: "community", destination: { kind: "channel-threads", channelId: agentChannel } } };
  const href = viewHref(target, "https://chat.test/?invite=secret&participant=alice&code=secret");
  assert.equal(href, `https://chat.test/?view=settings&section=community&task=channel-threads&channel=${agentChannel}`);
  assert.deepEqual(readViewTarget(href), target);
  const f = fixture(); f.history.bindOwner("alice"); f.history.navigate(channel("other")); f.history.navigate(target);
  assert.deepEqual(f.history.getSnapshot().target, target);
  f.history.back(); assert.deepEqual(f.history.restore(f.port.history.state), channel("other"));
  f.port.history.forward(); assert.deepEqual(f.history.restore(f.port.history.state), target);
  for (const bad of ["", "\u0001", "x".repeat(129)]) {
    const params = new URLSearchParams({ view: "settings", section: "community", task: "channel-threads", channel: bad });
    assert.equal(readViewTarget(`https://chat.test/?${params}`), null);
  }
});

test("destinations round-trip without leaking invitation or identity parameters into shared URLs", () => {
  const targets: ViewTarget[] = [channel("a"), { kind: "conversation", channelId: "a", messageId: "reply", sequence: 12, threadId: "root" },
    { kind: "directory", scope: "direct" }, { kind: "inbox", inbox: "mentions" },
    { kind: "management", page: { kind: "chat-agents", circleId: "c" } },
    { kind: "management", page: { kind: "community", destination: { kind: "create-channel", circleId: null } } },
    { kind: "management", page: { kind: "advanced", feature: "agent", channelId: "a" } }];
  for (const target of targets) {
    const href = viewHref(target, "https://chat.test/?invite=secret&participant=alice&state=oidc&code=credential&thread=stale");
    assert.deepEqual(readViewTarget(href), target);
    for (const secret of ["secret", "alice", "credential", "oidc", "stale"]) assert.ok(!href.includes(secret));
  }
});

test("two visits to the same conversation restore distinct anchors and retain unrelated history state", () => {
  const f = fixture(); f.history.bindOwner("alice"); f.history.navigate(channel("a"));
  f.history.capture(anchor("a-10", 10));
  f.history.navigate({ kind: "conversation", channelId: "a", messageId: "a-20", sequence: 20 });
  f.history.capture(anchor("a-20", 20));
  assert.equal(f.entries.length, 2);
  f.history.back();
  // A late scroll publication cannot overwrite the entry the browser has moved to.
  f.history.capture(anchor("wrong", 99));
  assert.deepEqual(f.history.restore(f.port.history.state), channel("a"));
  assert.equal(f.history.getSnapshot().positions[0]?.position.anchorId, "a-10");
  assert.equal(f.history.getSnapshot().canForward, true);
  f.history.forward(); f.history.restore(f.port.history.state);
  assert.equal(f.history.getSnapshot().positions[0]?.position.anchorId, "a-20");
  assert.equal((f.port.history.state as {otherFeature:string}).otherFeature, "keep");
});

test("refresh publications do not grow history; a new branch clears forward traversal", () => {
  const f = fixture(); f.history.bindOwner("alice"); f.history.navigate(channel("a"));
  for (let i = 0; i < 5; i++) f.history.navigate(channel("a"));
  assert.equal(f.entries.length, 1);
  f.history.navigate(channel("b")); f.history.back(); f.history.restore(f.port.history.state);
  f.history.navigate({ kind: "directory", scope: "circles" });
  assert.equal(f.history.getSnapshot().canForward, false);
  assert.equal(f.entries.length, 2);
});

test("recent views are bounded, structural and account-isolated; foreign history is rejected", () => {
  const disk = storage(), f = fixture(disk); f.history.bindOwner("alice");
  for (let i = 0; i < 45; i++) f.history.navigate(channel(`c-${i}`));
  assert.equal(f.history.getSnapshot().recent.length, 30);
  const old = f.port.history.state;
  f.history.bindOwner("bob");
  assert.equal(f.history.getSnapshot().recent.length, 0);
  assert.equal(f.history.restore(old), null);
  f.history.bindOwner("alice");
  assert.equal(f.history.getSnapshot().recent.length, 30);
  assert.equal(f.history.getSnapshot().recent[0]?.target.kind, "conversation");
});

test("denied storage/history leaves navigation usable without unsafe traversal controls", () => {
  const disk = storage(); disk.getItem = () => { throw new Error("denied"); }; disk.setItem = () => { throw new Error("denied"); };
  const f = fixture(disk); f.port.history.pushState = () => { throw new Error("denied"); };
  f.history.bindOwner("alice"); f.history.navigate(channel("a")); f.history.navigate(channel("b"));
  assert.deepEqual(f.history.getSnapshot().target, channel("b"));
  assert.equal(f.history.getSnapshot().canBack, false);
});

test("drafts remain account-scoped and old unowned drafts require explicit recovery", () => {
  const disk = storage(); disk.setItem("sproyt.channel-draft.v1.a", "old unowned draft");
  const nav = new NavigationController(disk, { href: "https://chat.test/" }, { accountScoped: true });
  assert.equal(nav.restoreChannelDraft("a"), "");
  nav.bindOwner("alice"); assert.equal(nav.restoreChannelDraft("a"), ""); assert.equal(nav.hasLegacyDraft("a"), true);
  nav.recoverLegacyDraft("a"); nav.persistThreadDraft("a", "root", "alice thread");
  nav.bindOwner("bob"); assert.equal(nav.restoreChannelDraft("a"), ""); assert.equal(nav.restoreThreadDraft("a", "root"), "");
  nav.persistChannelDraft("a", "bob draft"); nav.bindOwner("alice");
  assert.equal(nav.restoreChannelDraft("a"), "old unowned draft"); assert.equal(nav.restoreThreadDraft("a", "root"), "alice thread");
});
test("getting started and invitation acceptance have canonical destinations", () => {
  for (const href of ["/?view=settings&section=welcome", "/?view=settings&section=community&task=accept-invitation"]) {
    const target = readViewTarget(`https://sproyt.test${href}`);
    assert.ok(target);
    assert.equal(new URL(viewHref(target, "https://sproyt.test/")).search, new URL(href, "https://sproyt.test/").search);
  }
});
