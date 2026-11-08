import assert from "node:assert/strict";
import test from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { HttpClient } from "../src/api";
import { ChannelEventsApi, EventReferenceDiscovery, decodeChannelEvent, type ChannelEvent } from "../src/channel-events";
import { EventReferencesContext } from "../src/ui/react/event-references";
import { MarkdownContent } from "../src/ui/react/markdown-content";
import { ChannelGovernanceApi } from "../src/channel-governance";
import corpus from "../../docs/fixtures/event-references.json" with { type: "json" };

const channel = "00000000-0000-7000-8000-000000000001";
const event: ChannelEvent = {
  events_enabled: true,
  id: "00000000-0000-7000-8000-000000000002", channel_id: channel, creator_id: channel,
  title: "Konsert", code: "konsert", aliases: ["gammal"], starts_at: "2027-01-01T18:00:00Z", timezone: "Europe/Oslo",
  ends_at: null, location: null, description: "**Velkomen**", cancelled: false, revision: 1,
  updated_at: "2026-11-07T12:00:00Z", cover_media_id: null, my_rsvp: "unanswered", can_manage: true,
  needs_attention: false, images: [], images_next_after: null
};

function render(source: string) {
  return renderToStaticMarkup(<EventReferencesContext.Provider value={{ events: [event], href: item => `/?channel=${channel}&event=${item.id}`, open() {} }}>
    <MarkdownContent source={source} channelReferences={{ scopeKey: channel, find: slug => slug === "arr" ? { id: channel, slug, name: "Arr", is_member: true } : undefined,
      collect() {}, href: () => "/?channel=wrong", open: async () => undefined, join: async () => {} }} />
  </EventReferencesContext.Provider>);
}
test("event references retain UUID identity and old aliases, preserve rich Markdown, and never link the arr channel", () => {
  const html = render("**Hei** #arr:konsert og #arr:gammal og #arr:ukjent");
  assert.equal((html.match(/data-event-reference=/gu) ?? []).length, 2);
  assert.ok(html.includes(`<strong>Hei</strong>`));
  assert.ok(html.includes(`event=${event.id}`));
  assert.ok(!html.includes("channel=wrong"));
});
test("GFM still autolinks safe web URLs beside event references with the normal link protections", () => {
  const html = render("Sjå https://example.org/agenda og #arr:konsert.\n\n| Tid | Stad |\n| --- | --- |\n| 19:00 | Salen |");
  assert.match(html, /href="https:\/\/example\.org\/agenda"/u);
  assert.ok(html.includes('target="_blank"'));
  assert.ok(html.includes('rel="noopener noreferrer"'));
  assert.ok(html.includes('referrerPolicy="no-referrer"'));
  assert.ok(html.includes("<table>"));
  assert.ok(html.includes('data-event-reference="'));
  assert.ok(!html.includes('<img'));
});
test("event references skip code, escaped tokens, existing links, HTML and incomplete suffixes", () => {
  for (const source of ["`#arr:konsert`", "\\#arr:konsert", "[#arr:konsert](https://example.org)", "#arr:konsert_foo", "#arr:konsertø", "#arr:konsert-extra", "abc#arr:konsert"]) {
    assert.ok(!render(source).includes("data-event-reference="), source);
  }
  assert.equal((render("#ARR:KONSERT (#arr:konsert)").match(/data-event-reference=/gu) ?? []).length, 2);
});
test("the actual Markdown renderer agrees with the Rust reference parser corpus", () => {
  const codes = ["gig", "party", "last", "no", "yes", "gig-foo"];
  for (const sample of corpus) {
    const events = codes.map((code, index) => ({ ...event, id: `${event.id.slice(0, -2)}${String(index).padStart(2, "0")}`, code, aliases: [] }));
    const html = renderToStaticMarkup(<EventReferencesContext.Provider value={{ events, href: item => `/?event=${item.id}`, open() {} }}><MarkdownContent source={sample.source} /></EventReferencesContext.Provider>);
    const actual = [...new Set([...html.matchAll(/data-event-reference="([^"]+)"/gu)].map(match => events.find(event => event.id === match[1])!.code))];
    assert.deepEqual(actual, sample.codes, sample.name);
  }
});
test("event decoder refuses cross-channel data and untrusted media paths", () => {
  assert.deepEqual(decodeChannelEvent(event, channel), event);
  assert.throws(() => decodeChannelEvent({ ...event, channel_id: event.id }, channel));
  assert.throws(() => decodeChannelEvent({ ...event, cover_media_id: "../secret" }, channel));
  assert.throws(() => decodeChannelEvent({ ...event, my_rsvp: "maybe" }, channel));
  assert.equal(decodeChannelEvent({ ...event, events_enabled: false }, channel).events_enabled, false);
  assert.throws(() => decodeChannelEvent({ ...event, events_enabled: "false" }, channel));
});
test("availability governance keeps false typed and rejects a stale account write result", async () => {
  let owner = "owner", epoch = 1;
  let release!: (value: Response) => void;
  const api = new ChannelGovernanceApi(new HttpClient({ fetch: () => new Promise(resolve => { release = resolve; }) }), () => owner, () => epoch);
  const pending = api.request(channel, "/events-enabled", { enabled: false });
  owner = "other"; epoch++; release(Response.json({ hidden: false, events_enabled: false, members: [] }));
  await assert.rejects(pending, /Kontoen eller sambandet vart endra/u);
  const current = new ChannelGovernanceApi(new HttpClient({ fetch: async () => Response.json({ hidden: false, events_enabled: false, members: [] }) }), () => owner, () => epoch);
  assert.equal((await current.request(channel)).events_enabled, false);
});
test("event SDK bounds collection pages with account-bound admission and aborts stale account results", async () => {
  let owner = "owner"; let epoch = 1;
  const paths: string[] = [];
  const api = new ChannelEventsApi(new HttpClient({ fetch: async (input, options) => {
    paths.push(String(input)); assert.equal(new Headers(options?.headers).get("x-sproyt-account"), owner);
    return Response.json({ events: [event], next_after: paths.length === 1 ? event.id : null });
  } }), () => owner, () => epoch);
  const first = await api.list(owner, channel, new AbortController().signal);
  assert.equal(first.events.length, 1);
  assert.equal(paths.length, 1);
  await api.list(owner, channel, new AbortController().signal, first.next_after);
  assert.ok(paths[1]!.includes(`after=${event.id}`));
  let complete!: (response: Response) => void;
  const stale = new ChannelEventsApi(new HttpClient({ fetch: () => new Promise(resolve => { complete = resolve; }) }), () => owner, () => epoch);
  const pending = stale.detail(owner, channel, event.id);
  epoch++; owner = "other"; complete(Response.json(event));
  await assert.rejects(pending, /Kontoen vart endra/u);
});
test("responses stay paged with exact aggregate counts and reference discovery cannot publish after channel disposal", async () => {
  const requests: string[] = [];
  let complete!: (response: Response) => void;
  const api = new ChannelEventsApi(new HttpClient({ fetch: async input => {
    requests.push(String(input));
    if (String(input).includes("/responses")) return Response.json({ responses: [], next_after: "more-people", counts: { attending: 201, declined: 2, following: 3, unanswered: 42 } });
    return new Promise<Response>(resolve => { complete = resolve; });
  } }), () => "owner", () => 1);
  const page = await api.responses("owner", channel, event.id, new AbortController().signal);
  assert.equal(requests.length, 1); assert.equal(page.next_after, "more-people"); assert.equal(page.counts?.unanswered, 42);
  let published = 0;
  const discovery = new EventReferenceDiscovery(api, "owner", channel, () => published++);
  discovery.collect(new Set(["konsert"]));
  await new Promise(resolve => setTimeout(resolve, 0));
  discovery.dispose(); complete(Response.json({ events: [event] }));
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(published, 0); assert.deepEqual(discovery.events(), []);
});
