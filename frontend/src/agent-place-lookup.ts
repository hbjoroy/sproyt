import { HttpClient } from "./api";
import { isRecord } from "./types";
import { safeGoogleMapsSource, type AgentPlaceResultState, type AgentPlaceSource, type AgentPlaceWalking } from "./ui/react/agent-place-results";

export type AgentPlaceLookup = Readonly<{
  state: Extract<AgentPlaceResultState, { kind: "ready" | "empty" }>;
  /** Separate place links. The provider's attribution links remain in state.sources. */
  places: readonly Readonly<{ title: string; attributionUrl: string; placeUrl: string; routeToken?: string }>[];
  /** A route was included; do not invent or display a route until it is mapped. */
  walkingAvailable: boolean;
}>;
export type AgentPlacePreview = Readonly<{ query: string; originMode: "none" | "named" | "shared"; namedOrigin: string | null; usesSharedLocation: boolean; binding: string }>;

function invalid(): Error { return new Error("Ugyldig svar frå stadoppslaget."); }
function admittedNow(admitted: () => boolean, signal?: AbortSignal): void {
  if (signal?.aborted) throw new DOMException("Oppslaget vart avbrote.", "AbortError");
  if (!admitted()) throw new Error("Innlogginga eller samtalen vart endra. Prøv igjen.");
}
function validId(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 256 && value.trim() === value && !/[\u0000-\u001f\u007f]/u.test(value);
}
function source(value: unknown): AgentPlaceSource {
  if (!isRecord(value) || typeof value.title !== "string" || !value.title.trim() || value.title.length > 512
      || typeof value.url !== "string" || value.url.length > 4096 || !safeGoogleMapsSource(value.url)) throw invalid();
  return { title: value.title, url: value.url };
}

/** Decode the provider structure without changing or caching its summary or sources. */
export function decodeAgentPlaceLookup(value: unknown): AgentPlaceLookup {
  if (!isRecord(value) || !isRecord(value.search) || typeof value.search.summary !== "string"
      || value.search.summary.length > 262_144 || !Array.isArray(value.search.places) || value.search.places.length > 50
      || !(value.walking === null || isRecord(value.walking))) throw invalid();
  const summary = value.search.summary;
  if (!Array.isArray(value.route_tokens) || value.route_tokens.length > value.search.places.length) throw invalid();
  const tokens = new Map<number, string>();
  for (const token of value.route_tokens) {
    if (!isRecord(token) || !Number.isSafeInteger(token.index) || (token.index as number) < 0 || (token.index as number) >= value.search.places.length
        || typeof token.token !== "string" || !token.token || token.token.length > 8192 || tokens.has(token.index as number)) throw invalid();
    tokens.set(token.index as number, token.token);
  }
  const places = value.search.places.map((place, index) => {
    if (!isRecord(place) || !validId(place.id) || !isRecord(place.googleMapsLinks)
        || typeof place.googleMapsLinks.placeUrl !== "string" || place.googleMapsLinks.placeUrl.length > 4096
        || !safeGoogleMapsSource(place.googleMapsLinks.placeUrl)) throw invalid();
    const attribution = source(place.attribution);
    const routeToken = tokens.get(index);
    return { title: attribution.title, attributionUrl: attribution.url, placeUrl: place.googleMapsLinks.placeUrl, ...(routeToken ? { routeToken } : {}) };
  });
  if (!places.length) {
    if (summary.trim() || value.walking !== null) throw invalid();
    return { state: { kind: "empty" }, places: [], walkingAvailable: false };
  }
  if (!summary.trim()) throw invalid();
  let walkingAvailable = false;
  if (value.walking !== null) {
    if (!Array.isArray(value.walking.routes) || value.walking.routes.length > 3) throw invalid();
    walkingAvailable = value.walking.routes.length > 0;
    for (const route of value.walking.routes) {
      if (!isRecord(route) || !Number.isSafeInteger(route.distanceMeters) || (route.distanceMeters as number) < 0
          || typeof route.duration !== "string" || !/^\d+(?:\.\d{1,9})?s$/u.test(route.duration)) throw invalid();
      source(route.attribution);
    }
  }
  return { state: { kind: "ready", summary, sources: places.map(place => ({ title: place.title, url: place.attributionUrl })) },
    places, walkingAvailable };
}

export class AgentPlaceLookupApi {
  constructor(private readonly http: HttpClient) {}

  preview(owner: string, channelId: string, messageId: string, admitted: () => boolean, signal?: AbortSignal): Promise<AgentPlacePreview> {
    if (!validId(owner) || !validId(channelId) || !validId(messageId)) throw invalid();
    admittedNow(admitted, signal);
    return this.http.json<AgentPlacePreview>(`/api/v1/channels/${encodeURIComponent(channelId)}/messages/${encodeURIComponent(messageId)}/place-lookup`, value => {
      admittedNow(admitted, signal);
      if (!isRecord(value) || typeof value.query !== "string" || !value.query.trim() || value.query.length > 2048
          || !(value.origin_mode === "none" || value.origin_mode === "named" || value.origin_mode === "shared")
          || !(value.named_origin === null || (typeof value.named_origin === "string" && value.named_origin.trim() && value.named_origin.length <= 1024))
          || typeof value.preview_binding !== "string" || !/^[a-f0-9]{64}$/u.test(value.preview_binding)
          || typeof value.uses_shared_location !== "boolean" || value.uses_shared_location !== (value.origin_mode === "shared")
          || (value.origin_mode === "named") !== (value.named_origin !== null)) throw invalid();
      return { query: value.query, originMode: value.origin_mode, namedOrigin: value.named_origin as string | null, usesSharedLocation: value.uses_shared_location, binding: value.preview_binding };
    }, { method: "GET", headers: { "x-sproyt-account": owner }, signal }, admitted)
      .then(result => { admittedNow(admitted, signal); return result; });
  }

  list(owner: string, channelId: string, messageIds: readonly string[], admitted: () => boolean, signal?: AbortSignal): Promise<readonly string[]> {
    if (!validId(owner) || !validId(channelId) || !messageIds.length || messageIds.length > 30
        || messageIds.some(id => !validId(id)) || new Set(messageIds).size !== messageIds.length) throw invalid();
    admittedNow(admitted, signal);
    const requested = new Set(messageIds);
    return this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/place-lookups`, value => {
      admittedNow(admitted, signal);
      if (!isRecord(value) || !Array.isArray(value.message_ids) || value.message_ids.length > messageIds.length) throw invalid();
      const result: string[] = [];
      for (const id of value.message_ids) {
        if (!validId(id) || !requested.has(id) || result.includes(id)) throw invalid();
        result.push(id);
      }
      return result;
    }, { method: "POST", headers: { "content-type": "application/json", "x-sproyt-account": owner },
      body: JSON.stringify({ message_ids: messageIds }), signal }, admitted).then(result => { admittedNow(admitted, signal); return result; });
  }

  lookup(owner: string, channelId: string, messageId: string, allowSharedLocation: boolean, previewBinding: string, admitted: () => boolean, signal?: AbortSignal): Promise<AgentPlaceLookup> {
    if (!validId(owner) || !validId(channelId) || !validId(messageId) || typeof allowSharedLocation !== "boolean" || !/^[a-f0-9]{64}$/u.test(previewBinding)) throw invalid();
    admittedNow(admitted, signal);
    return this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/messages/${encodeURIComponent(messageId)}/place-lookup`, value => {
      admittedNow(admitted, signal);
      return decodeAgentPlaceLookup(value);
    }, { method: "POST", headers: { "content-type": "application/json", "x-sproyt-account": owner },
      body: JSON.stringify({ allow_shared_location: allowSharedLocation, preview_binding: previewBinding }), signal }, admitted)
      .then(result => { admittedNow(admitted, signal); return result; });
  }

  route(owner: string, channelId: string, messageId: string, token: string, admitted: () => boolean, signal?: AbortSignal): Promise<AgentPlaceWalking> {
    if (!validId(owner) || !validId(channelId) || !validId(messageId) || !token || token.length > 8192) throw invalid();
    admittedNow(admitted, signal);
    return this.http.json<AgentPlaceWalking>(`/api/v1/channels/${encodeURIComponent(channelId)}/messages/${encodeURIComponent(messageId)}/place-route`, value => {
      admittedNow(admitted, signal);
      if (!isRecord(value) || typeof value.warning !== "string" || !value.warning.trim() || value.warning.length > 1024
          || !isRecord(value.route) || !Array.isArray(value.route.routes) || !value.route.routes.length || value.route.routes.length > 3) throw invalid();
      for (const candidate of value.route.routes) {
        if (!isRecord(candidate) || !Number.isSafeInteger(candidate.distanceMeters) || (candidate.distanceMeters as number) < 0
            || typeof candidate.duration !== "string" || !/^\d+(?:\.\d{1,9})?s$/u.test(candidate.duration)) throw invalid();
        source(candidate.attribution);
      }
      const route = value.route.routes[0];
      if (!isRecord(route) || !Number.isSafeInteger(route.distanceMeters) || (route.distanceMeters as number) < 0
          || typeof route.duration !== "string" || !/^\d+(?:\.\d{1,9})?s$/u.test(route.duration)) throw invalid();
      const durationSeconds = Number(route.duration.slice(0, -1));
      if (!Number.isFinite(durationSeconds) || durationSeconds > 31536000) throw invalid();
      const distance = route.distanceMeters as number;
      return { distanceText: distance < 1000 ? `${distance} m` : `${(distance / 1000).toLocaleString("nb-NO", { maximumFractionDigits: 1 })} km`,
        durationText: `${Math.max(1, Math.ceil(durationSeconds / 60))} min`, warning: value.warning, sources: [source(route.attribution)] };
    }, { method: "POST", headers: { "content-type": "application/json", "x-sproyt-account": owner },
      body: JSON.stringify({ token, allow_shared_location: true }), signal }, admitted)
      .then(result => { admittedNow(admitted, signal); return result; });
  }
}
