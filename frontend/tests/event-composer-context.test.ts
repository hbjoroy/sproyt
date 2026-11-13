import assert from "node:assert/strict";
import test from "node:test";
import { messageWithEventContext } from "../src/ui/react/event-composer-context";
import type { ChannelEvent } from "../src/channel-events";
import corpus from "../../docs/fixtures/event-references.json" with { type: "json" };

const event = { id: "event-a", channel_id: "channel", code: "gig", aliases: ["old-gig"], title: "Konsert" } as unknown as ChannelEvent;
test("event context preserves drafts, aliases and several explicit event references", () => {
  assert.equal(messageWithEventContext("Teksten min", event), "Teksten min\n\n#arr:gig");
  assert.equal(messageWithEventContext("", event), "#arr:gig");
  for (const source of ["Hei #arr:gig", "Hei #ARR:OLD-GIG", "#arr:party og #arr:gig", "(#arr:old-gig)"]) assert.equal(messageWithEventContext(source, event), source);
  assert.equal(messageWithEventContext("#arr:party", event), "#arr:party\n\n#arr:gig");
  const firstAttempt = messageWithEventContext("Ikkje send dobbelt", event);
  assert.equal(messageWithEventContext(firstAttempt, event), firstAttempt);
});
test("event context uses the same Markdown and reference rules as rendering and Rust", () => {
  for (const sample of corpus) {
    const prepared = messageWithEventContext(sample.source, event);
    assert.equal(prepared, sample.codes.includes("gig") ? sample.source : `${sample.source}${sample.source ? "\n\n" : ""}#arr:gig`, sample.name);
  }
});
test("event context cannot silently become an ignored fifty-first reference", () => {
  const fifty = Array.from({ length: 50 }, (_, index) => `#arr:event-${index}`).join(" ");
  assert.throws(() => messageWithEventContext(fifty, event), /50 arrangementsreferansar/u);
  assert.throws(() => messageWithEventContext(`${fifty} #arr:gig`, event), /50 arrangementsreferansar/u);
  const withinLimit = `#arr:gig ${Array.from({ length: 49 }, (_, index) => `#arr:event-${index}`).join(" ")}`;
  assert.equal(messageWithEventContext(withinLimit, event), withinLimit);
});
