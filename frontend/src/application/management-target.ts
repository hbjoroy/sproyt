import type { ConversationSnapshot } from "./conversation-snapshot";

/** Resource IDs are captured when a destination opens, never inferred on save. */
export type CommunityDestination =
  | { kind: "people" | "create-circle" | "circles" | "global-channels" | "global-invite" }
  | { kind: "create-channel"; circleId: string | null }
  | { kind: "channel"; channelId: string }
  | { kind: "channels" | "invite" | "rename-circle" | "circle-members"; circleId: string };

export type ManagementTarget =
  | { kind: "overview" | "profile" | "notifications" }
  | { kind: "circle" | "chat-agents" | "memory"; circleId: string }
  | { kind: "community"; destination: CommunityDestination }
  | { kind: "advanced"; feature: "agent" | "heart"; channelId?: string };

/** A predictable parent for Close/Escape, shared by menus and settings search. */
export function managementParent(target: ManagementTarget, snapshot: Pick<ConversationSnapshot, "channels">): ManagementTarget | null {
  if (target.kind === "overview") return null;
  if (target.kind === "circle") return { kind: "community", destination: { kind: "circles" } };
  if (target.kind === "chat-agents" || target.kind === "memory") return { kind: "circle", circleId: target.circleId };
  if (target.kind === "community") {
    const destination = target.destination;
    if ("circleId" in destination && destination.circleId) return { kind: "circle", circleId: destination.circleId };
    if (destination.kind === "channel") {
      const channel = snapshot.channels.find(channel => channel.id === destination.channelId);
      return { kind: "community", destination: channel?.circle_id
        ? { kind: "channels", circleId: channel.circle_id } : { kind: "global-channels" } };
    }
  }
  return { kind: "overview" };
}
