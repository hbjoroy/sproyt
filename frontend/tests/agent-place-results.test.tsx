import assert from "node:assert/strict";
import test from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { AgentPlaceResults, safeGoogleMapsSource, safeUntappdSearch } from "../src/ui/react/agent-place-results";

test("place source links accept only trustworthy Google Maps and Untappd search URLs", () => {
  assert.equal(safeGoogleMapsSource("https://www.google.com/maps/place/Beer"), "https://www.google.com/maps/place/Beer");
  assert.equal(safeGoogleMapsSource("https://maps.google.com/?cid=123"), "https://maps.google.com/?cid=123");
  assert.equal(safeGoogleMapsSource("https://maps.app.goo.gl/abc"), "https://maps.app.goo.gl/abc");
  assert.equal(safeGoogleMapsSource("https://www.google.com/search?q=beer+malmo"), "https://www.google.com/search?q=beer+malmo");
  for (const value of ["javascript:alert(1)", "http://maps.google.com/", "https://google.com.evil.test/maps", "https://www.google.com.evil.test/search?q=bar", "https://google.com@evil.test/search?q=bar", "https://www.google.com/url?q=https://evil.test", "https://goo.gl/not-maps"]) {
    assert.equal(safeGoogleMapsSource(value), null);
  }
  assert.equal(safeUntappdSearch("https://untappd.com/search?q=beer"), "https://untappd.com/search?q=beer");
  for (const value of ["http://untappd.com/search?q=beer", "https://untappd.com/v/unknown", "https://untappd.com/search", "https://untappd.com.evil.test/search?q=beer"]) {
    assert.equal(safeUntappdSearch(value), null);
  }
});

test("place summary and route each retain their immediate Google Maps sources", () => {
  const html = renderToStaticMarkup(<AgentPlaceResults state={{ kind: "ready", summary: "Exact summary [0]\nLine 2",
    sources: [{ title: "Beer place", url: "https://www.google.com/maps/place/Beer" }], selection: "Beer place",
    walking: { distanceText: "700 m", durationText: "9 min", warning: "Rutene kan endrast.",
      sources: [{ title: "Walking route", url: "https://www.google.com/maps/dir/Start/Beer" }] },
    untappdSearchUrl: "https://untappd.com/search?q=Beer" }} />);
  assert.ok(html.includes("Exact summary [0]\nLine 2"));
  assert.ok(html.indexOf("Exact summary") < html.indexOf("Beer place · Google Maps"));
  assert.ok(html.indexOf("Beer place · Google Maps") < html.indexOf("Terningval:"));
  assert.ok(html.indexOf("700 m") < html.indexOf("Walking route · Google Maps"));
  assert.ok(html.indexOf("Walking route · Google Maps") < html.indexOf("Søk etter staden i Untappd"));
  assert.ok(html.includes("noopener noreferrer"));
});

test("invalid source fails closed and never renders the provider result or unsafe URL", () => {
  const html = renderToStaticMarkup(<AgentPlaceResults state={{ kind: "ready", summary: "must not leak",
    sources: [{ title: "place", url: "https://evil.test/" }] }} />);
  assert.ok(html.includes("kjelde manglar eller er ugyldig"));
  assert.ok(!html.includes("must not leak"));
  assert.ok(!html.includes("https://evil.test"));
});

test("loading, empty and retryable error states are explicit", () => {
  assert.ok(renderToStaticMarkup(<AgentPlaceResults state={{ kind: "loading" }} />).includes("Leitar etter stader"));
  assert.ok(renderToStaticMarkup(<AgentPlaceResults state={{ kind: "empty", onRetry: () => {} }} />).includes("Søk på nytt"));
  const html = renderToStaticMarkup(<AgentPlaceResults state={{ kind: "error", message: "Feil ved søk", onRetry: () => {} }} />);
  assert.ok(html.includes("Feil ved søk"));
  assert.ok(html.includes("Prøv igjen"));
});
