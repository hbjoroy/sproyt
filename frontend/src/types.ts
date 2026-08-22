/** Protocol DTOs and the only untrusted JSON boundary in the browser client. */
export type JsonPrimitive = null | boolean | number | string;
export type JsonValue = JsonPrimitive | JsonObject | JsonValue[];
export interface JsonObject { readonly [field: string]: JsonValue; }
export type Identifier = string;
export interface UserProfile { id: string; kind: "human" | "agent"; display_name: string; external_provider: string | null; external_subject: string | null; created_at: string; status_text: string; status_emoji: string; status_expires_at: string | null; }
export interface ChannelBase { id: string; slug: string; name: string; kind: "public" | "local" | "private"; circle_id: string | null; created_by: string; }
/** Rust ChannelSummary: deliberately does not include Channel.created_by. */
export interface Channel { id: string; slug: string; name: string; kind: "public" | "local" | "private"; circle_id: string | null; direct_user_id: string | null; description: string; role: "owner" | "moderator" | "member" | "observer"; last_read_sequence: number; latest_sequence: number; }
export interface ChatMessage { id: string; channel_id: string; parent_message_id: string | null; sender_id: string; sender_display_name: string; body: string; sequence: number; sent_at: string; edited_at: string | null; deleted_at: string | null; }
export interface CircleBase { id: string; slug: string; name: string; created_by: string; created_at: string; }
export interface Circle extends CircleBase { role: "owner" | "member"; }
export interface Membership { channel_id: string; user_id: string; role: "owner" | "moderator" | "member" | "observer"; last_read_sequence: number; }
export interface ThreadSummary { root_message_id: string; reply_count: number; unread_count: number; latest_sequence: number; }
export interface MessageReactionSummary { message_id: string; emoji: string; count: number; reacted_by_me: boolean; user_ids: string[]; }
export interface MessageReactionChange { message_id: string; channel_id: string; user_id: string; emoji: string; added: boolean; count: number; }
export interface Mention { read: boolean; message: ChatMessage; channel_name: string; }
export interface UserTask { id: string; source_message_id: string; channel_id: string; channel_name: string; assignee_id: string; created_by: string; process_link_id: string | null; title: string; status: string; created_at: string; completed_at: string | null; }
export interface MediaObject { id: string; name: string; content_type: string; channel_id: string; url?: string; original_filename: string; parent_message_id?: string; }
export interface ThreadComposerState { draft: string; composing: boolean; media: MediaObject[]; status: string; statusKind: string; hasFocus: boolean; uploadCount: number; }
export interface MermaidApi { initialize(options: Readonly<Record<string, JsonValue>>): void; run(options: Readonly<{ nodes: Element[] }>): Promise<void>; }
export interface UploadResponse { status: number; ok: boolean; headers: Readonly<{ get(name: string): string | null }>; text(): Promise<string>; json(): Promise<unknown>; }
type Command<Type extends string, Payload = never> = [Payload] extends [never]
  ? Readonly<{ type: Type }>
  : Readonly<{ type: Type; payload: Payload }>;

export type InvitationTarget =
  | Readonly<{ type: "circle"; circle_id: string }>
  | Readonly<{ type: "channel"; circle_id: string; channel_id: string }>;

/** Concrete mirror of every Rust `ClientCommand` variant. */
export type ClientCommand =
  | Command<"hello"> | Command<"list_users"> | Command<"list_my_channels">
  | Command<"list_thread_summaries", { channel_id: string }>
  | Command<"list_my_circles"> | Command<"list_mentions"> | Command<"list_tasks"> | Command<"ping">
  | Command<"list_circle_users", { circle_id: string }>
  | Command<"set_status", { text: string; emoji: string; expires_at: string | null }>
  | Command<"open_direct_channel", { user_id: string }>
  | Command<"create_channel", { slug: string; name: string; kind: "public" | "local" | "private"; circle_id: string | null }>
  | Command<"join_channel", { channel: { type: "id" | "slug"; value: string } }>
  | Command<"leave_channel", { channel_id: string }>
  | Command<"list_channel_users", { channel_id: string }>
  | Command<"update_channel_description", { channel_id: string; description: string }>
  | Command<"list_joinable_channels", { circle_id: string }>
  | Command<"add_channel_member", { channel_id: string; user_id: string }>
  | Command<"load_recent_messages", { channel_id: string; limit?: number | null; after?: number | null; before?: number | null }>
  | Command<"load_thread", { root_message_id: string }>
  | Command<"mark_thread_read", { root_message_id: string; sequence: number }>
  | Command<"subscribe_channel", { channel_id: string }>
  | Command<"unsubscribe_channel", { channel_id: string }>
  | Command<"send_message", { channel_id: string; parent_message_id?: string | null; body: string }>
  | Command<"edit_message", { message_id: string; body: string }>
  | Command<"delete_message", { message_id: string }>
  | Command<"list_channel_reactions", { channel_id: string }>
  | Command<"toggle_message_reaction", { message_id: string; emoji: string }>
  | Command<"mark_read", { channel_id: string; sequence: number }>
  | Command<"mark_mention_read", { message_id: string }>
  | Command<"create_task", { source_message_id: string; assignee_id: string; title: string; process_link_id: string | null }>
  | Command<"set_task_done", { task_id: string; done: boolean }>
  | Command<"create_circle", { slug: string; name: string }>
  | Command<"delete_circle", { circle_id: string }>
  | Command<"leave_circle", { circle_id: string }>
  | Command<"create_circle_invitation", { circle_id: string }>
  | Command<"accept_circle_invitation", { token: string }>
  | Command<"create_invitation", { target: InvitationTarget }>
  | Command<"inspect_invitation", { token: string }>
  | Command<"decline_invitation", { token: string }>
  | Command<"accept_invitation", { token: string }>;
export type ClientCommandType = ClientCommand["type"];
export type ClientCommandFor<Type extends ClientCommandType> = Extract<ClientCommand, { type: Type }>;
export type ClientCommandPayload<Type extends ClientCommandType> = ClientCommandFor<Type> extends Readonly<{ payload: infer Payload }> ? Payload : never;
export type ClientCommandArguments<Type extends ClientCommandType> = [ClientCommandPayload<Type>] extends [never] ? [] : [payload: ClientCommandPayload<Type>];

/** Every client command uses the versioned request/response envelope. */
export type WireCommand = Readonly<{
  protocol: typeof protocolId;
  request_id: string;
  type: ClientCommand["type"];
  payload?: JsonObject;
}>;
export const protocolId = "sproyt.chat.v1";

type Target = { type: "circle"; circle_id: string } | { type: "channel"; circle_id: string; channel_id: string };
type Preview = { target: Target; circle_name: string; channel_name: string | null; invited_by: string; invited_by_name: string; expires_at: string; response: "accepted" | "declined" | null; accepted_count: number; declined_count: number };
export type ChatEvent = { type: "channel_created"; channel_id: string; created_by: string } | { type: "participant_joined" | "participant_left"; channel_id: string; participant_id: string } | { type: "message_accepted" | "message_edited" | "message_deleted"; message: ChatMessage } | { type: "message_reaction_changed"; change: MessageReactionChange } | { type: "read_marker_updated"; channel_id: string; user_id: string; sequence: number };
/**
 * Distribute over `T`: several wire responses intentionally share a payload
 * shape, but they must remain individually discriminated for event handlers.
 */
type Frame<T extends string, P = never> = T extends T
  ? [P] extends [never]
    ? { protocol: typeof protocolId; type: T; request_id?: string }
    : { protocol: typeof protocolId; type: T; request_id?: string; payload: P }
  : never;
export type ServerEvent =
 | Frame<"hello", { participant_id: string; signup_ordinal: number | null }> | Frame<"users_listed", { users: UserProfile[] }> | Frame<"circle_users_listed", { circle_id: string; users: UserProfile[] }> | Frame<"status_updated", { profile: UserProfile }>
 | Frame<"direct_channel_opened" | "channel_created", { channel: ChannelBase }> | Frame<"membership_joined" | "channel_member_added" | "read_marker_updated", { membership: Membership }> | Frame<"membership_left" | "subscription_ended", { channel_id: string }>
 | Frame<"channels_listed", { channels: Channel[] }> | Frame<"channel_users_listed", { channel_id: string; users: UserProfile[] }> | Frame<"channel_description_updated", { channel_id: string; description: string }> | Frame<"joinable_channels_listed", { channels: { channel: ChannelBase; description: string }[] }>
 | Frame<"messages_loaded", { channel_id: string; messages: ChatMessage[] }> | Frame<"thread_loaded", { root_message_id: string; messages: ChatMessage[] }> | Frame<"thread_summaries_listed", { channel_id: string; summaries: ThreadSummary[] }> | Frame<"thread_read_updated", { summary: ThreadSummary }> | Frame<"subscription_started", { channel_id: string; history: ChatMessage[] }>
 | Frame<"message_accepted" | "message_edited" | "message_deleted", { message: ChatMessage }> | Frame<"channel_reactions_listed", { channel_id: string; reactions: MessageReactionSummary[] }> | Frame<"message_reaction_changed", { change: MessageReactionChange }> | Frame<"mentions_listed", { mentions: Mention[] }> | Frame<"mention_read", { message_id: string }> | Frame<"task_created" | "task_updated", { task: UserTask }> | Frame<"tasks_listed", { tasks: UserTask[] }> | Frame<"chat", { event: ChatEvent }>
 | Frame<"lagged", { channel_id: string; last_seen_sequence: number; latest_known_sequence: number; skipped: number; hint: string }> | Frame<"pong"> | Frame<"circle_created", { circle: CircleBase }> | Frame<"circles_listed", { circles: [CircleBase, "owner" | "member"][] }> | Frame<"circle_deleted" | "circle_left", { circle_id: string }>
 | Frame<"circle_invitation_created", { invitation: { invitation: { id: string; circle_id: string; invited_by: string; expires_at: string }; token: string } }> | Frame<"circle_invitation_accepted", { membership: { circle_id: string; user_id: string; role: "owner" | "member"; joined_at: string } }> | Frame<"invitation_created", { invitation: { target: Target; token: string; expires_at: string } }> | Frame<"invitation_inspected" | "invitation_declined", { token: string; invitation: Preview }> | Frame<"invitation_accepted", { token: string; invitation: { target: Target; channel: ChannelBase } }> | Frame<"error", { code: string; message: string }>;
export type ServerEventType = ServerEvent["type"]; export type WireEvent = ServerEvent;
export type StoredStringMap = Record<string, string>;
export function isRecord(value: unknown): value is Record<string, unknown> { return typeof value === "object" && value !== null && !Array.isArray(value); }
export function isJsonValue(value: unknown): value is JsonValue { return value === null || typeof value === "boolean" || typeof value === "number" || typeof value === "string" || (Array.isArray(value) && value.every(isJsonValue)) || (isRecord(value) && Object.values(value).every(isJsonValue)); }
export function isJsonObject(value: unknown): value is JsonObject { return isRecord(value) && Object.values(value).every(isJsonValue); }
export function isStringArray(value: unknown): value is string[] { return Array.isArray(value) && value.every((item) => typeof item === "string"); }
export function hasString(value: Record<string, unknown>, field: string): boolean { return typeof value[field] === "string"; }
export function hasNumber(value: Record<string, unknown>, field: string): boolean { return typeof value[field] === "number" && Number.isFinite(value[field]); }

const isString = (value: unknown): value is string => typeof value === "string";
const isCount = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const isBoolean = (value: unknown): value is boolean => typeof value === "boolean";
const isNullableString = (value: unknown): value is string | null => value === null || isString(value);
const isOneOf = <T extends string>(value: unknown, choices: readonly T[]): value is T => isString(value) && choices.some((choice) => choice === value);
const objectWith = (value: unknown, required: Readonly<Record<string, (entry: unknown) => boolean>>): value is Record<string, unknown> => isRecord(value) && Object.entries(required).every(([key, validate]) => validate(value[key]));
const listOf = <T>(guard: (value: unknown) => value is T) => (value: unknown): value is T[] => Array.isArray(value) && value.every(guard);
const isUser = (value: unknown): value is UserProfile => objectWith(value, { id: isString, kind: (v) => isOneOf(v, ["human", "agent"]), display_name: isString, external_provider: isNullableString, external_subject: isNullableString, created_at: isString, status_text: isString, status_emoji: isString, status_expires_at: isNullableString });
const isChannelBase = (value: unknown): value is ChannelBase => objectWith(value, { id: isString, slug: isString, name: isString, kind: (v) => isOneOf(v, ["public", "local", "private"]), circle_id: isNullableString, created_by: isString });
const isChannel = (value: unknown): value is Channel => objectWith(value, { id: isString, slug: isString, name: isString, kind: (v) => isOneOf(v, ["public", "local", "private"]), circle_id: isNullableString, direct_user_id: isNullableString, description: isString, role: (v) => isOneOf(v, ["owner", "moderator", "member", "observer"]), last_read_sequence: isCount, latest_sequence: isCount });
type WireMessage = Omit<ChatMessage, "parent_message_id" | "edited_at" | "deleted_at"> & { parent_message_id?: string | null; edited_at?: string | null; deleted_at?: string | null };
const isOptionalNullableString = (value: unknown): value is string | null | undefined => value === undefined || isNullableString(value);
const isMessage = (value: unknown): value is WireMessage => objectWith(value, { id: isString, channel_id: isString, parent_message_id: isOptionalNullableString, sender_id: isString, sender_display_name: isString, body: isString, sequence: isCount, sent_at: isString, edited_at: isOptionalNullableString, deleted_at: isOptionalNullableString });
const messageFromWire = (message: WireMessage): ChatMessage => ({ ...message, parent_message_id: message.parent_message_id ?? null, edited_at: message.edited_at ?? null, deleted_at: message.deleted_at ?? null });
const isMembership = (value: unknown): value is Membership => objectWith(value, { channel_id: isString, user_id: isString, role: (v) => isOneOf(v, ["owner", "moderator", "member", "observer"]), last_read_sequence: isCount });
const isSummary = (value: unknown): value is ThreadSummary => objectWith(value, { root_message_id: isString, reply_count: isCount, unread_count: isCount, latest_sequence: isCount });
const isReaction = (value: unknown): value is MessageReactionSummary => objectWith(value, { message_id: isString, emoji: isString, count: isCount, reacted_by_me: isBoolean, user_ids: isStringArray });
const isReactionChange = (value: unknown): value is MessageReactionChange => objectWith(value, { message_id: isString, channel_id: isString, user_id: isString, emoji: isString, added: isBoolean, count: isCount });
const isCircle = (value: unknown): value is CircleBase => objectWith(value, { id: isString, slug: isString, name: isString, created_by: isString, created_at: isString });
const isTarget = (value: unknown): value is Target => objectWith(value, { type: (v) => isOneOf(v, ["circle", "channel"]), circle_id: isString }) && (value.type === "circle" || isString(value.channel_id));
const isPreview = (value: unknown): value is Preview => objectWith(value, { target: isTarget, circle_name: isString, channel_name: isNullableString, invited_by: isString, invited_by_name: isString, expires_at: isString, response: (v) => v === null || isOneOf(v, ["accepted", "declined"]), accepted_count: isCount, declined_count: isCount });
const isTask = (value: unknown): value is UserTask => objectWith(value, { id: isString, source_message_id: isString, channel_id: isString, channel_name: isString, assignee_id: isString, created_by: isString, process_link_id: isNullableString, title: isString, status: isString, created_at: isString, completed_at: isNullableString });
const isDiscoverable = (value: unknown): value is { channel: ChannelBase; description: string } => objectWith(value, { channel: isChannelBase, description: isString });
type WireMention = Omit<Mention, "message"> & { message: WireMessage };
const isMention = (value: unknown): value is WireMention => objectWith(value, { read: isBoolean, message: isMessage, channel_name: isString });
const mentionFromWire = (mention: WireMention): Mention => ({ ...mention, message: messageFromWire(mention.message) });
type WireChatEvent = { type: "channel_created"; channel_id: string; created_by: string } | { type: "participant_joined"; channel_id: string; participant_id: string } | { type: "participant_left"; channel_id: string; participant_id: string } | { type: "message_accepted"; message: WireMessage } | { type: "message_edited"; message: WireMessage } | { type: "message_deleted"; message: WireMessage } | { type: "message_reaction_changed"; change: MessageReactionChange } | { type: "read_marker_updated"; channel_id: string; user_id: string; sequence: number };
const isChat = (value: unknown): value is WireChatEvent => isRecord(value) && ((value.type === "channel_created" && isString(value.channel_id) && isString(value.created_by)) || ((value.type === "participant_joined" || value.type === "participant_left") && isString(value.channel_id) && isString(value.participant_id)) || ((value.type === "message_accepted" || value.type === "message_edited" || value.type === "message_deleted") && isMessage(value.message)) || (value.type === "message_reaction_changed" && isReactionChange(value.change)) || (value.type === "read_marker_updated" && isString(value.channel_id) && isString(value.user_id) && isCount(value.sequence)));
const chatFromWire = (event: WireChatEvent): ChatEvent => {
  if (event.type === "message_accepted" || event.type === "message_edited" || event.type === "message_deleted") return { ...event, message: messageFromWire(event.message) };
  if (event.type === "channel_created") return { type: event.type, channel_id: event.channel_id, created_by: event.created_by };
  if (event.type === "participant_joined" || event.type === "participant_left") return { type: event.type, channel_id: event.channel_id, participant_id: event.participant_id };
  if (event.type === "message_reaction_changed") return { type: event.type, change: event.change };
  return { type: "read_marker_updated", channel_id: event.channel_id, user_id: event.user_id, sequence: event.sequence };
};
const isCirclePair = (value: unknown): value is [CircleBase, "owner" | "member"] => Array.isArray(value) && value.length === 2 && isCircle(value[0]) && isOneOf(value[1], ["owner", "member"]);
const isIssuedCircleInvitation = (value: unknown): value is { invitation: { id: string; circle_id: string; invited_by: string; expires_at: string }; token: string } => objectWith(value, { invitation: (entry) => objectWith(entry, { id: isString, circle_id: isString, invited_by: isString, expires_at: isString }), token: isString });
const isCircleMembership = (value: unknown): value is { circle_id: string; user_id: string; role: "owner" | "member"; joined_at: string } => objectWith(value, { circle_id: isString, user_id: isString, role: (entry) => isOneOf(entry, ["owner", "member"]), joined_at: isString });
const isIssuedChatInvitation = (value: unknown): value is { target: Target; token: string; expires_at: string } => objectWith(value, { target: isTarget, token: isString, expires_at: isString });
const isAcceptedChatInvitation = (value: unknown): value is { target: Target; channel: ChannelBase } => objectWith(value, { target: isTarget, channel: isChannelBase });

/** Decodes each concrete Rust `ServerEvent`, including all nested DTOs, before DOM/state handlers see it. */
export function asWireEvent(value: unknown): ServerEvent | null {
  if (!isRecord(value) || value.protocol !== protocolId || !isString(value.type) || (value.request_id !== undefined && !isString(value.request_id))) return null;
  const request_id = value.request_id;
  const payload = value.payload;
  if (value.type === "pong") return payload === undefined ? { protocol: protocolId, type: "pong", request_id } : null;
  if (!isRecord(payload)) return null;
  switch (value.type) {
    // New clients must tolerate an older pod during a rolling deployment.
    // Missing is normalised to null; malformed supplied values are rejected.
    case "hello": return isString(payload.participant_id) && (payload.signup_ordinal === undefined || payload.signup_ordinal === null || (isCount(payload.signup_ordinal) && payload.signup_ordinal > 0)) ? { protocol: protocolId, type: "hello", request_id, payload: { participant_id: payload.participant_id, signup_ordinal: payload.signup_ordinal ?? null } } : null;
    case "users_listed": return listOf(isUser)(payload.users) ? { protocol: protocolId, type: "users_listed", request_id, payload: { users: payload.users } } : null;
    case "circle_users_listed": return isString(payload.circle_id) && listOf(isUser)(payload.users) ? { protocol: protocolId, type: "circle_users_listed", request_id, payload: { circle_id: payload.circle_id, users: payload.users } } : null;
    case "status_updated": return isUser(payload.profile) ? { protocol: protocolId, type: "status_updated", request_id, payload: { profile: payload.profile } } : null;
    case "direct_channel_opened": case "channel_created": return isChannelBase(payload.channel) ? { protocol: protocolId, type: value.type, request_id, payload: { channel: payload.channel } } : null;
    case "membership_joined": case "channel_member_added": case "read_marker_updated": return isMembership(payload.membership) ? { protocol: protocolId, type: value.type, request_id, payload: { membership: payload.membership } } : null;
    case "membership_left": case "subscription_ended": return isString(payload.channel_id) ? { protocol: protocolId, type: value.type, request_id, payload: { channel_id: payload.channel_id } } : null;
    case "channels_listed": return listOf(isChannel)(payload.channels) ? { protocol: protocolId, type: "channels_listed", request_id, payload: { channels: payload.channels } } : null;
    case "channel_users_listed": return isString(payload.channel_id) && listOf(isUser)(payload.users) ? { protocol: protocolId, type: "channel_users_listed", request_id, payload: { channel_id: payload.channel_id, users: payload.users } } : null;
    case "channel_description_updated": return isString(payload.channel_id) && isString(payload.description) ? { protocol: protocolId, type: "channel_description_updated", request_id, payload: { channel_id: payload.channel_id, description: payload.description } } : null;
    case "joinable_channels_listed": return listOf(isDiscoverable)(payload.channels) ? { protocol: protocolId, type: "joinable_channels_listed", request_id, payload: { channels: payload.channels } } : null;
    case "messages_loaded": return isString(payload.channel_id) && listOf(isMessage)(payload.messages) ? { protocol: protocolId, type: "messages_loaded", request_id, payload: { channel_id: payload.channel_id, messages: payload.messages.map(messageFromWire) } } : null;
    case "thread_loaded": return isString(payload.root_message_id) && listOf(isMessage)(payload.messages) ? { protocol: protocolId, type: "thread_loaded", request_id, payload: { root_message_id: payload.root_message_id, messages: payload.messages.map(messageFromWire) } } : null;
    case "thread_summaries_listed": return isString(payload.channel_id) && listOf(isSummary)(payload.summaries) ? { protocol: protocolId, type: "thread_summaries_listed", request_id, payload: { channel_id: payload.channel_id, summaries: payload.summaries } } : null;
    case "thread_read_updated": return isSummary(payload.summary) ? { protocol: protocolId, type: "thread_read_updated", request_id, payload: { summary: payload.summary } } : null;
    case "subscription_started": return isString(payload.channel_id) && listOf(isMessage)(payload.history) ? { protocol: protocolId, type: "subscription_started", request_id, payload: { channel_id: payload.channel_id, history: payload.history.map(messageFromWire) } } : null;
    case "message_accepted": case "message_edited": case "message_deleted": return isMessage(payload.message) ? { protocol: protocolId, type: value.type, request_id, payload: { message: messageFromWire(payload.message) } } : null;
    case "channel_reactions_listed": return isString(payload.channel_id) && listOf(isReaction)(payload.reactions) ? { protocol: protocolId, type: "channel_reactions_listed", request_id, payload: { channel_id: payload.channel_id, reactions: payload.reactions } } : null;
    case "message_reaction_changed": return isReactionChange(payload.change) ? { protocol: protocolId, type: "message_reaction_changed", request_id, payload: { change: payload.change } } : null;
    case "mentions_listed": return listOf(isMention)(payload.mentions) ? { protocol: protocolId, type: "mentions_listed", request_id, payload: { mentions: payload.mentions.map(mentionFromWire) } } : null;
    case "mention_read": return isString(payload.message_id) ? { protocol: protocolId, type: "mention_read", request_id, payload: { message_id: payload.message_id } } : null;
    case "task_created": case "task_updated": return isTask(payload.task) ? { protocol: protocolId, type: value.type, request_id, payload: { task: payload.task } } : null;
    case "tasks_listed": return listOf(isTask)(payload.tasks) ? { protocol: protocolId, type: "tasks_listed", request_id, payload: { tasks: payload.tasks } } : null;
    case "chat": return isChat(payload.event) ? { protocol: protocolId, type: "chat", request_id, payload: { event: chatFromWire(payload.event) } } : null;
    case "lagged": return isString(payload.channel_id) && isCount(payload.last_seen_sequence) && isCount(payload.latest_known_sequence) && isCount(payload.skipped) && isString(payload.hint) ? { protocol: protocolId, type: "lagged", request_id, payload: { channel_id: payload.channel_id, last_seen_sequence: payload.last_seen_sequence, latest_known_sequence: payload.latest_known_sequence, skipped: payload.skipped, hint: payload.hint } } : null;
    case "circle_created": return isCircle(payload.circle) ? { protocol: protocolId, type: "circle_created", request_id, payload: { circle: payload.circle } } : null;
    case "circles_listed": return listOf(isCirclePair)(payload.circles) ? { protocol: protocolId, type: "circles_listed", request_id, payload: { circles: payload.circles } } : null;
    case "circle_deleted": case "circle_left": return isString(payload.circle_id) ? { protocol: protocolId, type: value.type, request_id, payload: { circle_id: payload.circle_id } } : null;
    case "circle_invitation_created": return isIssuedCircleInvitation(payload.invitation) ? { protocol: protocolId, type: "circle_invitation_created", request_id, payload: { invitation: payload.invitation } } : null;
    case "circle_invitation_accepted": return isCircleMembership(payload.membership) ? { protocol: protocolId, type: "circle_invitation_accepted", request_id, payload: { membership: payload.membership } } : null;
    case "invitation_created": return isIssuedChatInvitation(payload.invitation) ? { protocol: protocolId, type: "invitation_created", request_id, payload: { invitation: payload.invitation } } : null;
    case "invitation_inspected": case "invitation_declined": return isString(payload.token) && isPreview(payload.invitation) ? { protocol: protocolId, type: value.type, request_id, payload: { token: payload.token, invitation: payload.invitation } } : null;
    case "invitation_accepted": return isString(payload.token) && isAcceptedChatInvitation(payload.invitation) ? { protocol: protocolId, type: "invitation_accepted", request_id, payload: { token: payload.token, invitation: payload.invitation } } : null;
    case "error": return isString(payload.code) && isString(payload.message) ? { protocol: protocolId, type: "error", request_id, payload: { code: payload.code, message: payload.message } } : null;
    default: return null;
  }
}
export function parseStoredStringMap(value: string | null): StoredStringMap { if (value === null) return {}; try { const parsed: unknown = JSON.parse(value); if (!isRecord(parsed)) return {}; const result: Record<string, string> = {}; for (const [key, entry] of Object.entries(parsed)) if (key.length <= 128 && isString(entry) && entry.length <= 128) result[key] = entry; return result; } catch { return {}; } }
export function mediaFromUpload(value: unknown): MediaObject | null { if (!isRecord(value) || !isRecord(value.media)) return null; const media = value.media; if (!isString(media.id) || !isString(media.original_filename) || !isString(media.content_type) || !isString(media.channel_id)) return null; return { id: media.id, name: media.original_filename, original_filename: media.original_filename, content_type: media.content_type, channel_id: media.channel_id, ...(isString(media.url) ? { url: media.url } : {}) }; }
