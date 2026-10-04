import { HttpClient } from "./api";
import { isRecord } from "./types";

export type AgentWeather = Readonly<{ location: string; latitude: number; longitude: number }>;

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
  weather: AgentWeather | null;
  ferryPort?: "paros" | null;
  visionEnabled: boolean;
  visionAvailable: boolean;
}>;

export type CircleChatAgentInput = Readonly<{
  displayName: string;
  triggerWords: readonly string[];
  responsePhrases: readonly string[];
  enabled: boolean;
  revision?: number;
  weather?: AgentWeather | null;
  ferryPort?: "paros" | null;
  visionEnabled?: boolean;
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
    || (value.vision_enabled !== undefined && typeof value.vision_enabled !== "boolean")
    || (value.vision_available !== undefined && typeof value.vision_available !== "boolean")) throw new Error("Ugyldig agentsvar frå tenaren.");
  let weather: AgentWeather | null = null;
  if (value.weather !== undefined && value.weather !== null) {
    if (!isRecord(value.weather) || typeof value.weather.location !== "string"
      || typeof value.weather.latitude !== "number" || typeof value.weather.longitude !== "number"
      || !validAgentWeather(value.weather as AgentWeather)) throw new Error("Ugyldig vêroppsett frå tenaren.");
    weather = { location: value.weather.location, latitude: value.weather.latitude, longitude: value.weather.longitude };
  }
  if (value.ferry_port !== undefined && value.ferry_port !== null && value.ferry_port !== "paros")
    throw new Error("Ugyldig fergehamn frå tenaren.");
  return { agentId: value.agent_id, circleId: value.circle_id, displayName: value.display_name,
    triggerWords: value.trigger_words, responsePhrases: value.response_phrases,
    enabled: value.enabled, revision: value.revision, workerAvailable: value.worker_available, weather,
    ferryPort: value.ferry_port === "paros" ? "paros" : null,
    visionEnabled: value.vision_enabled === true, visionAvailable: value.vision_available === true };
}

export class CircleChatAgentApi {
  constructor(private readonly http: HttpClient) {}

  listChannel(channelId: string): Promise<ChannelChatAgents> {
    return this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/chat-agents`, decodeChannelAgents);
  }

  selectChannel(channelId: string, agentId: string, enabled: boolean, accessRevision: number): Promise<ChannelChatAgents> {
    return this.http.json(`/api/v1/channels/${encodeURIComponent(channelId)}/chat-agents/${encodeURIComponent(agentId)}`, decodeChannelAgents,
      { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify({ enabled, access_revision: accessRevision }) });
  }

  async list(circleId: string): Promise<{ agents: CircleChatAgent[]; workerAvailable: boolean; weatherAvailable: boolean; ferryAvailable: boolean; visionAvailable: boolean }> {
    const response = await this.http.json(`/api/v1/circles/${encodeURIComponent(circleId)}/chat-agents`, value => value);
    if (!isRecord(response) || !Array.isArray(response.agents) || typeof response.worker_available !== "boolean"
      || (response.weather_available !== undefined && typeof response.weather_available !== "boolean")
      || (response.ferry_available !== undefined && typeof response.ferry_available !== "boolean")
      || (response.vision_available !== undefined && typeof response.vision_available !== "boolean"))
      throw new Error("Ugyldig agentliste frå tenaren.");
    return { agents: response.agents.map(decodeAgent), workerAvailable: response.worker_available,
      weatherAvailable: response.weather_available === true, ferryAvailable: response.ferry_available === true,
      visionAvailable: response.vision_available === true };
  }

  async create(circleId: string, input: CircleChatAgentInput): Promise<CircleChatAgent> {
    return this.http.json(`/api/v1/circles/${encodeURIComponent(circleId)}/chat-agents`, decodeAgent,
      { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(this.body(input)) });
  }

  async update(circleId: string, agentId: string, input: CircleChatAgentInput): Promise<CircleChatAgent> {
    return this.http.json(`/api/v1/circles/${encodeURIComponent(circleId)}/chat-agents/${encodeURIComponent(agentId)}`, decodeAgent,
      { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(this.body(input)) });
  }

  private body(input: CircleChatAgentInput) {
    return { display_name: input.displayName, trigger_words: input.triggerWords,
      response_phrases: input.responsePhrases, enabled: input.enabled, revision: input.revision, weather: input.weather,
      ferry_port: input.ferryPort, vision_enabled: input.visionEnabled };
  }
}
