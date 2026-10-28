import { Button } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { AgentPlaceLookup, AgentPlaceLookupApi, AgentPlacePreview } from "../../agent-place-lookup";
import { AgentPlaceResults, type AgentPlaceWalking } from "./agent-place-results";

type CardState = "closed" | "preview-loading" | "preview-error" | "loading" | "error" | { kind: "confirm"; preview: AgentPlacePreview } | AgentPlaceLookup;

/** One explicitly requested, in-memory result. No provider data enters chat history. */
export function AgentPlaceCard({ api, owner, channelId, messageId, admitted }: {
  readonly api: AgentPlaceLookupApi; readonly owner: string; readonly channelId: string; readonly messageId: string;
  readonly admitted: () => boolean;
}) {
  const [state, setState] = useState<CardState>("closed");
  const [routes, setRoutes] = useState<Record<number, "loading" | "error" | AgentPlaceWalking>>({});
  const live = useRef(true);
  const generation = useRef(0);
  const request = useRef<AbortController | null>(null);
  const routeRequests = useRef(new Map<number, AbortController>());
  const expiry = useRef<ReturnType<typeof setTimeout> | null>(null);
  const currentAdmission = useRef(admitted); currentAdmission.current = admitted;
  useEffect(() => {
    live.current = true;
    return () => { live.current = false; generation.current++; request.current?.abort(); routeRequests.current.forEach(controller => controller.abort());
      if (expiry.current !== null) clearTimeout(expiry.current); };
  }, []);
  function close() {
    generation.current++; request.current?.abort(); request.current = null;
    routeRequests.current.forEach(controller => controller.abort()); routeRequests.current.clear(); setRoutes({});
    if (expiry.current !== null) clearTimeout(expiry.current);
    expiry.current = null; setState("closed");
  }
  function search() {
    if (!live.current || !currentAdmission.current() || typeof state !== "object" || !("kind" in state) || state.kind !== "confirm") return;
    const binding = state.preview.binding;
    const current = ++generation.current;
    request.current?.abort();
    if (expiry.current !== null) clearTimeout(expiry.current);
    expiry.current = null;
    const controller = new AbortController(); request.current = controller;
    const allowed = () => live.current && generation.current === current && !controller.signal.aborted && currentAdmission.current();
    setState("loading");
    void api.lookup(owner, channelId, messageId, true, binding, allowed, controller.signal).then(result => {
      if (!allowed()) return;
      setState(result);
      // The card is deliberately transient and can be fetched again on demand.
      expiry.current = setTimeout(() => { if (live.current && generation.current === current) close(); }, 5 * 60_000);
    }).catch(() => { if (allowed()) setState("error"); });
  }
  function walk(index: number, token: string) {
    if (!live.current || !currentAdmission.current()) return;
    const current = generation.current;
    routeRequests.current.get(index)?.abort();
    const controller = new AbortController(); routeRequests.current.set(index, controller);
    const allowed = () => live.current && generation.current === current && !controller.signal.aborted && currentAdmission.current();
    setRoutes(previous => ({ ...previous, [index]: "loading" }));
    void api.route(owner, channelId, messageId, token, allowed, controller.signal).then(result => {
      if (allowed()) setRoutes(previous => ({ ...previous, [index]: result }));
    }).catch(() => { if (allowed()) setRoutes(previous => ({ ...previous, [index]: "error" })); });
  }
  function loadPreview() {
    if (!live.current || !currentAdmission.current()) return;
    const current = ++generation.current;
    request.current?.abort();
    const controller = new AbortController(); request.current = controller;
    const allowed = () => live.current && generation.current === current && !controller.signal.aborted && currentAdmission.current();
    setState("preview-loading");
    void api.preview(owner, channelId, messageId, allowed, controller.signal).then(preview => {
      if (allowed()) setState({ kind: "confirm", preview });
    }).catch(() => { if (allowed()) setState("preview-error"); });
  }
  return <div className="sp-agent-place-card">
    {state === "closed" && <Button variant="quiet" onClick={loadPreview}>Vis stadssøk</Button>}
    {state === "preview-loading" && <div><p>Hentar søkeforespurnaden …</p><Button variant="quiet" onClick={close}>Avbryt</Button></div>}
    {state === "preview-error" && <div><p>Kunne ikkje hente søkeforespurnaden.</p><Button variant="quiet" onClick={loadPreview}>Prøv igjen</Button> <Button variant="quiet" onClick={close}>Lukk</Button></div>}
    {typeof state === "object" && "kind" in state && state.kind === "confirm" && <div>
      <p><strong>Søk:</strong> {state.preview.query}</p>
      {state.preview.namedOrigin && <p><strong>Utgangspunkt:</strong> {state.preview.namedOrigin}</p>}
      <p>Søkeforespurnaden blir sendt til Google Maps.{state.preview.usesSharedLocation ? " Den delte posisjonen blir òg send til Google." : ""} Resultatet blir berre vist mellombels.</p>
      <Button onClick={search}>Søk med Google</Button> <Button variant="quiet" onClick={close}>Avbryt</Button>
    </div>}
    {state === "loading" && <><AgentPlaceResults state={{ kind: "loading" }} /><Button variant="quiet" onClick={close}>Avbryt</Button></>}
    {state === "error" && <><AgentPlaceResults state={{ kind: "error", message: "Kunne ikkje hente stadene. Kontroller posisjonsdelinga og prøv igjen.", onRetry: loadPreview }} />
      <Button variant="quiet" onClick={close}>Lukk</Button></>}
    {typeof state === "object" && "state" in state && <><AgentPlaceResults state={state.state} placeLinks={state.places.map((place, index) => ({ ...place,
      walking: routes[index], onWalk: place.routeToken && routes[index] !== "loading" ? () => walk(index, place.routeToken!) : undefined }))} />
      <Button variant="quiet" onClick={close}>Lukk stadssøk</Button></>}
  </div>;
}
