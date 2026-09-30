import { HttpClient } from "./api";
import { isRecord } from "./types";

export type CircleChatAgent = Readonly<{
  agentId: string;
  circleId: string;
  displayName: string;
  triggerWords: readonly string[];
  responsePhrases: readonly string[];
  enabled: boolean;
  revision: number;
  workerAvailable: boolean;
}>;

export type CircleChatAgentInput = Readonly<{
  displayName: string;
  triggerWords: readonly string[];
  responsePhrases: readonly string[];
  enabled: boolean;
  revision?: number;
}>;

function decodeAgent(value: unknown): CircleChatAgent {
  if (!isRecord(value) || typeof value.agent_id !== "string" || typeof value.circle_id !== "string"
    || typeof value.display_name !== "string" || !Array.isArray(value.trigger_words)
    || !value.trigger_words.every(item => typeof item === "string")
    || !Array.isArray(value.response_phrases) || !value.response_phrases.every(item => typeof item === "string")
    || typeof value.enabled !== "boolean" || typeof value.revision !== "number"
    || typeof value.worker_available !== "boolean") throw new Error("Ugyldig agentsvar frå tenaren.");
  return { agentId: value.agent_id, circleId: value.circle_id, displayName: value.display_name,
    triggerWords: value.trigger_words, responsePhrases: value.response_phrases,
    enabled: value.enabled, revision: value.revision, workerAvailable: value.worker_available };
}

export class CircleChatAgentApi {
  constructor(private readonly http: HttpClient) {}

  async list(circleId: string): Promise<{ agents: CircleChatAgent[]; workerAvailable: boolean }> {
    const response = await this.http.json(`/api/v1/circles/${encodeURIComponent(circleId)}/chat-agents`, value => value);
    if (!isRecord(response) || !Array.isArray(response.agents) || typeof response.worker_available !== "boolean")
      throw new Error("Ugyldig agentliste frå tenaren.");
    return { agents: response.agents.map(decodeAgent), workerAvailable: response.worker_available };
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
      response_phrases: input.responsePhrases, enabled: input.enabled, revision: input.revision };
  }
}
