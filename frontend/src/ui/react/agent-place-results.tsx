import { Button, Status } from "@sproyt/ui/react";

/** Provider text and source URLs are deliberately passed through without storage. */
export type AgentPlaceSource = Readonly<{ title: string; url: string }>;
export type AgentPlaceWalking = Readonly<{
  /** A separately obtained walking route, not a straight-line estimate. */
  distanceText: string;
  durationText: string;
  warning: string;
  sources: readonly AgentPlaceSource[];
}>;
export type AgentPlaceResultState =
  | Readonly<{ kind: "loading" }>
  | Readonly<{ kind: "error"; message: string; onRetry?: () => void }>
  | Readonly<{ kind: "empty"; onRetry?: () => void }>
  | Readonly<{
      kind: "ready";
      /** The original provider summary. Do not rewrite, split or add content to it. */
      summary: string;
      sources: readonly AgentPlaceSource[];
      /** Optional draw information, separately displayed from the provider output. */
      selection?: string;
      walking?: AgentPlaceWalking;
      /** An explicitly labelled search, never a claim that this venue is verified. */
      untappdSearchUrl?: string;
    }>;

const css = `
.sp-theme .sp-agent-place-results { min-width: 0; max-width: 42rem; color: var(--sp-text); }
.sp-theme .sp-agent-place-results h3 { font-size: 1rem; margin: 0 0 .45rem; }
.sp-theme .sp-agent-place-results p { margin: .35rem 0; }
.sp-theme .sp-agent-place-results-summary { white-space: pre-wrap; overflow-wrap: anywhere; }
.sp-theme .sp-agent-place-results-sources { list-style: none; margin: .45rem 0 .7rem; padding: 0; }
.sp-theme .sp-agent-place-results-sources li { margin: .15rem 0; }
.sp-theme .sp-agent-place-results a { color: inherit; text-underline-offset: .2em; overflow-wrap: anywhere; }
.sp-theme .sp-agent-place-results a:focus-visible { outline: 2px solid currentColor; outline-offset: 3px; }
.sp-theme .sp-agent-place-results-extra { border-top: 1px solid var(--sp-line); margin-top: .65rem; padding-top: .45rem; }
.sp-theme .sp-agent-place-results-meta { color: var(--sp-muted); font-size: .875rem; }
@media (pointer: coarse) { .sp-theme .sp-agent-place-results a { display: inline-flex; align-items: center; min-height: 44px; } }
`;

/** Only real provider URLs may be followed; never render a model-supplied scheme. */
export function safeGoogleMapsSource(value: string): string | null {
  try {
    const url = new URL(value);
    if (url.protocol !== "https:" || url.username || url.password || (url.port && url.port !== "443")) return null;
    const host = url.hostname.toLowerCase();
    if (host === "maps.app.goo.gl" || (host === "goo.gl" && url.pathname.startsWith("/maps/"))) return url.href;
    if (/^(?:www\.)?google\.(?:com|dk|se|no)$/u.test(host)
        && (url.pathname === "/maps" || url.pathname.startsWith("/maps/") || url.pathname === "/search")) return url.href;
    if (/^maps\.google\.(?:com|dk|se|no)$/u.test(host)) return url.href;
  } catch { /* The untrusted URL must not reach the DOM. */ }
  return null;
}

export function safeUntappdSearch(value: string): string | null {
  try {
    const url = new URL(value);
    if (url.protocol !== "https:" || url.username || url.password || (url.port && url.port !== "443")) return null;
    if ((url.hostname === "untappd.com" || url.hostname === "www.untappd.com")
        && (url.pathname === "/search" || url.pathname === "/search/") && url.searchParams.get("q")?.trim()) return url.href;
  } catch { /* Do not render malformed or unsupported search URLs. */ }
  return null;
}

function Sources({ sources }: { readonly sources: readonly AgentPlaceSource[] }) {
  return <ul className="sp-agent-place-results-sources" aria-label="Kjelder frå Google Maps">
    {sources.map((source, index) => <li key={`${source.url}-${index}`}>
      <a href={safeGoogleMapsSource(source.url)!} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer">
        {source.title} · Google Maps ↗
      </a>
    </li>)}
  </ul>;
}

/** Presentation only: the host owns requests, cancellation, consent and lifetime. */
export function AgentPlaceResults({ state, placeLinks = [] }: { readonly state: AgentPlaceResultState;
  readonly placeLinks?: readonly Readonly<{ title: string; placeUrl: string; walking?: "loading" | "error" | AgentPlaceWalking; onWalk?: () => void }>[] }) {
  const invalidSources = state.kind === "ready" && (!state.summary.trim() || !state.sources.length
    || state.sources.some(source => !source.title.trim() || !safeGoogleMapsSource(source.url))
    || placeLinks.some(link => !link.title.trim() || !safeGoogleMapsSource(link.placeUrl)
      || (typeof link.walking === "object" && (!link.walking.sources.length
        || link.walking.sources.some(item => !item.title.trim() || !safeGoogleMapsSource(item.url)))))
    || Boolean(state.walking && (!state.walking.sources.length
      || state.walking.sources.some(source => !source.title.trim() || !safeGoogleMapsSource(source.url)))));
  const untappdUrl = state.kind === "ready" && state.untappdSearchUrl ? safeUntappdSearch(state.untappdSearchUrl) : null;
  return <section className="sp-agent-place-results" aria-label="Stadsøk">
    <style>{css}</style>
    <h3>Stadsøk</h3>
    {state.kind === "loading" && <Status>Leitar etter stader …</Status>}
    {state.kind === "error" && <Status tone="error">{state.message} {state.onRetry && <Button variant="quiet" onClick={state.onRetry}>Prøv igjen</Button>}</Status>}
    {state.kind === "empty" && <><p>Fann ingen stader som passa søket.</p>{state.onRetry && <Button variant="quiet" onClick={state.onRetry}>Søk på nytt</Button>}</>}
    {state.kind === "ready" && invalidSources && <Status tone="error">Kan ikkje vise resultatet fordi ei Google Maps-kjelde manglar eller er ugyldig.</Status>}
    {state.kind === "ready" && !invalidSources && <>
      <p className="sp-agent-place-results-summary">{state.summary}</p>
      <Sources sources={state.sources} />
      {placeLinks.length > 0 && <ul className="sp-agent-place-results-sources" aria-label="Opne stader i Google Maps">
        {placeLinks.map((place, index) => <li key={`${place.placeUrl}-${index}`}>
          <a href={safeGoogleMapsSource(place.placeUrl)!} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer">Opne {place.title} i Google Maps ↗</a>
          {place.onWalk && <Button variant="quiet" onClick={place.onWalk}>Vis gangavstand</Button>}
          {place.walking === "loading" && <Status>Hentar gangrute …</Status>}
          {place.walking === "error" && <Status tone="error">Kunne ikkje hente gangruta. Vent litt og prøv igjen, eller hent stadssøket på nytt.</Status>}
          {typeof place.walking === "object" && <div className="sp-agent-place-results-extra">
            <p><strong>Til fots:</strong> {place.walking.distanceText} · {place.walking.durationText}</p>
            <p className="sp-agent-place-results-meta">{place.walking.warning}</p><Sources sources={place.walking.sources} />
          </div>}
        </li>)}
      </ul>}
      {(state.selection || state.walking || untappdUrl) && <div className="sp-agent-place-results-extra">
        {state.selection && <p><strong>Terningval:</strong> {state.selection}</p>}
        {state.walking && <>
          <p><strong>Til fots:</strong> {state.walking.distanceText} · {state.walking.durationText}</p>
          <p className="sp-agent-place-results-meta">{state.walking.warning}</p>
          <Sources sources={state.walking.sources} />
        </>}
        {untappdUrl && <p><a href={untappdUrl} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer">Søk etter staden i Untappd ↗</a></p>}
      </div>}
      <p className="sp-agent-place-results-meta">Resultatet er mellombels. Opningstider og tilbod kan endre seg.</p>
    </>}
  </section>;
}
