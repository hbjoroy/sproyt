import { HttpClient } from "./api";
import { isRecord } from "./types";

export type AgentWeather = Readonly<{ location: string; latitude: number; longitude: number }>;
export type AgentSeaTemperature = Readonly<{ defaultLocation: string; locations: readonly string[] }>;
export type AgentSeaTemperatureLocation = Readonly<{ id: string; name: string }>;
export type AgentHandoffInput = Readonly<{ revision: number; targetAgentIds: readonly string[]; guidance: string }>;
export type AgentHandoff = AgentHandoffInput & Readonly<{ available: boolean; agentRevision: number }>;

export function validAgentHandoff(value: AgentHandoffInput): boolean {
  return Number.isSafeInteger(value.revision) && value.revision >= 0
    && value.targetAgentIds.length <= 4
    && value.targetAgentIds.every(id => /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/iu.test(id))
    && new Set(value.targetAgentIds.map(id => id.toLowerCase())).size === value.targetAgentIds.length
    && [...value.guidance].length <= 1000;
}

function decodeHandoff(value: unknown): AgentHandoff {
  if (!isRecord(value) || typeof value.available !== "boolean" || !Number.isSafeInteger(value.revision)
    || !Number.isSafeInteger(value.agent_revision) || Number(value.agent_revision) < 1
    || typeof value.guidance !== "string" || !Array.isArray(value.target_agent_ids)
    || !value.target_agent_ids.every(id => typeof id === "string")) throw new Error("Ugyldig videresendingsoppsett frå tenaren.");
  const result: AgentHandoff = { available: value.available, revision: Number(value.revision),
    agentRevision: Number(value.agent_revision), targetAgentIds: value.target_agent_ids as string[], guidance: value.guidance };
  if (!validAgentHandoff(result)) throw new Error("Ugyldig videresendingsoppsett frå tenaren.");
  return result;
}

export function validAgentSeaTemperature(value: AgentSeaTemperature): boolean {
  return value.locations.length >= 1 && value.locations.length <= 8
    && value.locations.every(id => id.length >= 1 && id.length <= 80 && id.trim() === id)
    && new Set(value.locations).size === value.locations.length && value.locations.includes(value.defaultLocation);
}
export type AgentImageGeneration = Readonly<{ identityId: string; enabled: boolean; occasional: boolean }>;
export type AgentImageIdentity = Readonly<{ id: string; label: string }>;

export function validAgentWeather(value: AgentWeather): boolean {
  const length = [...value.location.trim()].length;
  return length >= 1 && length <= 80 && Number.isFinite(value.latitude) && Math.abs(value.latitude) <= 90
    && Number.isFinite(value.longitude) && Math.abs(value.longitude) <= 180;
}

export type CircleChatAgent = Readonly<{
  agentId: string;
  circleId: string;
  displayName: string;
  triggerWords: readonly string[];
  responsePhrases: readonly string[];
  enabled: boolean;
  revision: number;
  workerAvailable: boolean;
  memoryEnabled?: boolean;
  weather: AgentWeather | null;
  seaTemperature?: AgentSeaTemperature | null;
  ferryPort?: "paros" | null;
  visionEnabled: boolean;
  visionAvailable: boolean;
  imageGeneration: AgentImageGeneration | null;
  imageGenerationAvailable: boolean;
}>;

export type CircleChatAgentInput = Readonly<{
  displayName: string;
  triggerWords: readonly string[];
  responsePhrases: readonly string[];
  enabled: boolean;
  revision?: number;
  memoryEnabled?: boolean;
  weather?: AgentWeather | null;
  seaTemperature?: AgentSeaTemperature | null;
  ferryPort?: "paros" | null;
  visionEnabled?: boolean;
  imageGeneration?: AgentImageGeneration | null;
}>;

export type ChannelChatAgents = Readonly<{
  accessRevision: number;
  selectionAvailable: boolean;
  agents: readonly Readonly<{ agentId: string; displayName: string; agentEnabled: boolean; enabled: boolean }>[];
}>;

function decodeChannelAgents(value: unknown): ChannelChatAgents {
  if (!isRecord(value) || !Number.isSafeInteger(value.access_revision) || Number(value.access_revision) < 1
    || typeof value.selection_available !== "boolean" || !Array.isArray(value.agents) || value.agents.length > 10) throw new Error("Ugyldig kanalagentsvar frå tenaren.");
  return { accessRevision: Number(value.access_revision), selectionAvailable: value.selection_available, agents: value.agents.map(agent => {
    if (!isRecord(agent) || typeof agent.agent_id !== "string" || typeof agent.display_name !== "string"
      || typeof agent.agent_enabled !== "boolean" || typeof agent.enabled !== "boolean")
      throw new Error("Ugyldig kanalagent frå tenaren.");
    return { agentId: agent.agent_id, displayName: agent.display_name, agentEnabled: agent.agent_enabled, enabled: agent.enabled };
  }) };
}

function decodeAgent(value: unknown): CircleChatAgent {
  if (!isRecord(value) || typeof value.agent_id !== "string" || typeof value.circle_id !== "string"
    || typeof value.display_name !== "string" || !Array.isArray(value.trigger_words)
    || !value.trigger_words.every(item => typeof item === "string")
    || !Array.isArray(value.response_phrases) || !value.response_phrases.every(item => typeof item === "string")
    || typeof value.enabled !== "boolean" || typeof value.revision !== "number"
    || typeof value.worker_available !== "boolean"
    || (value.memory_enabled !== undefined && typeof value.memory_enabled !== "boolean")
    || (value.vision_enabled !== undefined && typeof value.vision_enabled !== "boolean")
    || (value.vision_available !== undefined && typeof value.vision_available !== "boolean")
    || (value.image_generation_available !== undefined && typeof value.image_generation_available !== "boolean")) throw new Error("Ugyldig agentsvar frå tenaren.");
  let imageGeneration: AgentImageGeneration | null = null;
  if (value.image_generation !== undefined && value.image_generation !== null) {
    const images = value.image_generation;
    if (!isRecord(images) || typeof images.identity_id !== "string" || !images.identity_id.length || images.identity_id.length > 80
      || typeof images.enabled !== "boolean" || typeof images.occasional !== "boolean") throw new Error("Ugyldig biletoppsett frå tenaren.");
    imageGeneration = { identityId: images.identity_id, enabled: images.enabled, occasional: images.occasional };
  }
  let weather: AgentWeather | null = null;
  if (value.weather !== undefined && value.weather !== null) {
    if (!isRecord(value.weather) || typeof value.weather.location !== "string"
      || typeof value.weather.latitude !== "number" || typeof value.weather.longitude !== "number"
      || !validAgentWeather(value.weather as AgentWeather)) throw new Error("Ugyldig vêroppsett frå tenaren.");
    weather = { location: value.weather.location, latitude: value.weather.latitude, longitude: value.weather.longitude };
  }
  let seaTemperature: AgentSeaTemperature | null = null;
  if (value.sea_temperature !== undefined && value.sea_temperature !== null) {
    const sea = value.sea_temperature;
    if (!isRecord(sea) || typeof sea.default_location !== "string" || !Array.isArray(sea.locations)
      || !sea.locations.every(id => typeof id === "string")
      || !validAgentSeaTemperature({ defaultLocation: sea.default_location, locations: sea.locations }))
      throw new Error("Ugyldig sjøtemperaturoppsett frå tenaren.");
    seaTemperature = { defaultLocation: sea.default_location, locations: sea.locations };
  }
  if (value.ferry_port !== undefined && value.ferry_port !== null && value.ferry_port !== "paros")
    throw new Error("Ugyldig fergehamn frå tenaren.");
  return { agentId: value.agent_id, circleId: value.circle_id, displayName: value.display_name,
    triggerWords: value.trigger_words, responsePhrases: value.response_phrases,
    enabled: value.enabled, revision: value.revision, workerAvailable: value.worker_available, weather, seaTemperature,
    memoryEnabled: value.memory_enabled === true,
    ferryPort: value.ferry_port === "paros" ? "paros" : null,
    visionEnabled: value.vision_enabled === true, visionAvailable: value.vision_available === true,
    imageGeneration, imageGenerationAvailable: value.image_generation_available === true };
}

export class CircleChatAgentApi {
  constructor(private readonly http: HttpClient, private readonly scope?: { owner: string; admitted: () => boolean }) {}

  forOwner(owner: string, admitted: () => boolean): CircleChatAgentApi {
    return new CircleChatAgentApi(this.http, { owner, admitted });
  }

  private async json<T>(path: string, decoder: (value: unknown) => T, options: RequestInit = {}): Promise<T> {
    let scopedOptions = options;
    if (this.scope) {
      const headers = new Headers(options.headers);
      headers.set("x-sproyt-account", this.scope.owner);
      scopedOptions = { ...options, headers, signal: options.signal ?? AbortSignal.timeout(20_000) };
    }
    const result = await this.http.json(path, decoder, scopedOptions, this.scope?.admitted);
    if (this.scope && !this.scope.admitted()) throw new Error("Innlogginga vart endra. Prøv igjen på rett konto.");
    return result;
  }

  listChannel(channelId: string): Promise<ChannelChatAgents> {
    return this.json(`/api/v1/channels/${encodeURIComponent(channelId)}/chat-agents`, decodeChannelAgents);
  }

  selectChannel(channelId: string, agentId: string, enabled: boolean, accessRevision: number): Promise<ChannelChatAgents> {
    return this.json(`/api/v1/channels/${encodeURIComponent(channelId)}/chat-agents/${encodeURIComponent(agentId)}`, decodeChannelAgents,
      { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify({ enabled, access_revision: accessRevision }) });
  }

  async list(circleId: string): Promise<{ agents: CircleChatAgent[]; workerAvailable: boolean; weatherAvailable: boolean; seaTemperatureAvailable: boolean; seaTemperatureLocations: AgentSeaTemperatureLocation[]; ferryAvailable: boolean; visionAvailable: boolean; imageGenerationAvailable: boolean; imageIdentities: AgentImageIdentity[] }> {
    const response = await this.json(`/api/v1/circles/${encodeURIComponent(circleId)}/chat-agents`, value => value);
    if (this.scope && !this.scope.admitted()) throw new Error("Innlogginga vart endra. Prøv igjen på rett konto.");
    if (!isRecord(response) || !Array.isArray(response.agents) || typeof response.worker_available !== "boolean"
      || (response.weather_available !== undefined && typeof response.weather_available !== "boolean")
      || (response.sea_temperature_available !== undefined && typeof response.sea_temperature_available !== "boolean")
      || (response.ferry_available !== undefined && typeof response.ferry_available !== "boolean")
      || (response.vision_available !== undefined && typeof response.vision_available !== "boolean")
      || (response.image_generation_available !== undefined && typeof response.image_generation_available !== "boolean"))
      throw new Error("Ugyldig agentliste frå tenaren.");
    const seaLocations = response.sea_temperature_locations === undefined ? [] : response.sea_temperature_locations;
    if (!Array.isArray(seaLocations) || seaLocations.length > 50 || !seaLocations.every(location => isRecord(location)
      && typeof location.id === "string" && location.id.length > 0 && location.id.length <= 80 && location.id.trim() === location.id
      && typeof location.name === "string" && location.name.trim().length > 0 && location.name.length <= 120)
      || new Set(seaLocations.map(location => location.id)).size !== seaLocations.length)
      throw new Error("Ugyldig sjøtemperaturstad frå tenaren.");
    const identities = response.image_identities ?? [];
    if (!Array.isArray(identities) || identities.length > 50 || !identities.every(identity => isRecord(identity)
      && typeof identity.id === "string" && identity.id.length > 0 && identity.id.length <= 80
      && typeof identity.label === "string" && identity.label.length > 0 && identity.label.length <= 120)
      || new Set(identities.map(identity => identity.id)).size !== identities.length) throw new Error("Ugyldig biletidentitet frå tenaren.");
    return { agents: response.agents.map(decodeAgent), workerAvailable: response.worker_available,
      weatherAvailable: response.weather_available === true, ferryAvailable: response.ferry_available === true,
      seaTemperatureAvailable: response.sea_temperature_available === true,
      seaTemperatureLocations: seaLocations.map(location => ({ id: String(location.id), name: String(location.name) })),
      visionAvailable: response.vision_available === true, imageGenerationAvailable: response.image_generation_available === true,
      imageIdentities: identities.map(identity => ({ id: String(identity.id), label: String(identity.label) })) };
  }

  async create(circleId: string, input: CircleChatAgentInput): Promise<CircleChatAgent> {
    return this.json(`/api/v1/circles/${encodeURIComponent(circleId)}/chat-agents`, decodeAgent,
      { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(this.body(input)) });
  }

  async update(circleId: string, agentId: string, input: CircleChatAgentInput): Promise<CircleChatAgent> {
    return this.json(`/api/v1/circles/${encodeURIComponent(circleId)}/chat-agents/${encodeURIComponent(agentId)}`, decodeAgent,
      { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(this.body(input)) });
  }

  handoff(circleId: string, agentId: string): Promise<AgentHandoff> {
    return this.json(`/api/v1/circles/${encodeURIComponent(circleId)}/chat-agents/${encodeURIComponent(agentId)}/handoff`, decodeHandoff);
  }

  updateHandoff(circleId: string, agentId: string, input: AgentHandoffInput): Promise<AgentHandoff> {
    if (!validAgentHandoff(input) || input.targetAgentIds.some(id => id.toLowerCase() === agentId.toLowerCase())) return Promise.reject(new Error("Vel inntil fire andre agentar og ei gyldig rettleiing."));
    return this.json(`/api/v1/circles/${encodeURIComponent(circleId)}/chat-agents/${encodeURIComponent(agentId)}/handoff`, decodeHandoff,
      { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({
        revision: input.revision, target_agent_ids: input.targetAgentIds, guidance: input.guidance
      }) });
  }

  private body(input: CircleChatAgentInput) {
    return { display_name: input.displayName, trigger_words: input.triggerWords,
      response_phrases: input.responsePhrases, enabled: input.enabled, revision: input.revision, weather: input.weather,
      ferry_port: input.ferryPort, vision_enabled: input.visionEnabled, memory_enabled: input.memoryEnabled,
      sea_temperature: input.seaTemperature === undefined ? undefined : input.seaTemperature === null ? null
        : { default_location: input.seaTemperature.defaultLocation, locations: input.seaTemperature.locations },
      image_generation: input.imageGeneration === undefined ? undefined : input.imageGeneration === null ? null
        : { identity_id: input.imageGeneration.identityId, enabled: input.imageGeneration.enabled, occasional: input.imageGeneration.occasional } };
  }
}
