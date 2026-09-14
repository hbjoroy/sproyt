import { isJsonObject, isJsonValue, isRecord, type JsonObject, type JsonValue } from "./types";

export type FetchLike = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

export class HttpError extends Error {
  constructor(readonly status: number, message: string) {
    super(message);
    this.name = "HttpError";
  }
}

export type NotificationSettings = Readonly<{
  enabled: boolean;
  publicKey: string;
  subscriptions: number;
  channelIds: ReadonlyArray<string>;
  preferences: Readonly<{ mode: "instant" | "weekly" | "muted"; directMessages: boolean; mentions: boolean }>;
}>;

export type NotificationPreferences = Readonly<{
  mode: NotificationSettings["preferences"]["mode"];
  directMessages: boolean;
  mentions: boolean;
}>;

export type ProcessView = Readonly<{
  process: Readonly<{ definitionName: string; status: string }>;
  events: ReadonlyArray<Readonly<{ eventType: string; actorId: string; payload: JsonObject }>>;
}>;

export type CreatedAgent = Readonly<{ agentId: string; credential: string }>;
export type EnrollmentInvitation = Readonly<{ url: string; expiresAt: string }>;
export type EventPlanningRequest = Readonly<{ channelId: string; requestId: string; title: string }>;
export type HttpClientDependencies = Readonly<{ fetch?: FetchLike; refreshSession?: () => Promise<boolean>; participant?: () => string | null }>;

export class HttpClient {
  readonly #fetch: FetchLike;
  readonly #refreshSession: (() => Promise<boolean>) | undefined;
  readonly #participant: () => string | null;

  constructor(dependencies: HttpClientDependencies = {}) {
    this.#fetch = dependencies.fetch ?? globalThis.fetch.bind(globalThis);
    this.#refreshSession = dependencies.refreshSession;
    this.#participant = dependencies.participant ?? (() => null);
  }

  async request(path: string, options: RequestInit = {}): Promise<Response> {
    const participant = this.#participant();
    const separator = path.includes("?") ? "&" : "?";
    const url = participant ? `${path}${separator}participant=${encodeURIComponent(participant)}` : path;
    const init: RequestInit = { credentials: "same-origin", cache: "no-store", ...options };
    let response = await this.#fetch(url, init);
    if (response.status === 401 && this.#refreshSession && await this.#refreshSession()) response = await this.#fetch(url, init);
    return response;
  }

  async json<T>(path: string, decoder: (value: unknown) => T, options: RequestInit = {}): Promise<T> {
    const response = await this.request(path, options);
    if (!response.ok) throw await HttpClient.error(response);
    return decoder(await HttpClient.jsonBody(response));
  }

  async empty(path: string, options: RequestInit = {}): Promise<void> {
    const response = await this.request(path, options);
    if (!response.ok) throw await HttpClient.error(response);
  }

  static async jsonBody(response: Response): Promise<JsonValue | null> {
    const text = await response.text();
    if (text.length === 0) return null;
    const parsed: unknown = JSON.parse(text);
    if (!isJsonValue(parsed)) throw new Error("Svaret inneheld ikkje gyldig JSON");
    return parsed;
  }

  static async error(response: Response): Promise<HttpError> {
    const text = await response.text();
    return new HttpError(response.status, text || `HTTP ${response.status}`);
  }
}

export class NotificationApi {
  constructor(private readonly http: HttpClient) {}
  get(): Promise<NotificationSettings> { return this.http.json("/api/v1/me/notifications", decodeNotificationSettings, { headers: { accept: "application/json" } }); }
  save(preferences: NotificationPreferences): Promise<void> {
    return this.http.empty("/api/v1/me/notifications", { method: "PUT", headers: jsonHeaders(), body: JSON.stringify({ mode: preferences.mode, direct_messages: preferences.directMessages, mentions: preferences.mentions, weekly_weekday: 1 }) });
  }
  registerPush(subscription: unknown): Promise<void> {
    const serialized = JSON.stringify(subscription);
    if (serialized === undefined) throw new Error("Ugyldig Push-abonnement");
    return this.http.empty("/api/v1/me/push-subscriptions", { method: "POST", headers: jsonHeaders(), body: serialized });
  }
  setChannel(channelId: string, enabled: boolean): Promise<void> {
    return this.http.empty(`/api/v1/channels/${encodeURIComponent(channelId)}/notifications`, { method: enabled ? "PUT" : "DELETE" });
  }
}

export class ProcessApi {
  constructor(private readonly http: HttpClient) {}
  setHeartFeature(circleId: string, enabled: boolean): Promise<void> { return this.http.empty(`/api/v1/circles/${encodeURIComponent(circleId)}/features/heart-event-planning`, jsonPost({ enabled })); }
  startEventPlanning(request: EventPlanningRequest): Promise<string> { return this.http.json("/api/v1/processes", decodeProcessStarted, jsonPost({ channel_id: request.channelId, request_id: request.requestId, namespace: "sproyt", definition_name: "event-planning", definition_version: "1", metadata: { title: request.title } })); }
  get(processId: string): Promise<ProcessView> { return this.http.json(`/api/v1/processes/${encodeURIComponent(processId)}`, decodeProcessView); }
  inspect(processId: string, requestId: string): Promise<void> { return this.http.empty(`/api/v1/processes/${encodeURIComponent(processId)}/inspect`, jsonPost({ request_id: requestId })); }
  answer(processId: string, requestId: string, answer: "yes" | "no"): Promise<void> { return this.http.empty(`/api/v1/processes/${encodeURIComponent(processId)}/messages`, jsonPost({ request_id: requestId, payload: { answer } })); }
}

export class AgentApi {
  constructor(private readonly http: HttpClient) {}
  create(input: Readonly<{ displayName: string; provider: string; serviceIdentity: string; purpose: string; rateLimitPerMinute: number; expiresAt: string }>): Promise<CreatedAgent> {
    return this.http.json("/api/v1/agents", decodeCreatedAgent, jsonPost({ display_name: input.displayName, provider: input.provider, service_identity: input.serviceIdentity, purpose: input.purpose, rate_limit_per_minute: input.rateLimitPerMinute, expires_at: input.expiresAt }));
  }
  grant(agentId: string, channelId: string, scope: "read_history" | "send_messages", expiresAt: string): Promise<void> { return this.http.empty(`/api/v1/agents/${encodeURIComponent(agentId)}/grants`, jsonPost({ circle_id: null, channel_id: channelId, scope, expires_at: expiresAt })); }
  revoke(agentId: string): Promise<void> { return this.http.empty(`/api/v1/agents/${encodeURIComponent(agentId)}/revoke`, { method: "POST", headers: jsonHeaders() }); }
}

export class EnrollmentApi {
  constructor(private readonly http: HttpClient) {}
  create(circleId: string, input: Readonly<{ email: string; displayName?: string }>): Promise<EnrollmentInvitation> {
    return this.http.json(
      `/api/v1/circles/${encodeURIComponent(circleId)}/enrollment-invitations`,
      decodeEnrollmentInvitation,
      jsonPost({ email: input.email, display_name: input.displayName || null })
    );
  }
}

/** The enrollment endpoint deliberately uses this response while Authentik is absent. */
export function isEnrollmentNotConfigured(error: unknown): boolean {
  return error instanceof HttpError
    && error.status === 503
    && /ikkje konfigurert/i.test(error.message);
}

function jsonHeaders(): HeadersInit { return { accept: "application/json", "content-type": "application/json" }; }
function jsonPost(body: JsonObject): RequestInit { return { method: "POST", headers: jsonHeaders(), body: JSON.stringify(body) }; }

export function decodeNotificationSettings(value: unknown): NotificationSettings {
  if (!isRecord(value) || !isRecord(value.preferences)) throw new Error("Ugyldige varselinnstillingar");
  const mode = value.preferences.mode;
  if ((mode !== "instant" && mode !== "weekly" && mode !== "muted")
    || typeof value.preferences.direct_messages !== "boolean"
    || typeof value.preferences.mentions !== "boolean"
    || typeof value.enabled !== "boolean") throw new Error("Ugyldige varselinnstillingar");
  return {
    enabled: value.enabled,
    publicKey: typeof value.public_key === "string" ? value.public_key : "",
    subscriptions: typeof value.subscriptions === "number" && Number.isFinite(value.subscriptions) ? value.subscriptions : 0,
    channelIds: Array.isArray(value.channel_ids) ? value.channel_ids.filter((id): id is string => typeof id === "string") : [],
    preferences: { mode, directMessages: value.preferences.direct_messages, mentions: value.preferences.mentions }
  };
}

export function decodeProcessView(value: unknown): ProcessView {
  if (!isRecord(value) || !isRecord(value.process) || !Array.isArray(value.events) || typeof value.process.definition_name !== "string" || typeof value.process.status !== "string") throw new Error("Ugyldig prosess-svar");
  const events = value.events.map((event) => {
    if (!isRecord(event) || typeof event.event_type !== "string" || typeof event.actor_id !== "string" || !isJsonObject(event.payload)) throw new Error("Prosessen inneheld eit ugyldig event");
    return { eventType: event.event_type, actorId: event.actor_id, payload: event.payload };
  });
  return { process: { definitionName: value.process.definition_name, status: value.process.status }, events };
}

export function decodeCreatedAgent(value: unknown): CreatedAgent {
  if (!isRecord(value) || typeof value.agent_id !== "string" || typeof value.credential !== "string") throw new Error("Ugyldig svar ved oppretting av agenttilgang.");
  return { agentId: value.agent_id, credential: value.credential };
}

export function decodeEnrollmentInvitation(value: unknown): EnrollmentInvitation {
  if (!isRecord(value) || typeof value.url !== "string" || typeof value.expires_at !== "string") throw new Error("Ugyldig registreringsinvitasjon.");
  let url: URL;
  try {
    url = new URL(value.url);
  } catch (_) {
    throw new Error("Utrygg registreringsinvitasjon.");
  }
  const expiresAt = new Date(value.expires_at);
  if (url.protocol !== "https:" || !url.hostname || url.username || url.password || !Number.isFinite(expiresAt.getTime())) {
    throw new Error("Utrygg registreringsinvitasjon.");
  }
  return { url: url.toString(), expiresAt: value.expires_at };
}

function decodeProcessStarted(value: unknown): string {
  if (!isRecord(value) || typeof value.process_link_id !== "string") throw new Error("Prosessen manglar ID i serversvaret.");
  return value.process_link_id;
}

export async function readJson(response: Response): Promise<JsonValue | null> {
  return HttpClient.jsonBody(response);
}

export async function sameOriginJson(
  path: string,
  method: string = "GET",
  body?: JsonValue
): Promise<Readonly<{ response: Response; body: JsonValue | null }>> {
  const http = new HttpClient();
  const response = await http.request(path, { method, headers: body === undefined ? undefined : jsonHeaders(), body: body === undefined ? undefined : JSON.stringify(body) });
  if (!response.ok) throw await HttpClient.error(response);
  return { response, body: await HttpClient.jsonBody(response) };
}
