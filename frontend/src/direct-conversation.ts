import type { Channel, UserProfile } from "./types";

type DirectChannel = Pick<Channel, "name" | "is_direct" | "direct_user_id">;
type PersonName = Pick<UserProfile, "id" | "display_name">;

/** Handles belong in metadata. A stored group title is never split into names. */
export function directConversationTitle(channel: DirectChannel | null | undefined, profiles: readonly PersonName[] = []): string {
  if (!channel) return "Direktesamtale";
  const peer = channel.direct_user_id ? profiles.find(person => person.id === channel.direct_user_id) : undefined;
  return peer?.display_name.trim() || channel.name.trim() || (channel.is_direct && !channel.direct_user_id ? "Gruppesamtale" : "Direktesamtale");
}

/** Only an existing one-to-one channel can satisfy a person selection. */
export function existingDirectChannel(channels: readonly Readonly<Channel>[], userId: string): Readonly<Channel> | undefined {
  return channels.find(channel => channel.is_direct !== false && channel.direct_user_id === userId);
}
