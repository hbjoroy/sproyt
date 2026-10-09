import { HttpClient } from "./api";
import { isRecord } from "./types";

export type AgentLocation = Readonly<{
  latitude: number;
  longitude: number;
  accuracyM: number;
  observedAt: string;
  expiresAt: string;
}>;

export type LocationAgent = Readonly<{
  id: string;
  name: string;
  location: AgentLocation | null;
}>;

export type ShareAgentLocation = Readonly<{
  latitude: number;
  longitude: number;
  accuracyM: number;
  observedAt: string;
}>;

function invalid(): Error { return new Error("Ugyldig posisjonssvar frå tenaren."); }

function timestamp(value: unknown): value is string {
  return typeof value === "string" && value.length <= 64 && Number.isFinite(Date.parse(value));
}

function coordinate(value: unknown, minimum: number, maximum: number): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= minimum && value <= maximum;
}

export function decodeAgentLocation(value: unknown): AgentLocation {
  if (!isRecord(value)
    || !coordinate(value.latitude, -90, 90)
    || !coordinate(value.longitude, -180, 180)
    || typeof value.accuracy_m !== "number" || !Number.isFinite(value.accuracy_m) || value.accuracy_m < 0 || value.accuracy_m > 100_000
    || !timestamp(value.observed_at) || !timestamp(value.expires_at)
    || Date.parse(value.expires_at) <= Date.parse(value.observed_at)) throw invalid();
  return {
    latitude: value.latitude,
    longitude: value.longitude,
    accuracyM: value.accuracy_m,
    observedAt: value.observed_at,
    expiresAt: value.expires_at
  };
}

export function decodeLocationAgents(value: unknown): readonly LocationAgent[] {
  if (!isRecord(value) || !Array.isArray(value.agents) || value.agents.length > 100) throw invalid();
  const agents = value.agents.map(item => {
    if (!isRecord(item) || typeof item.id !== "string" || !item.id.trim() || item.id.length > 256
      || typeof item.name !== "string" || !item.name.trim() || item.name.length > 256
      || !(item.location === null || isRecord(item.location))) throw invalid();
    return { id: item.id, name: item.name, location: item.location === null ? null : decodeAgentLocation(item.location) };
  });
  if (new Set(agents.map(agent => agent.id)).size !== agents.length) throw invalid();
  return agents;
}

export class AgentLocationApi {
  constructor(private readonly http: HttpClient) {}

  private path(channelId: string, agentId?: string): string {
    const base = `/api/v1/channels/${encodeURIComponent(channelId)}/agent-locations`;
    return agentId ? `${base}/${encodeURIComponent(agentId)}` : base;
  }

  list(channelId: string, signal?: AbortSignal): Promise<readonly LocationAgent[]> {
    return this.http.json(this.path(channelId), decodeLocationAgents, { signal });
  }

  share(channelId: string, agentId: string, location: ShareAgentLocation, signal?: AbortSignal): Promise<AgentLocation> {
    return this.http.json(this.path(channelId, agentId), decodeAgentLocation, {
      method: "PUT",
      headers: { accept: "application/json", "content-type": "application/json" },
      body: JSON.stringify({
        latitude: location.latitude,
        longitude: location.longitude,
        accuracy_m: location.accuracyM,
        observed_at: location.observedAt
      }),
      signal
    });
  }

  remove(channelId: string, agentId: string, signal?: AbortSignal): Promise<void> {
    return this.http.empty(this.path(channelId, agentId), { method: "DELETE", signal });
  }
}
