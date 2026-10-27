import type { ConversationSnapshot } from "./conversation-snapshot";

/** Resource IDs are captured when a destination opens, never inferred on save. */
export type CommunityDestination =
  | { kind: "people" | "create-circle" | "circles" | "global-channels" | "global-invite" | "accept-invitation" }
  | { kind: "create-channel"; circleId: string | null }
  | { kind: "channel"; channelId: string; channelSection?: ChannelSettingsSection }
  | { kind: "channel-threads"; channelId: string }
  | { kind: "channels" | "invite" | "rename-circle" | "circle-members"; circleId: string };

export type AgentSettingsPanel = "overview" | "behaviour" | "skills" | "memory" | "vision" | "images" | "weather" | "sea" | "ferry" | "handoff" | "channels";
export type ChannelSettingsSection = "about" | "members" | "agents" | "notifications";
export const validChannelSettingsSection = (value: unknown): value is ChannelSettingsSection =>
  value === "about" || value === "members" || value === "agents" || value === "notifications";

export const agentSettingsPanels: readonly AgentSettingsPanel[] = ["overview", "behaviour", "skills", "memory", "vision", "images", "weather", "sea", "ferry", "handoff", "channels"];
const uuid = (value: unknown): value is string => typeof value === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);

/** Reject malformed resource context rather than silently opening another page. */
export function validAgentSettingsRoute(agentId: unknown, panel: unknown, channelId: unknown): boolean {
  if (channelId !== undefined && !uuid(channelId)) return false;
  if (panel !== undefined && !agentSettingsPanels.includes(panel as AgentSettingsPanel)) return false;
  if (agentId === undefined) return panel === undefined || panel === "channels";
  if (agentId === "new") return panel === undefined || panel === "behaviour";
  return uuid(agentId);
}

export type ManagementTarget =
  | { kind: "overview" | "profile" | "notifications" | "welcome" | "appearance" | "help" }
  | { kind: "circle" | "memory"; circleId: string }
  | { kind: "chat-agents"; circleId: string; agentId?: string; panel?: AgentSettingsPanel; channelId?: string }
  | { kind: "community"; destination: CommunityDestination }
  | { kind: "advanced"; feature: "agent" | "heart"; channelId?: string };

/** A predictable parent for Close/Escape, shared by menus and settings search. */
export function managementParent(target: ManagementTarget, snapshot: Pick<ConversationSnapshot, "channels">): ManagementTarget | null {
  if (target.kind === "overview") return null;
  if (target.kind === "circle") return { kind: "community", destination: { kind: "circles" } };
  if (target.kind === "chat-agents") {
    if (target.agentId && target.agentId !== "new" && target.panel && target.panel !== "overview") {
      return { kind: "chat-agents", circleId: target.circleId, agentId: target.agentId, panel: "overview",
        ...(target.channelId ? { channelId: target.channelId } : {}) };
    }
    if (target.agentId || target.panel === "channels") return { kind: "chat-agents", circleId: target.circleId,
      ...(target.channelId ? { channelId: target.channelId } : {}) };
    return { kind: "circle", circleId: target.circleId };
  }
  if (target.kind === "memory") return { kind: "circle", circleId: target.circleId };
  if (target.kind === "community") {
    const destination = target.destination;
    if (destination.kind === "channel-threads") return null;
    if ("circleId" in destination && destination.circleId) return { kind: "circle", circleId: destination.circleId };
    if (destination.kind === "channel") {
      const channel = snapshot.channels.find(channel => channel.id === destination.channelId);
      return { kind: "community", destination: channel?.circle_id
        ? { kind: "channels", circleId: channel.circle_id } : { kind: "global-channels" } };
    }
  }
  return { kind: "overview" };
}
