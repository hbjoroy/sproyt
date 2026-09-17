import type { AgentApi, IntegrationApi, ProcessApi, ProcessView } from "../api";
import type { Channel, Circle } from "../types";

/** Reuses the application's authenticated API owners. Secrets are returned once,
 * never persisted here; only the revocation handle survives dialog closure. */
export function createAdvancedHost(deps: {
  agents: AgentApi; integrations: IntegrationApi; processes: ProcessApi;
  channels(): readonly Channel[]; circles(): ReadonlyMap<string, Circle>;
  capabilities(): { agent: boolean; heart: boolean };
}) {
  let agent: { id: string; channelId: string; expiresAt: string } | null = null;
  let agentBusy = false;
  let revision = 0;
  const listeners = new Set<() => void>();
  const publish = () => { revision++; for (const listener of listeners) listener(); };
  let processId = "";
  const channel = (id: string, manager = false) => {
    const value = deps.channels().find(item => item.id === id);
    if (!value || (manager && !["owner", "moderator"].includes(value.role))) throw new Error("Du har ikkje rettar til denne handlinga i kanalen.");
    return value;
  };
  const heart = () => { if (!deps.capabilities().heart) throw new Error("Heart er ikkje tilgjengeleg."); };
  return {
    revision: () => revision,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    agent: () => agent,
    agentBusy: () => agentBusy,
    async createAgent(channelId: string) {
      if (!deps.capabilities().agent) throw new Error("Agenttilgang er ikkje tilgjengeleg.");
      channel(channelId, true);
      if (agent || agentBusy) throw new Error("Trekk tilbake den førre agenttilgangen først.");
      agentBusy = true;
      publish();
      const expiresAt = new Date(Date.now() + 30 * 60_000).toISOString();
      try {
        const created = await deps.agents.create({ displayName: "Kortliva MCP-agent", provider: "sproyt-owner-ui", serviceIdentity: crypto.randomUUID(), purpose: `Kortliva MCP-tilgang til kanal ${channelId}`, rateLimitPerMinute: 30, expiresAt });
        agent = { id: created.agentId, channelId, expiresAt };
        try {
          for (const scope of ["read_history", "send_messages"] as const) await deps.agents.grant(created.agentId, channelId, scope, expiresAt);
        } catch (error) {
          try { await deps.agents.revoke(created.agentId); agent = null; }
          catch { throw new Error("Kanalrettane kunne ikkje opprettast, og tilbakekalling feila. Trekk tilbake agenttilgangen før du prøver igjen."); }
          throw error;
        }
        return created.credential;
      } finally { agentBusy = false; publish(); }
    },
    async revokeAgent() {
      if (agentBusy) throw new Error("Vent til førre handling er ferdig.");
      if (!agent) return;
      agentBusy = true;
      publish();
      try { await deps.agents.revoke(agent.id); agent = null; } finally { agentBusy = false; publish(); }
    },
    async createGrafana(channelId: string) {
      const value = channel(channelId, true);
      if (value.is_direct) throw new Error("Grafana er ikkje tilgjengeleg i direktemeldingar.");
      return deps.integrations.createGrafana(channelId);
    },
    processId: () => processId,
    async setHeart(circleId: string, enabled: boolean) {
      heart();
      if (deps.circles().get(circleId)?.role !== "owner") throw new Error("Berre kretseigaren kan endre event-planlegging.");
      await deps.processes.setHeartFeature(circleId, enabled);
    },
    async startProcess(channelId: string, title: string) {
      heart();
      const value = channel(channelId);
      if (!value.circle_id || value.is_direct) throw new Error("Vel ein kretskanal før du startar planlegging.");
      processId = await deps.processes.startEventPlanning({ channelId, title: title.trim() || "Event-planlegging", requestId: crypto.randomUUID() });
      return processId;
    },
    async getProcess(id: string): Promise<ProcessView> { heart(); processId = id; return deps.processes.get(id); },
    async inspectProcess(id: string) { heart(); await deps.processes.inspect(id, crypto.randomUUID()); },
    async answerProcess(id: string, answer: "yes" | "no") { heart(); await deps.processes.answer(id, crypto.randomUUID(), answer); }
  };
}
export type AdvancedHost = ReturnType<typeof createAdvancedHost>;
