import { HttpClient } from "./api";
import { isRecord } from "./types";

export type GeoRollStatus = "geocoding" | "choose_origin" | "pending" | "leased" | "ready" | "published" | "failed" | "cancelled";
export type GeoRollView = Readonly<{ id: string; status: GeoRollStatus; choices: readonly Readonly<{ index: number; label: string }>[]; errorCode: string | null; resultMessageId: string | null; originText: string | null; canRetryGeocoding: boolean }>;
export type GeoRollPreview = Readonly<{ binding: string; originMode: "none" | "named" | "shared"; suggestedOrigin: string | null; radius: number; state: GeoRollView | null }>;
const statuses = new Set(["geocoding", "choose_origin", "pending", "leased", "ready", "published", "failed", "cancelled"]);
function invalid(): Error { return new Error("Ugyldig svar frå baroppslaget."); }
function validId(value: unknown): value is string { return typeof value === "string" && value.length > 0 && value.length <= 256 && value.trim() === value && !/[\u0000-\u001f\u007f]/u.test(value); }
function admittedNow(admitted: () => boolean, signal?: AbortSignal): void {
  if (signal?.aborted) throw new DOMException("Oppslaget vart avbrote.", "AbortError");
  if (!admitted()) throw new Error("Innlogginga eller samtalen vart endra.");
}
function decodeView(value: unknown): GeoRollView {
  if (!isRecord(value) || !validId(value.id) || typeof value.status !== "string" || !statuses.has(value.status)
      || !Array.isArray(value.choices) || value.choices.length > 10
      || !(value.can_retry_geocoding === undefined || typeof value.can_retry_geocoding === "boolean")
      || !(value.origin_text === undefined || value.origin_text === null || (typeof value.origin_text === "string" && value.origin_text.length > 0 && value.origin_text.length <= 240))
      || !(value.error_code === null || value.error_code === undefined || typeof value.error_code === "string")
      || !(value.result_message_id === null || value.result_message_id === undefined || validId(value.result_message_id))) throw invalid();
  const seen = new Set<number>();
  const choices = value.choices.map(choice => {
    if (!isRecord(choice) || !Number.isSafeInteger(choice.index) || (choice.index as number) < 0 || (choice.index as number) > 100
        || typeof choice.label !== "string" || !choice.label.trim() || choice.label.length > 512 || seen.has(choice.index as number)) throw invalid();
    seen.add(choice.index as number); return { index: choice.index as number, label: choice.label };
  });
  if (value.status === "choose_origin" && choices.length === 0) throw invalid();
  if (value.can_retry_geocoding === true && (typeof value.origin_text !== "string" || !value.origin_text.trim())) throw invalid();
  return { id: value.id, status: value.status as GeoRollStatus, choices,
    errorCode: typeof value.error_code === "string" ? value.error_code : null,
    resultMessageId: typeof value.result_message_id === "string" ? value.result_message_id : null,
    originText: typeof value.origin_text === "string" ? value.origin_text : null, canRetryGeocoding: value.can_retry_geocoding === true };
}
function decodePreview(value: unknown): GeoRollPreview {
  if (!isRecord(value) || value.provider !== "geoapify" || typeof value.preview_binding !== "string" || !/^[a-f0-9]{64}$/u.test(value.preview_binding)
      || !(value.origin_mode === "none" || value.origin_mode === "named" || value.origin_mode === "shared")
      || !(value.suggested_origin === null || (typeof value.suggested_origin === "string" && value.suggested_origin.length <= 512))
      || value.radius_m !== 1500 || !Array.isArray(value.categories) || value.categories.length !== 2
      || value.categories[0] !== "catering.bar" || value.categories[1] !== "catering.pub" || value.state === undefined) throw invalid();
  return { binding: value.preview_binding, originMode: value.origin_mode, suggestedOrigin: value.suggested_origin as string | null,
    radius: value.radius_m as number, state: value.state === null ? null : decodeView(value.state) };
}
export class AgentGeoapifyRollApi {
  constructor(private readonly http: HttpClient) {}
  private path(channel: string, message: string): string { return `/api/v1/channels/${encodeURIComponent(channel)}/messages/${encodeURIComponent(message)}/geoapify-roll`; }
  preview(owner: string, channel: string, message: string, admitted: () => boolean, signal?: AbortSignal): Promise<GeoRollPreview> {
    if (!validId(owner) || !validId(channel) || !validId(message)) throw invalid();
    admittedNow(admitted, signal);
    return this.http.json(this.path(channel, message), value => { admittedNow(admitted, signal); return decodePreview(value); },
      { method: "GET", headers: { "x-sproyt-account": owner }, signal }, admitted).then(result => { admittedNow(admitted, signal); return result; });
  }
  start(owner: string, channel: string, message: string, preview: GeoRollPreview, originText: string, admitted: () => boolean, signal?: AbortSignal): Promise<GeoRollView> {
    if (!validId(owner) || !validId(channel) || !validId(message) || !/^[a-f0-9]{64}$/u.test(preview.binding)
        || (preview.originMode !== "shared" && (!originText.trim() || originText.length > 240 || /[\u0000-\u001f\u007f]/u.test(originText)))) throw invalid();
    admittedNow(admitted, signal);
    return this.http.json(this.path(channel, message), value => { admittedNow(admitted, signal); return decodeView(value); },
      { method: "POST", headers: { "content-type": "application/json", "x-sproyt-account": owner },
        body: JSON.stringify({ preview_binding: preview.binding, allow_shared_location: preview.originMode === "shared",
          ...(preview.originMode === "shared" ? {} : { origin_text: originText.trim() }) }), signal }, admitted)
      .then(result => { admittedNow(admitted, signal); return result; });
  }
  choose(owner: string, channel: string, message: string, rollId: string, index: number, admitted: () => boolean, signal?: AbortSignal): Promise<GeoRollView> {
    if (!validId(owner) || !validId(channel) || !validId(message) || !validId(rollId) || !Number.isSafeInteger(index) || index < 0 || index > 100) throw invalid();
    admittedNow(admitted, signal);
    return this.http.json(`${this.path(channel, message)}/origin`, value => { admittedNow(admitted, signal); return decodeView(value); },
      { method: "POST", headers: { "content-type": "application/json", "x-sproyt-account": owner },
        body: JSON.stringify({ roll_id: rollId, index }), signal }, admitted).then(result => { admittedNow(admitted, signal); return result; });
  }
}
