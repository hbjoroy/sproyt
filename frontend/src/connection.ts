import { asWireEvent, isRecord, protocolId, type ClientCommand, type ClientCommandArguments, type ClientCommandType, type ServerEvent } from "./types";
import { createOutbox, createRequestTracker } from "./outbox";

export interface ConnectionSocket { readonly readyState: number; send(data: string): void; close(code?: number, reason?: string): void; addEventListener(type: string, listener: (event: Event) => void): void; }
interface ConnectionCloseEvent extends Event { readonly code: number; readonly reason: string; }
interface SocketHandoff {
  previousSocket: ConnectionSocket;
  nextSocket: ConnectionSocket;
  timeoutId: number | null;
  ready: boolean;
  expectedChannelId: string | null;
  expectedGeneration: number;
  expectedSubscriptionRequestId: string | null;
  readySubscriptionEvent: Extract<ServerEvent, Readonly<{ type: "subscription_started" }>> | null;
}
interface ConnectionState {
  socket: ConnectionSocket | null;
  socketHandoff: SocketHandoff | null;
  subscribedChannelId: string | null;
  desiredChannelId: string | null;
  subscriptionGeneration: number;
  recoveryPromise: Promise<void> | null;
  reconnectTimer: number | null;
  reconnectAttempt: number;
  heartbeatTimer: number | null;
  livenessTimer: number | null;
  stableConnectionTimer: number | null;
}
export interface ConnectionSnapshot { readonly connected: boolean; readonly closing: boolean; readonly handoffActive: boolean; readonly subscribedChannelId: string | null; }
export interface ConnectionController {
  start(): void;
  connect(silent?: boolean, replaceCurrent?: boolean): void;
  snapshot(): ConnectionSnapshot;
  recover(operation: (snapshot: ConnectionSnapshot) => Promise<void>): Promise<void>;
  setSubscribedChannel(channelId: string): void;
  clearSubscribedChannel(expectedChannelId?: string): void;
  takeSubscribedChannel(): string | null;
  replaceAfterSessionRefresh(): void;
  /** Discard a potentially stale OPEN socket (not a refresh handoff). */
  recoverAfterResume(): void;
  scheduleReconnect(closeCode?: number, closeReason?: string): void;
  send<Type extends ClientCommandType>(type: Type, ...args: ClientCommandArguments<Type>): string | null;
  resend<Type extends ClientCommandType>(requestId: string, type: Type, ...args: ClientCommandArguments<Type>): string | null;
}
export interface ConnectionDependencies {
  readonly websocketUrl: () => string;
  readonly createSocket?: (url: string) => ConnectionSocket;
  readonly createRequestId: () => string;
  readonly onCommandSent: (requestId: string, command: ClientCommand) => void;
  readonly onBeforeConnect: () => void;
  readonly onOpen: (send: ConnectionController["send"]) => void;
  readonly onEvent: (event: ServerEvent) => void;
  readonly onUnsupportedProtocol: () => void;
  readonly onStatus: (connected: boolean, text: string) => void;
  readonly onConnected: () => void;
  readonly onDisconnected: () => void;
  readonly onSocketError: () => void;
  readonly onConnectionLost: () => void;
  readonly onRequestsLost: (requestIds: readonly string[]) => void;
  readonly onUncertainRequests?: (requestIds: readonly string[]) => void;
  readonly onAuthenticationFailure: () => Promise<void>;
  readonly recover: () => Promise<void>;
  readonly onHandoffFallback: () => void;
  readonly reportClientEvent?: (event: "resume_recovery" | "connect_timeout" | "liveness_timeout") => void;
  readonly now?: () => number;
  readonly isVisible?: () => boolean;
  readonly setTimeout: (callback: () => void, milliseconds: number) => number;
  readonly clearTimeout: (timer: number) => void;
  readonly setInterval: (callback: () => void, milliseconds: number) => number;
  readonly clearInterval: (timer: number) => void;
}

export interface TransientRequestState {
  readonly historyRequestIds: Readonly<{ clear(): void }>;
  readonly pendingCommands: Map<string, string>;
  readonly pendingInvitationResponses: Map<string, Readonly<{ token: string }>>;
  readonly pendingInvitationInspections: Map<string, string>;
  readonly pendingChannelInvitationRecipients: Map<string, string>;
  readonly pendingDirectInvitationMessages: Map<string, string>;
}

export function resetTransientRequestsAfterDisconnect(
  state: TransientRequestState,
  effects: Readonly<{
    setHistoryLoading(loading: boolean): void;
    failInspection(token: string): void;
    failInvitationResponse(token: string): void;
    failChannelInvitation(): void;
  }>
): void {
  state.historyRequestIds.clear();
  effects.setHistoryLoading(false);
  for (const token of new Set(state.pendingInvitationInspections.values())) effects.failInspection(token);
  for (const pending of state.pendingInvitationResponses.values()) effects.failInvitationResponse(pending.token);
  if (state.pendingChannelInvitationRecipients.size > 0 || state.pendingDirectInvitationMessages.size > 0) effects.failChannelInvitation();
  state.pendingCommands.clear();
  state.pendingInvitationResponses.clear();
  state.pendingInvitationInspections.clear();
  state.pendingChannelInvitationRecipients.clear();
  state.pendingDirectInvitationMessages.clear();
}

export function parseSocketEvent(data: unknown): ServerEvent | null { if (typeof data !== "string") return null; try { return asWireEvent(JSON.parse(data)); } catch { return null; } }
export function shouldForceResume(now: number, hiddenSince: number | null, forcedSignal: boolean): boolean {
  return forcedSignal || (hiddenSince !== null && now - hiddenSince >= 30_000);
}
export function hasUnsupportedProtocol(data: unknown): boolean { if (typeof data !== "string") return false; try { const value: unknown = JSON.parse(data); return isRecord(value) && typeof value.protocol === "string" && value.protocol !== protocolId; } catch { return false; } }
export const clientCommandTypes = [
  "hello", "list_users", "list_my_channels", "list_thread_summaries", "list_my_circles", "list_mentions", "list_tasks", "ping", "list_circle_users", "set_status", "open_direct_channel", "expand_direct_channel", "create_channel", "join_channel", "leave_channel", "list_channel_users", "update_channel_description", "list_joinable_channels", "add_channel_member", "load_recent_messages", "load_thread", "mark_thread_read", "subscribe_channel", "unsubscribe_channel", "send_message", "edit_message", "delete_message", "list_channel_reactions", "toggle_message_reaction", "mark_read", "mark_mention_read", "create_task", "set_task_done", "create_circle", "delete_circle", "leave_circle", "create_circle_invitation", "accept_circle_invitation", "create_invitation", "inspect_invitation", "decline_invitation", "accept_invitation"
 ] as const satisfies readonly ClientCommandType[];
const allClientCommandsListed: Exclude<ClientCommandType, typeof clientCommandTypes[number]> extends never ? true : false = true;
void allClientCommandsListed;
function isClientCommandType(value: unknown): value is ClientCommandType { return typeof value === "string" && clientCommandTypes.some((type) => type === value); }
const payloadlessClientCommands = new Set<ClientCommandType>(["hello", "list_users", "list_my_channels", "list_my_circles", "list_mentions", "list_tasks", "ping"]);
const isString = (value: unknown): value is string => typeof value === "string";
const isBoolean = (value: unknown): value is boolean => typeof value === "boolean";
const hasFields = (value: Record<string, unknown>, required: readonly string[], optional: readonly string[] = []): boolean => required.every((key) => key in value) && Object.keys(value).every((key) => required.includes(key) || optional.includes(key));
const strings = (value: Record<string, unknown>, fields: readonly string[]): boolean => fields.every((field) => isString(value[field]));
const channelIdPayload = (payload: Record<string, unknown>): boolean => hasFields(payload, ["channel_id"]) && isString(payload.channel_id);
export function isClientCommand(value: unknown): value is ClientCommand {
  if (!isRecord(value) || !isClientCommandType(value.type) || "payload" in value && !hasSafeOutboundNumbers(value.payload)) return false;
  if (!isRecord(value.payload)) return !("payload" in value) && payloadlessClientCommands.has(value.type);
  if (payloadlessClientCommands.has(value.type)) return false;
  if (value.type === "load_recent_messages") {
    const limit = value.payload.limit;
    if (limit !== undefined && limit !== null && (typeof limit !== "number" || !Number.isSafeInteger(limit) || limit < 0 || limit > 65_535)) return false;
    for (const field of [value.payload.after, value.payload.before]) if (field !== undefined && field !== null && (typeof field !== "number" || !Number.isSafeInteger(field) || field < 0)) return false;
  }
  if ((value.type === "mark_read" || value.type === "mark_thread_read") && (typeof value.payload.sequence !== "number" || !Number.isSafeInteger(value.payload.sequence) || value.payload.sequence < 0)) return false;
  const payload = value.payload;
  switch (value.type) {
    case "list_circle_users": case "list_joinable_channels": case "delete_circle": case "leave_circle": case "create_circle_invitation": return hasFields(payload, ["circle_id"]) && isString(payload.circle_id);
    case "set_status": return hasFields(payload, ["text", "emoji", "expires_at"]) && strings(payload, ["text", "emoji"]) && (payload.expires_at === null || isString(payload.expires_at));
    case "open_direct_channel": return hasFields(payload, ["user_id"]) && isString(payload.user_id);
    case "expand_direct_channel": return hasFields(payload, ["channel_id", "user_id"]) && strings(payload, ["channel_id", "user_id"]);
    case "create_channel": return hasFields(payload, ["slug", "name", "kind", "circle_id"]) && strings(payload, ["slug", "name"]) && (payload.kind === "public" || payload.kind === "local" || payload.kind === "private") && (payload.circle_id === null || isString(payload.circle_id));
    case "join_channel": return hasFields(payload, ["channel"]) && isRecord(payload.channel) && hasFields(payload.channel, ["type", "value"]) && (payload.channel.type === "id" || payload.channel.type === "slug") && isString(payload.channel.value);
    case "leave_channel": case "list_channel_users": case "list_thread_summaries": case "subscribe_channel": case "unsubscribe_channel": case "list_channel_reactions": return channelIdPayload(payload);
    case "update_channel_description": return hasFields(payload, ["channel_id", "description"]) && strings(payload, ["channel_id", "description"]);
    case "add_channel_member": return hasFields(payload, ["channel_id", "user_id"]) && strings(payload, ["channel_id", "user_id"]);
    case "load_recent_messages": return hasFields(payload, ["channel_id"], ["limit", "after", "before"]) && isString(payload.channel_id);
    case "load_thread": return hasFields(payload, ["root_message_id"]) && isString(payload.root_message_id);
    case "mark_thread_read": return hasFields(payload, ["root_message_id", "sequence"]) && isString(payload.root_message_id);
    case "send_message": return hasFields(payload, ["channel_id", "body"], ["parent_message_id"]) && strings(payload, ["channel_id", "body"]) && (payload.parent_message_id === undefined || payload.parent_message_id === null || isString(payload.parent_message_id));
    case "edit_message": return hasFields(payload, ["message_id", "body"]) && strings(payload, ["message_id", "body"]);
    case "delete_message": case "mark_mention_read": return hasFields(payload, ["message_id"]) && isString(payload.message_id);
    case "toggle_message_reaction": return hasFields(payload, ["message_id", "emoji"]) && strings(payload, ["message_id", "emoji"]);
    case "mark_read": return hasFields(payload, ["channel_id", "sequence"]) && isString(payload.channel_id);
    case "create_task": return hasFields(payload, ["source_message_id", "assignee_id", "title", "process_link_id"]) && strings(payload, ["source_message_id", "assignee_id", "title"]) && (payload.process_link_id === null || isString(payload.process_link_id));
    case "set_task_done": return hasFields(payload, ["task_id", "done"]) && isString(payload.task_id) && isBoolean(payload.done);
    case "create_circle": return hasFields(payload, ["slug", "name"]) && strings(payload, ["slug", "name"]);
    case "accept_circle_invitation": case "inspect_invitation": case "decline_invitation": case "accept_invitation": return hasFields(payload, ["token"]) && isString(payload.token);
    case "create_invitation": { const target = payload.target; return hasFields(payload, ["target"]) && isRecord(target) && (target.type === "circle" ? hasFields(target, ["type", "circle_id"]) && isString(target.circle_id) : target.type === "channel" && hasFields(target, ["type", "circle_id", "channel_id"]) && strings(target, ["circle_id", "channel_id"])); }
    default: return false;
  }
}
function hasMessageData(event: Event): event is Event & Readonly<{ data: unknown }> { return "data" in event; }
function isConnectionCloseEvent(event: Event): event is ConnectionCloseEvent { return "code" in event && typeof event.code === "number" && "reason" in event && typeof event.reason === "string"; }
function hasSafeOutboundNumbers(value: unknown): boolean {
  if (typeof value === "number") return Number.isSafeInteger(value);
  if (Array.isArray(value)) return value.every(hasSafeOutboundNumbers);
  return !isRecord(value) || Object.values(value).every(hasSafeOutboundNumbers);
}

export function createConnectionController(dependencies: ConnectionDependencies): ConnectionController {
  const state: ConnectionState = { socket: null, socketHandoff: null, subscribedChannelId: null, desiredChannelId: null, subscriptionGeneration: 0, recoveryPromise: null, reconnectTimer: null, reconnectAttempt: 0, heartbeatTimer: null, livenessTimer: null, stableConnectionTimer: null };
  const createSocket = dependencies.createSocket ?? ((url: string) => new WebSocket(url));
  const requestTracker = createRequestTracker(dependencies.createRequestId, protocolId);
  const outbox = createOutbox();
  const pendingBySocket = new Map<ConnectionSocket, Set<string>>();
  const connectTimeouts = new Map<ConnectionSocket, number>();
  const lastServerActivity = new WeakMap<ConnectionSocket, number>();
  const clearReconnect = (): void => { if (state.reconnectTimer !== null) dependencies.clearTimeout(state.reconnectTimer); state.reconnectTimer = null; };
  const clearHeartbeat = (): void => { if (state.heartbeatTimer !== null) dependencies.clearInterval(state.heartbeatTimer); state.heartbeatTimer = null; };
  const clearConnectTimeout = (socket: ConnectionSocket): void => { const timer = connectTimeouts.get(socket); if (timer !== undefined) dependencies.clearTimeout(timer); connectTimeouts.delete(socket); };
  const clearLiveness = (): void => { if (state.livenessTimer !== null) dependencies.clearInterval(state.livenessTimer); state.livenessTimer = null; };
  const clearStableTimer = (): void => { if (state.stableConnectionTimer !== null) dependencies.clearTimeout(state.stableConnectionTimer); state.stableConnectionTimer = null; };
  const now = dependencies.now ?? (() => Date.now());
  const isVisible = dependencies.isVisible ?? (() => true);
  const loseSocketRequests = (socket: ConnectionSocket, uncertain = false): void => { const requestIds = [...(pendingBySocket.get(socket) ?? [])]; pendingBySocket.delete(socket); if (requestIds.length > 0) (uncertain ? dependencies.onUncertainRequests ?? dependencies.onRequestsLost : dependencies.onRequestsLost)(requestIds); };
  const sendVia = <Type extends ClientCommandType>(socket: ConnectionSocket | null, type: Type, ...args: ClientCommandArguments<Type>): string | null => sendViaRequest(socket, null, type, ...args);
  const sendViaRequest = <Type extends ClientCommandType>(socket: ConnectionSocket | null, preservedRequestId: string | null, type: Type, ...args: ClientCommandArguments<Type>): string | null => {
    if (socket === null || socket.readyState !== WebSocket.OPEN) return null;
    const candidate: unknown = args.length === 0 ? { type } : { type, payload: args[0] };
    if (!isClientCommand(candidate)) return null;
    const envelope = requestTracker.register(candidate, preservedRequestId ?? undefined);
    if (!outbox.send(socket, envelope)) return null;
    if (candidate.type !== "ping") { const pending = pendingBySocket.get(socket) ?? new Set<string>(); pending.add(envelope.request_id); pendingBySocket.set(socket, pending); dependencies.onCommandSent(envelope.request_id, candidate); }
    return envelope.request_id;
  };
  const subscriptionChannelId = <Type extends ClientCommandType>(type: Type, args: ClientCommandArguments<Type>): string | null => {
    if (type !== "subscribe_channel" && type !== "unsubscribe_channel") return null;
    const payload: unknown = args[0];
    return isRecord(payload) && isString(payload.channel_id) ? payload.channel_id : null;
  };
  const updateDesiredSubscription = (type: ClientCommandType, channelId: string | null): boolean => {
    if (channelId === null) return false;
    const nextDesired = type === "subscribe_channel" ? channelId : state.desiredChannelId === channelId ? null : state.desiredChannelId;
    if (nextDesired === state.desiredChannelId) return false;
    state.desiredChannelId = nextDesired;
    state.subscriptionGeneration += 1;
    const handoff = state.socketHandoff;
    if (handoff !== null) {
      handoff.ready = false;
      handoff.expectedChannelId = nextDesired;
      handoff.expectedGeneration = state.subscriptionGeneration;
      handoff.expectedSubscriptionRequestId = null;
    }
    return true;
  };
  const rememberCandidateSubscription = (socket: ConnectionSocket, type: ClientCommandType, channelId: string | null, requestId: string | null): void => {
    const handoff = state.socketHandoff;
    if (handoff === null || handoff.nextSocket !== socket || type !== "subscribe_channel" || channelId === null || channelId !== state.desiredChannelId || requestId === null) return;
    handoff.ready = false;
    handoff.expectedChannelId = channelId;
    handoff.expectedGeneration = state.subscriptionGeneration;
    handoff.expectedSubscriptionRequestId = requestId;
  };
  const sendWithSubscription = <Type extends ClientCommandType>(socket: ConnectionSocket | null, type: Type, ...args: ClientCommandArguments<Type>): string | null => {
    const channelId = subscriptionChannelId(type, args);
    updateDesiredSubscription(type, channelId);
    const handoff = state.socketHandoff;
    // During session rotation the old socket stays subscribed until the
    // candidate has proved that it has the current desired channel. Routing a
    // channel change to the candidate makes timeout fallback unambiguous.
    const target = channelId !== null && handoff !== null && socket === state.socket ? handoff.nextSocket : socket;
    const requestId = sendVia(target, type, ...args);
    if (target !== null) rememberCandidateSubscription(target, type, channelId, requestId);
    return requestId;
  };
  const send = <Type extends ClientCommandType>(type: Type, ...args: ClientCommandArguments<Type>): string | null => sendWithSubscription(state.socket, type, ...args);
  const resend = <Type extends ClientCommandType>(requestId: string, type: Type, ...args: ClientCommandArguments<Type>): string | null => sendViaRequest(state.socket, requestId, type, ...args);
  const reconcileDesiredSubscription = (socket: ConnectionSocket): void => {
    if (socket.readyState === WebSocket.OPEN && state.desiredChannelId !== null) {
      sendVia(socket, "subscribe_channel", { channel_id: state.desiredChannelId });
    }
  };
  let controller: ConnectionController;
  const activateSocket = (socket: ConnectionSocket): void => {
    clearHeartbeat(); clearLiveness(); clearStableTimer();
    lastServerActivity.set(socket, now());
    dependencies.onConnected(); dependencies.onStatus(true, "Tilkopla");
    state.stableConnectionTimer = dependencies.setTimeout(() => { if (state.socket === socket && socket.readyState === WebSocket.OPEN) state.reconnectAttempt = 0; }, 10_000);
    state.heartbeatTimer = dependencies.setInterval(() => { if (state.socket === socket && isVisible()) sendVia(socket, "ping"); }, 10_000);
    state.livenessTimer = dependencies.setInterval(() => {
      if (state.socket !== socket || socket.readyState !== WebSocket.OPEN) return;
      if (!isVisible()) return;
      if (now() - (lastServerActivity.get(socket) ?? now()) <= 20_000) return;
      dependencies.reportClientEvent?.("liveness_timeout");
        loseSocketRequests(socket, true);
      state.socket = null; state.subscribedChannelId = null;
      clearHeartbeat(); clearLiveness(); clearStableTimer();
      socket.close(4003, "liveness timed out");
      dependencies.onDisconnected(); dependencies.onConnectionLost();
      controller.scheduleReconnect(1006, "sambandet svarar ikkje");
    }, 5_000);
  };
  const tryCommitHandoff = (): Extract<ServerEvent, Readonly<{ type: "subscription_started" }>> | null => {
    const handoff = state.socketHandoff;
    if (handoff === null || !handoff.ready || handoff.expectedChannelId !== state.desiredChannelId || handoff.expectedGeneration !== state.subscriptionGeneration || (pendingBySocket.get(handoff.previousSocket)?.size ?? 0) > 0) return null;
    if (handoff.timeoutId !== null) dependencies.clearTimeout(handoff.timeoutId);
    state.socketHandoff = null;
    state.socket = handoff.nextSocket;
    if (handoff.readySubscriptionEvent !== null) state.subscribedChannelId = handoff.readySubscriptionEvent.payload.channel_id;
    activateSocket(handoff.nextSocket);
    if (handoff.previousSocket.readyState === WebSocket.OPEN) handoff.previousSocket.close(4000, "session refreshed");
    return handoff.readySubscriptionEvent;
  };
  const connectSocket = (silent = false, previousSocket: ConnectionSocket | null = null): void => {
    clearReconnect();
    if (previousSocket === null) {
      const current = state.socket;
      if (current !== null && (current.readyState === WebSocket.OPEN || current.readyState === WebSocket.CONNECTING)) return;
      clearHeartbeat(); clearLiveness(); clearStableTimer();
    }
    dependencies.onBeforeConnect();
    const nextSocket = createSocket(dependencies.websocketUrl());
    if (previousSocket === null) { state.socket = nextSocket; state.subscribedChannelId = null; }
    if (previousSocket === null) {
      const timeout = dependencies.setTimeout(() => {
        if (state.socket !== nextSocket || nextSocket.readyState !== WebSocket.CONNECTING) return;
        connectTimeouts.delete(nextSocket);
        dependencies.reportClientEvent?.("connect_timeout");
        loseSocketRequests(nextSocket, true);
        nextSocket.close(4002, "connection timed out");
        if (state.socket === nextSocket) {
          state.socket = null;
          dependencies.onDisconnected();
          dependencies.onConnectionLost();
          controller.scheduleReconnect(1006, "sambandet tok for lang tid");
        }
      }, 12_000);
      connectTimeouts.set(nextSocket, timeout);
    } else {
      const handoff: SocketHandoff = { previousSocket, nextSocket, timeoutId: null, ready: false, expectedChannelId: state.desiredChannelId, expectedGeneration: state.subscriptionGeneration, expectedSubscriptionRequestId: null, readySubscriptionEvent: null };
      handoff.timeoutId = dependencies.setTimeout(() => {
        if (state.socketHandoff !== handoff) return;
        state.socketHandoff = null;
        if (nextSocket.readyState === WebSocket.CONNECTING) dependencies.reportClientEvent?.("connect_timeout");
        loseSocketRequests(nextSocket);
        nextSocket.close(4001, "session handoff timed out");
        if (previousSocket.readyState === WebSocket.OPEN) {
          dependencies.onStatus(true, "Tilkopla");
          reconcileDesiredSubscription(previousSocket);
          dependencies.onHandoffFallback();
          return;
        }
        loseSocketRequests(previousSocket, true);
        if (state.socket === previousSocket) state.socket = null;
        dependencies.onDisconnected(); dependencies.onConnectionLost();
        connectSocket(true);
      }, 12_000);
      state.socketHandoff = handoff;
    }
    if (!silent) dependencies.onStatus(false, "Koplar til ...");
    nextSocket.addEventListener("open", () => {
      clearConnectTimeout(nextSocket);
      if (previousSocket !== null && (state.socket !== previousSocket || state.socketHandoff?.nextSocket !== nextSocket)) { nextSocket.close(4000, "superseded session refresh"); return; }
      if (previousSocket === null && state.socket !== nextSocket) return;
      if (previousSocket === null) activateSocket(nextSocket);
      lastServerActivity.set(nextSocket, now());
      dependencies.onOpen((type, ...args) => sendWithSubscription(nextSocket, type, ...args));
    });
    nextSocket.addEventListener("message", (event) => {
      if ((state.socket !== nextSocket && state.socketHandoff?.nextSocket !== nextSocket) || !hasMessageData(event)) return;
      const serverEvent = parseSocketEvent(event.data);
      if (serverEvent === null) {
        if (hasUnsupportedProtocol(event.data)) dependencies.onUnsupportedProtocol();
        return;
      }
      lastServerActivity.set(nextSocket, now());
      if (serverEvent.request_id !== undefined) {
        const pending = pendingBySocket.get(nextSocket);
        pending?.delete(serverEvent.request_id);
        if (pending?.size === 0) pendingBySocket.delete(nextSocket);
      }
      const handoff = state.socketHandoff;
      const isCandidate = handoff?.nextSocket === nextSocket;
      if (serverEvent.type === "subscription_started") {
        if (isCandidate) {
          const matchesExpectedSubscription = serverEvent.payload.channel_id === handoff.expectedChannelId
            && handoff.expectedGeneration === state.subscriptionGeneration
            && serverEvent.request_id === handoff.expectedSubscriptionRequestId;
          if (!matchesExpectedSubscription) {
            sendVia(nextSocket, "unsubscribe_channel", { channel_id: serverEvent.payload.channel_id });
            return;
          }
          handoff.ready = true;
          handoff.readySubscriptionEvent = serverEvent;
          const committedSubscription = tryCommitHandoff();
          if (committedSubscription !== null) dependencies.onEvent(committedSubscription);
          return;
        } else if (serverEvent.payload.channel_id === state.desiredChannelId) {
          state.subscribedChannelId = serverEvent.payload.channel_id;
        } else {
          sendVia(nextSocket, "unsubscribe_channel", { channel_id: serverEvent.payload.channel_id });
          return;
        }
      } else if (isCandidate && serverEvent.type === "subscription_ended") {
        return;
      } else if (isCandidate && serverEvent.type === "hello" && handoff.expectedChannelId === null && handoff.expectedGeneration === state.subscriptionGeneration) {
        handoff.ready = true;
        const committedSubscription = tryCommitHandoff();
        if (committedSubscription !== null) dependencies.onEvent(committedSubscription);
        if (state.socket !== nextSocket) return;
      }
      const committedSubscription = tryCommitHandoff();
      if (committedSubscription !== null) dependencies.onEvent(committedSubscription);
      dependencies.onEvent(serverEvent);
    });
    nextSocket.addEventListener("close", (event) => {
      if (!isConnectionCloseEvent(event)) return;
      const closeEvent = event;
      clearConnectTimeout(nextSocket);
      if (state.socketHandoff?.previousSocket === nextSocket) {
        const handoff = state.socketHandoff;
        loseSocketRequests(nextSocket, true);
        if (handoff.nextSocket.readyState === WebSocket.OPEN || handoff.nextSocket.readyState === WebSocket.CONNECTING) {
          const committedSubscription = tryCommitHandoff();
          if (committedSubscription !== null) dependencies.onEvent(committedSubscription);
          return;
        }
        if (handoff.timeoutId !== null) dependencies.clearTimeout(handoff.timeoutId);
        state.socketHandoff = null;
        loseSocketRequests(handoff.nextSocket);
      }
      if (state.socketHandoff?.nextSocket === nextSocket) {
        const handoff = state.socketHandoff;
        if (handoff.timeoutId !== null) dependencies.clearTimeout(handoff.timeoutId);
        state.socketHandoff = null;
        if (handoff.previousSocket.readyState === WebSocket.OPEN) {
          dependencies.onStatus(true, "Tilkopla"); loseSocketRequests(nextSocket); reconcileDesiredSubscription(handoff.previousSocket); dependencies.onHandoffFallback(); return;
        }
        loseSocketRequests(handoff.previousSocket, true);
        if (state.socket === handoff.previousSocket) state.socket = nextSocket;
      }
      if (previousSocket !== null && state.socket === previousSocket && previousSocket.readyState === WebSocket.OPEN) { dependencies.onHandoffFallback(); return; }
      if (state.socket !== nextSocket) return;
      loseSocketRequests(nextSocket, true);
      dependencies.onDisconnected(); state.subscribedChannelId = null; dependencies.onConnectionLost(); clearHeartbeat(); clearLiveness(); clearStableTimer();
      if (closeEvent.code === 1008) dependencies.onAuthenticationFailure().catch(() => controller.scheduleReconnect(closeEvent.code, closeEvent.reason)); else controller.scheduleReconnect(closeEvent.code, closeEvent.reason);
    });
    nextSocket.addEventListener("error", () => { if ((previousSocket === null || state.socket !== previousSocket) && state.socket === nextSocket) { dependencies.onSocketError(); dependencies.onStatus(false, "Mista sambandet"); } });
  };
  controller = Object.freeze({
    start: (): void => connectSocket(),
    connect: (silent = false, replaceCurrent = false): void => connectSocket(silent, replaceCurrent && state.socket?.readyState === WebSocket.OPEN ? state.socket : null),
    snapshot: (): ConnectionSnapshot => Object.freeze({ connected: state.socket?.readyState === WebSocket.OPEN, closing: state.socket?.readyState === WebSocket.CLOSING, handoffActive: state.socketHandoff !== null, subscribedChannelId: state.subscribedChannelId }),
    recover: async (operation: (snapshot: ConnectionSnapshot) => Promise<void>): Promise<void> => { if (state.recoveryPromise !== null) return state.recoveryPromise; state.recoveryPromise = operation(controller.snapshot()); try { await state.recoveryPromise; } finally { state.recoveryPromise = null; } },
    setSubscribedChannel: (channelId: string): void => {
      state.subscribedChannelId = channelId;
      if (state.desiredChannelId !== channelId) {
        state.desiredChannelId = channelId;
        state.subscriptionGeneration += 1;
      }
    },
    clearSubscribedChannel: (expectedChannelId?: string): void => {
      if (expectedChannelId !== undefined && state.subscribedChannelId !== expectedChannelId) return;
      state.subscribedChannelId = null;
      if (expectedChannelId === undefined || state.desiredChannelId === expectedChannelId) {
        state.desiredChannelId = null;
        state.subscriptionGeneration += 1;
      }
    },
    takeSubscribedChannel: (): string | null => { const channelId = state.subscribedChannelId; state.subscribedChannelId = null; return channelId; },
    replaceAfterSessionRefresh: (): void => { const current = state.socket; if (state.socketHandoff !== null) return; if (current === null || current.readyState === WebSocket.CLOSED || current.readyState === WebSocket.CLOSING) connectSocket(true); else if (current.readyState === WebSocket.OPEN) connectSocket(true, current); },
    recoverAfterResume: (): void => {
      const current = state.socket;
      if (state.socketHandoff !== null) {
        const handoff = state.socketHandoff;
        if (handoff.timeoutId !== null) dependencies.clearTimeout(handoff.timeoutId);
        state.socketHandoff = null;
        loseSocketRequests(handoff.nextSocket);
        handoff.nextSocket.close(4004, "resume recovery");
      }
      dependencies.reportClientEvent?.("resume_recovery");
      clearReconnect(); clearHeartbeat(); clearLiveness(); clearStableTimer();
      state.socket = null; state.subscribedChannelId = null;
      if (current !== null && (current.readyState === WebSocket.OPEN || current.readyState === WebSocket.CONNECTING)) {
        clearConnectTimeout(current);
        loseSocketRequests(current, true);
        current.close(4004, "resume recovery");
      }
      dependencies.onDisconnected(); dependencies.onConnectionLost();
      connectSocket(true);
    },
    scheduleReconnect: (closeCode = 1006, closeReason = ""): void => { if (state.reconnectTimer !== null) return; state.reconnectAttempt += 1; const delay = Math.min(15_000, 500 * (2 ** Math.min(state.reconnectAttempt - 1, 5))); const detail = closeReason ? `kode ${closeCode}: ${closeReason}` : `kode ${closeCode}`; dependencies.onStatus(false, `Fråkopla (${detail}) – prøver igjen om ${Math.ceil(delay / 1000)} sekund`); state.reconnectTimer = dependencies.setTimeout(() => { state.reconnectTimer = null; dependencies.recover().catch(() => controller.scheduleReconnect(closeCode, closeReason)); }, delay); },
    send,
    resend
  });
  return controller;
}
