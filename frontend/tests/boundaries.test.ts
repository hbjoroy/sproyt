import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { AgentApi, HttpClient, HttpError, NotificationApi, ProcessApi, readJson, sameOriginJson } from "../src/api";
import { clientCommandTypes, createConnectionController, isClientCommand, parseSocketEvent, resetTransientRequestsAfterDisconnect, shouldForceResume, type ConnectionSocket } from "../src/connection";
import { NavigationController, restoreNavigation } from "../src/navigation";
import { asWireEvent, isRecord, mediaFromUpload, protocolId } from "../src/types";
import { createSessionController, fetchWithTimeout, parseSessionRefreshBroadcast, parseSessionRefreshLease, refreshDelayMilliseconds, sessionRefreshAfterSeconds } from "../src/session";

class MemoryStorage implements Storage {
  #values = new Map<string, string>();

  get length(): number { return this.#values.size; }
  clear(): void { this.#values.clear(); }
  getItem(key: string): string | null { return this.#values.get(key) ?? null; }
  key(index: number): string | null { return [...this.#values.keys()][index] ?? null; }
  removeItem(key: string): void { this.#values.delete(key); }
  setItem(key: string, value: string): void { this.#values.set(key, value); }
}

class FakeSocket implements ConnectionSocket {
  readyState = 0;
  readonly sent: string[] = [];
  readonly closed: Array<readonly [number | undefined, string | undefined]> = [];
  readonly #listeners = new Map<string, Array<(event: Event) => void>>();
  send(data: string): void { this.sent.push(data); }
  close(code?: number, reason?: string): void { this.closed.push([code, reason]); this.readyState = 3; }
  addEventListener(type: string, listener: (event: Event) => void): void {
    const listeners = this.#listeners.get(type) ?? [];
    listeners.push(listener);
    this.#listeners.set(type, listeners);
  }
  emit(type: string, event: Event): void { for (const listener of this.#listeners.get(type) ?? []) listener(event); }
}

test("WebSocket boundary rejects malformed envelopes without changing accepted data", () => {
  const valid = JSON.stringify({ protocol: protocolId, type: "chat", request_id: "request-1", payload: { event: { type: "message_accepted", message: { id: "m1", channel_id: "c1", sender_id: "u1", sender_display_name: "Ada", body: "hei", sequence: 1, sent_at: "2026-08-20T08:00:00Z" } } } });
  const accepted = parseSocketEvent(valid);
  assert.deepEqual(accepted, { protocol: protocolId, type: "chat", request_id: "request-1", payload: { event: { type: "message_accepted", message: { id: "m1", channel_id: "c1", sender_id: "u1", sender_display_name: "Ada", body: "hei", sequence: 1, sent_at: "2026-08-20T08:00:00Z", parent_message_id: null, edited_at: null, deleted_at: null } } } });

  const malformed: unknown[] = [
    null,
    "not JSON",
    JSON.stringify({ protocol: "sproyt.chat.v0", type: "chat", payload: {} }),
    JSON.stringify({ protocol: protocolId, type: "unknown_event", payload: {} }),
    JSON.stringify({ protocol: protocolId, type: "chat", request_id: 7, payload: {} }),
    JSON.stringify({ protocol: protocolId, type: "chat", payload: [] }),
    JSON.stringify({ protocol: protocolId, type: "chat" }),
    new Blob(["{}"])
  ];
  for (const envelope of malformed) assert.equal(parseSocketEvent(envelope), null);

  assert.deepEqual(parseSocketEvent(valid), accepted);
});

test("short focus changes do not force reconnect while real suspension and online recovery do", () => {
  assert.equal(shouldForceResume(10_000, 0, false), false);
  assert.equal(shouldForceResume(30_000, 0, false), true);
  assert.equal(shouldForceResume(1_000, null, false), false);
  assert.equal(shouldForceResume(1_000, null, true), true);
});

test("connection controller serializes typed commands and keeps malformed frames out of callbacks", () => {
  const socket = new FakeSocket();
  const events: unknown[] = [];
  const statuses: string[] = [];
  const timers: Array<() => void> = [];
  const trackedCommands: string[] = [];
  let request = 0;
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws",
    createSocket: () => socket,
    createRequestId: () => `request-${++request}`,
    onCommandSent: (_requestId, command) => trackedCommands.push(command.type), onBeforeConnect: () => {},
    onOpen: (send) => { send("hello"); send("load_recent_messages", { channel_id: "c1", limit: 20, after: 3 }); },
    onEvent: (event) => events.push(event), onUnsupportedProtocol: () => statuses.push("unsupported"),
    onStatus: (_connected, text) => statuses.push(text), onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => {}, onRequestsLost: () => {},
    onAuthenticationFailure: async () => {}, recover: async () => {}, onHandoffFallback: () => {},
    setTimeout: (callback) => { timers.push(callback); return timers.length; }, clearTimeout: () => {},
    setInterval: (callback) => { timers.push(callback); return timers.length; }, clearInterval: () => {}
  });
  controller.start();
  socket.readyState = WebSocket.OPEN;
  socket.emit("open", new Event("open"));
  assert.deepEqual(socket.sent.map((frame) => JSON.parse(frame)), [
    { protocol: protocolId, request_id: "request-1", type: "hello" },
    { protocol: protocolId, request_id: "request-2", type: "load_recent_messages", payload: { channel_id: "c1", limit: 20, after: 3 } }
  ]);
  assert.equal(controller.send("mark_read", { channel_id: "c1", sequence: 2 ** 53 }), null);
  assert.equal(controller.send("mark_read", { channel_id: "c1", sequence: -1 }), null);
  assert.equal(controller.send("load_recent_messages", { channel_id: "c1", limit: 65_536 }), null);
  assert.equal(controller.send("load_recent_messages", { channel_id: "c1", limit: 20, after: -1 }), null);
  const heartbeat = timers.at(-2);
  assert.notEqual(heartbeat, undefined);
  for (let tick = 0; tick < 10; tick += 1) heartbeat?.();
  assert.equal(socket.sent.filter((frame) => JSON.parse(frame).type === "ping").length, 10);
  assert.deepEqual(trackedCommands, ["hello", "load_recent_messages"]);
  socket.emit("message", new MessageEvent("message", { data: "{broken" }));
  socket.emit("message", new MessageEvent("message", { data: JSON.stringify({ protocol: "wrong", type: "hello" }) }));
  assert.deepEqual(events, []);
  assert.ok(statuses.includes("unsupported"));
});

test("socket handoff keeps active acknowledgements routed and settles candidate fallback", () => {
  const sockets: FakeSocket[] = [];
  const events: string[] = [];
  let lost = 0;
  let request = 0;
  const pendingUserRequests = new Set<string>();
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws", createSocket: () => { const socket = new FakeSocket(); sockets.push(socket); return socket; },
    createRequestId: () => `handoff-${++request}`, onCommandSent: (requestId, command) => { if (command.type === "send_message") pendingUserRequests.add(requestId); }, onBeforeConnect: () => {},
    onOpen: (send) => { send("hello"); send("subscribe_channel", { channel_id: "c1" }); }, onEvent: (event) => { events.push(event.type); if (event.request_id !== undefined) pendingUserRequests.delete(event.request_id); },
    onUnsupportedProtocol: () => {}, onStatus: () => {}, onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => { lost += 1; },
    onRequestsLost: (requestIds) => { for (const requestId of requestIds) pendingUserRequests.delete(requestId); }, onAuthenticationFailure: async () => {}, recover: async () => {}, onHandoffFallback: () => {}, setTimeout: () => 1, clearTimeout: () => {}, setInterval: () => 1, clearInterval: () => {}
  });
  controller.start();
  const active = sockets[0]; assert.notEqual(active, undefined); if (!active) return;
  active.readyState = WebSocket.OPEN; active.emit("open", new Event("open")); controller.setSubscribedChannel("c1");
  active.emit("message", new MessageEvent("message", { data: JSON.stringify({ protocol: protocolId, request_id: "handoff-1", type: "hello", payload: { participant_id: "u1" } }) }));
  active.emit("message", new MessageEvent("message", { data: JSON.stringify({ protocol: protocolId, request_id: "handoff-2", type: "subscription_started", payload: { channel_id: "c1", history: [] } }) }));
  assert.notEqual(controller.send("send_message", { channel_id: "c1", body: "main" }), null);
  controller.replaceAfterSessionRefresh();
  const candidate = sockets[1]; assert.notEqual(candidate, undefined); if (!candidate) return;
  candidate.readyState = WebSocket.OPEN; candidate.emit("open", new Event("open"));
  assert.equal(JSON.parse(candidate.sent[0] ?? "{}").type, "hello");
  candidate.emit("message", new MessageEvent("message", { data: JSON.stringify({ protocol: protocolId, request_id: "handoff-5", type: "subscription_started", payload: { channel_id: "c1", history: [] } }) }));
  assert.equal(active.readyState, WebSocket.OPEN);
  active.emit("message", new MessageEvent("message", { data: JSON.stringify({ protocol: protocolId, request_id: "handoff-3", type: "message_accepted", payload: { message: { id: "m1", channel_id: "c1", sender_id: "u1", sender_display_name: "Ada", body: "main", sequence: 1, sent_at: "2026-08-20T08:00:00Z" } } }) }));
  assert.ok(events.includes("message_accepted"));
  assert.equal(pendingUserRequests.size, 0);
  assert.equal(active.readyState, WebSocket.CLOSED);
  assert.notEqual(controller.send("send_message", { channel_id: "c1", parent_message_id: "m1", body: "thread" }), null);
  controller.replaceAfterSessionRefresh();
  const failedCandidate = sockets[2]; assert.notEqual(failedCandidate, undefined); if (!failedCandidate) return;
  failedCandidate.readyState = WebSocket.OPEN; failedCandidate.emit("open", new Event("open"));
  failedCandidate.emit("close", Object.assign(new Event("close"), { code: 1006, reason: "failed" }));
  assert.equal(lost, 0);
  assert.equal(pendingUserRequests.size, 1);
  candidate.emit("message", new MessageEvent("message", { data: JSON.stringify({ protocol: protocolId, request_id: "handoff-6", type: "message_accepted", payload: { message: { id: "m2", channel_id: "c1", parent_message_id: "m1", sender_id: "u1", sender_display_name: "Ada", body: "thread", sequence: 2, sent_at: "2026-08-20T08:01:00Z" } } }) }));
  assert.equal(pendingUserRequests.size, 0);
});

test("channel changes during a session handoff cannot commit the old candidate subscription and reconcile after candidate close", () => {
  const sockets: FakeSocket[] = [];
  const subscriptionEvents: string[] = [];
  let request = 0;
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws", createSocket: () => { const socket = new FakeSocket(); sockets.push(socket); return socket; },
    createRequestId: () => `switch-${++request}`, onCommandSent: () => {}, onBeforeConnect: () => {},
    onOpen: (send) => { send("hello"); send("subscribe_channel", { channel_id: "c1" }); }, onEvent: (event) => { if (event.type === "subscription_started") subscriptionEvents.push(event.payload.channel_id); }, onUnsupportedProtocol: () => {}, onStatus: () => {}, onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => {}, onRequestsLost: () => {}, onAuthenticationFailure: async () => {}, recover: async () => {}, onHandoffFallback: () => {}, setTimeout: () => 1, clearTimeout: () => {}, setInterval: () => 1, clearInterval: () => {}
  });
  const emit = (socket: FakeSocket, event: unknown): void => socket.emit("message", new MessageEvent("message", { data: JSON.stringify(event) }));
  const command = (socket: FakeSocket, type: string, channelId?: string): Readonly<{ request_id: string }> => {
    const parsed: unknown[] = socket.sent.map((frame) => JSON.parse(frame));
    const found = parsed.find((frame): frame is Record<string, unknown> => isRecord(frame)
      && frame.type === type
      && typeof frame.request_id === "string"
      && (channelId === undefined || (isRecord(frame.payload) && frame.payload.channel_id === channelId)));
    assert.notEqual(found, undefined);
    if (found === undefined) throw new Error(`missing ${type}`);
    if (typeof found.request_id !== "string") throw new Error(`missing request id for ${type}`);
    return { request_id: found.request_id };
  };
  const envelope = (requestId: string, type: string, payload: unknown): unknown => ({ protocol: protocolId, request_id: requestId, type, payload });

  controller.start();
  const active = sockets[0]; assert.notEqual(active, undefined); if (!active) return;
  active.readyState = WebSocket.OPEN; active.emit("open", new Event("open"));
  emit(active, envelope(command(active, "hello").request_id, "hello", { participant_id: "u1" }));
  emit(active, envelope(command(active, "subscribe_channel", "c1").request_id, "subscription_started", { channel_id: "c1", history: [] }));
  assert.equal(controller.snapshot().subscribedChannelId, "c1");
  subscriptionEvents.length = 0;

  controller.replaceAfterSessionRefresh();
  const candidate = sockets[1]; assert.notEqual(candidate, undefined); if (!candidate) return;
  candidate.readyState = WebSocket.OPEN; candidate.emit("open", new Event("open"));
  const oldCandidateSubscription = command(candidate, "subscribe_channel", "c1");

  assert.equal(controller.takeSubscribedChannel(), "c1");
  assert.notEqual(controller.send("unsubscribe_channel", { channel_id: "c1" }), null);
  assert.notEqual(controller.send("subscribe_channel", { channel_id: "c2" }), null);
  command(candidate, "subscribe_channel", "c2");
  assert.equal(active.sent.some((frame) => {
    const value: unknown = JSON.parse(frame);
    return isRecord(value) && isRecord(value.payload) && value.payload.channel_id === "c2";
  }), false);

  emit(candidate, envelope(oldCandidateSubscription.request_id, "subscription_started", { channel_id: "c1", history: [] }));
  assert.equal(active.readyState, WebSocket.OPEN);
  assert.equal(controller.snapshot().handoffActive, true);
  assert.ok(candidate.sent.some((frame) => {
    const value: unknown = JSON.parse(frame);
    return isRecord(value) && value.type === "unsubscribe_channel" && isRecord(value.payload) && value.payload.channel_id === "c1";
  }));

  candidate.emit("close", Object.assign(new Event("close"), { code: 1006, reason: "candidate failed" }));
  emit(active, envelope(command(active, "subscribe_channel", "c2").request_id, "subscription_started", { channel_id: "c2", history: [] }));
  assert.equal(controller.snapshot().subscribedChannelId, "c2");
  assert.equal(controller.snapshot().handoffActive, false);
  assert.equal(active.readyState, WebSocket.OPEN);
  assert.deepEqual(subscriptionEvents, ["c2"]);
});

test("candidate readiness does not replace the active subscription when handoff falls back", () => {
  const sockets: FakeSocket[] = [];
  const timers: Array<Readonly<{ milliseconds: number; callback: () => void }>> = [];
  let request = 0;
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws", createSocket: () => { const socket = new FakeSocket(); sockets.push(socket); return socket; }, createRequestId: () => `fallback-${++request}`,
    onCommandSent: () => {}, onBeforeConnect: () => {}, onOpen: (send) => { send("hello"); send("subscribe_channel", { channel_id: "c1" }); }, onEvent: () => {}, onUnsupportedProtocol: () => {}, onStatus: () => {}, onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => {}, onRequestsLost: () => {}, onAuthenticationFailure: async () => {}, recover: async () => {}, onHandoffFallback: () => {},
    setTimeout: (callback, milliseconds) => { timers.push({ callback, milliseconds }); return timers.length; }, clearTimeout: () => {}, setInterval: () => 1, clearInterval: () => {}
  });
  const event = (requestId: string, type: string, payload: unknown): Event => new MessageEvent("message", { data: JSON.stringify({ protocol: protocolId, request_id: requestId, type, payload }) });
  const requestId = (socket: FakeSocket, type: string, channelId?: string): string => {
    for (const frame of socket.sent) {
      const value: unknown = JSON.parse(frame);
      if (isRecord(value) && value.type === type && typeof value.request_id === "string" && (channelId === undefined || (isRecord(value.payload) && value.payload.channel_id === channelId))) return value.request_id;
    }
    throw new Error(`missing ${type}`);
  };
  controller.start();
  const active = sockets[0]; assert.notEqual(active, undefined); if (!active) return;
  active.readyState = WebSocket.OPEN; active.emit("open", new Event("open"));
  active.emit("message", event(requestId(active, "hello"), "hello", { participant_id: "u1" }));
  active.emit("message", event(requestId(active, "subscribe_channel", "c1"), "subscription_started", { channel_id: "c1", history: [] }));
  assert.notEqual(controller.send("send_message", { channel_id: "c1", body: "held back" }), null);
  controller.replaceAfterSessionRefresh();
  const candidate = sockets[1]; assert.notEqual(candidate, undefined); if (!candidate) return;
  candidate.readyState = WebSocket.OPEN; candidate.emit("open", new Event("open"));
  assert.equal(controller.takeSubscribedChannel(), "c1");
  controller.send("unsubscribe_channel", { channel_id: "c1" });
  controller.send("subscribe_channel", { channel_id: "c2" });
  candidate.emit("message", event(requestId(candidate, "subscribe_channel", "c2"), "subscription_started", { channel_id: "c2", history: [] }));
  assert.equal(controller.snapshot().subscribedChannelId, null);
  const timeout = timers.filter((timer) => timer.milliseconds === 12_000).at(-1);
  assert.notEqual(timeout, undefined);
  timeout?.callback();
  assert.equal(controller.snapshot().handoffActive, false);
  assert.equal(active.readyState, WebSocket.OPEN);
  active.emit("message", event(requestId(active, "subscribe_channel", "c2"), "subscription_started", { channel_id: "c2", history: [] }));
  assert.equal(controller.snapshot().subscribedChannelId, "c2");
});

test("clearing an inaccessible channel lets a refresh handoff commit on hello", () => {
  const sockets: FakeSocket[] = [];
  let request = 0;
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws", createSocket: () => { const socket = new FakeSocket(); sockets.push(socket); return socket; }, createRequestId: () => `clear-${++request}`,
    onCommandSent: () => {}, onBeforeConnect: () => {}, onOpen: (send) => { send("hello"); }, onEvent: () => {}, onUnsupportedProtocol: () => {}, onStatus: () => {}, onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => {}, onRequestsLost: () => {}, onAuthenticationFailure: async () => {}, recover: async () => {}, onHandoffFallback: () => {}, setTimeout: () => 1, clearTimeout: () => {}, setInterval: () => 1, clearInterval: () => {}
  });
  const hello = (socket: FakeSocket): string => {
    const value: unknown = JSON.parse(socket.sent[0] ?? "{}");
    if (!isRecord(value) || typeof value.request_id !== "string") throw new Error("missing hello request");
    return value.request_id;
  };
  const emitHello = (socket: FakeSocket): void => socket.emit("message", new MessageEvent("message", { data: JSON.stringify({ protocol: protocolId, request_id: hello(socket), type: "hello", payload: { participant_id: "u1" } }) }));
  controller.start();
  const active = sockets[0]; assert.notEqual(active, undefined); if (!active) return;
  active.readyState = WebSocket.OPEN; active.emit("open", new Event("open")); emitHello(active);
  controller.setSubscribedChannel("removed-channel");
  controller.clearSubscribedChannel();
  controller.replaceAfterSessionRefresh();
  const candidate = sockets[1]; assert.notEqual(candidate, undefined); if (!candidate) return;
  candidate.readyState = WebSocket.OPEN; candidate.emit("open", new Event("open")); emitHello(candidate);
  assert.equal(controller.snapshot().handoffActive, false);
  assert.equal(controller.snapshot().subscribedChannelId, null);
  assert.equal(active.readyState, WebSocket.CLOSED);
});

test("disconnect unlocks history and settles invitation requests before clearing correlation state", () => {
  const historyRequestIds = new Set(["history-1"]);
  const pendingCommands = new Map([["history-1", "load_recent_messages"]]);
  const pendingInvitationResponses = new Map([["response-1", { token: "invite-response", command: "accept_invitation" }]]);
  const pendingInvitationInspections = new Map([["inspect-1", "invite-inspect"]]);
  const pendingChannelInvitationRecipients = new Map([["channel-1", "user-1"]]);
  const pendingDirectInvitationMessages = new Map<string, string>();
  const effects: string[] = [];
  let historyLoading = true;
  resetTransientRequestsAfterDisconnect({ historyRequestIds, pendingCommands, pendingInvitationResponses, pendingInvitationInspections, pendingChannelInvitationRecipients, pendingDirectInvitationMessages }, {
    setHistoryLoading: (loading) => { historyLoading = loading; },
    failInspection: (token) => effects.push(`inspect:${token}`),
    failInvitationResponse: (token) => effects.push(`response:${token}`),
    failChannelInvitation: () => effects.push("channel")
  });
  assert.equal(historyLoading, false);
  assert.deepEqual(effects, ["inspect:invite-inspect", "response:invite-response", "channel"]);
  assert.equal(historyRequestIds.size, 0);
  assert.equal(pendingCommands.size, 0);
  assert.equal(pendingInvitationResponses.size, 0);
  assert.equal(pendingInvitationInspections.size, 0);
  assert.equal(pendingChannelInvitationRecipients.size, 0);
});

test("upload decoder accepts the Rust MediaObject wire shape without a name field", () => {
  assert.deepEqual(mediaFromUpload({ media: { id: "m1", original_filename: "bilete.png", content_type: "image/png", channel_id: "c1" } }), {
    id: "m1", name: "bilete.png", original_filename: "bilete.png", content_type: "image/png", channel_id: "c1"
  });
});

test("navigation storage treats malformed state as absent and keeps only bounded string mappings", () => {
  const storage = new MemoryStorage();
  storage.setItem("sproyt.active-channel.v1", "stored-channel");
  storage.setItem("sproyt.active-circle.v1", "stored-circle");
  storage.setItem("sproyt.active-channel-by-circle.v1", '{"circle":"channel","number":7,"tooLong":"' + "x".repeat(129) + '"}');
  const location = new URL("https://chat.example.test/?channel=linked-channel");
  const restored = restoreNavigation(storage, location);
  assert.deepEqual(restored, {
    activeChannelId: null,
    activeCircleId: null,
    activeRootScope: "shared",
    restoredChannelId: "linked-channel",
    restoredCircleId: "stored-circle",
    lastChannelByCircle: { circle: "channel" }
  });

  storage.setItem("sproyt.active-channel-by-circle.v1", "{broken");
  assert.deepEqual(restoreNavigation(storage, location).lastChannelByCircle, {});
  storage.setItem("sproyt.active-channel.v1", "x".repeat(129));
  storage.setItem("sproyt.active-circle.v1", "");
  const invalidIdentifiers = restoreNavigation(storage, new URL("https://chat.example.test/"));
  assert.equal(invalidIdentifiers.restoredChannelId, null);
  assert.equal(invalidIdentifiers.restoredCircleId, null);
});

test("temporarily opening an inbox preserves the channel restored after reload", () => {
  const storage = new MemoryStorage();
  const navigation = new NavigationController(storage, new URL("https://chat.example.test/"));
  navigation.setActiveChannel({ id: "direct-1", slug: "dm", name: "Ada", circle_id: null, direct_user_id: "ada" });
  navigation.deactivateChannel();
  assert.equal(navigation.activeChannelId, null);
  assert.equal(new NavigationController(storage, new URL("https://chat.example.test/")).restoredChannelId, "direct-1");
});

test("navigation controller keeps URL precedence, circle history and drafts outside the DOM", () => {
  const storage = new MemoryStorage();
  storage.setItem("sproyt.active-channel.v1", "stored-channel");
  storage.setItem("sproyt.active-circle.v1", "circle-a");
  const navigation = new NavigationController(storage, new URL("https://chat.example.test/?channel=linked-channel"));
  assert.equal(navigation.restoredChannelId, "linked-channel");
  assert.equal(navigation.restoreActiveCircle(["circle-b", "circle-a"], "circle-b"), "circle-a");
  const prat = { id: "prat-a", slug: "circlea-prat", name: "Prat", circle_id: "circle-a", direct_user_id: null };
  const other = { id: "other-a", slug: "circlea-anna", name: "Anna", circle_id: "circle-a", direct_user_id: null };
  navigation.setActiveChannel(other);
  assert.equal(navigation.preferredCircleChannel("circle-a", [prat, other])?.id, "other-a");
  navigation.forgetCircleChannel("circle-a");
  assert.equal(navigation.preferredCircleChannel("circle-a", [prat, other])?.id, "prat-a");
  navigation.persistChannelDraft("other-a", "utkast");
  navigation.persistThreadDraft("other-a", "root-a", "trådutkast");
  assert.equal(navigation.restoreChannelDraft("other-a"), "utkast");
  assert.equal(navigation.restoreThreadDraft("other-a", "root-a"), "trådutkast");
  navigation.clearThreadDraft("other-a", "root-a");
  assert.equal(navigation.restoreThreadDraft("other-a", "root-a"), "");
});

test("malformed API JSON rejects instead of being treated as state", async () => {
  await assert.rejects(readJson(new Response("{not-json")), SyntaxError);
  await assert.rejects(readJson(new Response('{"nested":{"bad":')),
    SyntaxError);

  const originalFetch = globalThis.fetch;
  try {
    globalThis.fetch = async () => new Response("not json", { status: 200 });
    await assert.rejects(sameOriginJson("/api/v1/example"), SyntaxError);
    globalThis.fetch = async () => new Response("request refused", { status: 422 });
    await assert.rejects(sameOriginJson("/api/v1/example"), /request refused/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("typed HTTP endpoints decode Rust-shaped responses and keep participant plus refresh outside UI", async () => {
  const calls: Array<Readonly<{ input: string; init: RequestInit | undefined }>> = [];
  const responses = [
    new Response("expired", { status: 401 }),
    new Response(JSON.stringify({ enabled: true, public_key: "key", subscriptions: 2, preferences: { mode: "instant", direct_messages: true, mentions: false } })),
    new Response(JSON.stringify({ process_link_id: "process-1" })),
    new Response(JSON.stringify({ process: { definition_name: "event-planning", status: "waiting" }, events: [{ event_type: "asked", actor_id: "agent-1", payload: { question: "when" } }] })),
    new Response(JSON.stringify({ agent_id: "agent-1", credential: "secret" })),
    new Response(null, { status: 204 }),
    new Response(null, { status: 204 })
  ];
  let refreshed = 0;
  const http = new HttpClient({
    fetch: async (input, init) => { calls.push({ input: String(input), init }); return responses.shift() ?? new Response("missing", { status: 500 }); },
    refreshSession: async () => { refreshed += 1; return true; }, participant: () => "participant-1"
  });
  const settings = await new NotificationApi(http).get();
  assert.equal(settings.preferences.directMessages, true);
  const processes = new ProcessApi(http);
  assert.equal(await processes.startEventPlanning({ channelId: "channel/1", requestId: "request-1", title: "Test" }), "process-1");
  assert.deepEqual(await processes.get("process/1"), { process: { definitionName: "event-planning", status: "waiting" }, events: [{ eventType: "asked", actorId: "agent-1", payload: { question: "when" } }] });
  const agents = new AgentApi(http);
  assert.deepEqual(await agents.create({ displayName: "Agent", provider: "test", serviceIdentity: "service-1", purpose: "test", rateLimitPerMinute: 1, expiresAt: "2026-08-20T09:00:00Z" }), { agentId: "agent-1", credential: "secret" });
  await agents.grant("agent/1", "channel/1", "read_history", "2026-08-20T09:00:00Z");
  await agents.revoke("agent/1");
  assert.equal(refreshed, 1);
  assert.equal(calls[0]?.input, "/api/v1/me/notifications?participant=participant-1");
  assert.equal(calls[1]?.input, "/api/v1/me/notifications?participant=participant-1");
  assert.match(calls[2]?.init?.body?.toString() ?? "", /channel\/1/);
  assert.match(calls[3]?.input ?? "", /process%2F1/);
  assert.match(calls[5]?.input ?? "", /agent%2F1/);
});

test("typed HTTP endpoints reject malformed and empty bodies before they reach UI", async () => {
  const malformed = new HttpClient({ fetch: async () => new Response(JSON.stringify({ enabled: true, preferences: { mode: "wrong" } })) });
  await assert.rejects(new NotificationApi(malformed).get(), /Ugyldige varselinnstillingar/);
  const empty = new HttpClient({ fetch: async () => new Response(null, { status: 200 }) });
  await assert.rejects(new ProcessApi(empty).get("process-1"), /Ugyldig prosess-svar/);
  const refused = new HttpClient({ fetch: async () => new Response("ikkje lov", { status: 403 }) });
  await assert.rejects(new AgentApi(refused).revoke("agent-1"), (error: unknown) => error instanceof HttpError && error.status === 403 && error.message === "ikkje lov");
});

test("invalid refresh delays fail closed to a short retry", () => {
  for (const value of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
    assert.equal(refreshDelayMilliseconds(value), 1_000);
  }
  assert.equal(refreshDelayMilliseconds(2.5), 2_500);
});

test("hung authentication requests are aborted within the shared recovery deadline", async () => {
  const timeouts: Array<() => void> = [];
  let cleared = false;
  const request = fetchWithTimeout((_input, init) => new Promise<Response>((_resolve, reject) => {
    init?.signal?.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")), { once: true });
  }), (callback) => { timeouts.push(callback); return 7; }, (timer) => { assert.equal(timer, 7); cleared = true; }, "/auth/session", { credentials: "same-origin" }, 8_000);
  assert.equal(timeouts.length, 1);
  timeouts[0]?.();
  await assert.rejects(request, { name: "AbortError" });
  assert.equal(cleared, true);
});

test("session and cross-tab decoders reject syntax-valid values of the wrong type", () => {
  for (const value of [null, {}, { refresh_after_seconds: "60" }, { refresh_after_seconds: 2.5 }, { refresh_after_seconds: 0 }, { refresh_after_seconds: 2 ** 53 }]) {
    assert.equal(sessionRefreshAfterSeconds(value), 300);
  }
  assert.equal(sessionRefreshAfterSeconds({ refresh_after_seconds: 60 }), 60);
  assert.equal(parseSessionRefreshLease('{"owner":"tab-a","expiresAt":"123"}'), null);
  assert.equal(parseSessionRefreshLease('{"owner":"tab-a","expiresAt":null}'), null);
  assert.deepEqual(parseSessionRefreshLease('{"owner":"tab-a","expiresAt":123}'), { owner: "tab-a", expiresAt: 123 });
  assert.equal(parseSessionRefreshBroadcast({ type: "session_refreshed", refreshAfterSeconds: "60" }), null);
  assert.equal(parseSessionRefreshBroadcast({ type: "session_rotated", refreshAfterSeconds: 2.5 }), null);
  assert.equal(parseSessionRefreshBroadcast({ type: "session_refreshed", refreshAfterSeconds: 60 }), null);
  assert.equal(parseSessionRefreshBroadcast({ type: "session_rotated", refreshAfterSeconds: 60 })?.refreshAfterSeconds, 60);
});

test("session controller owns refresh scheduling, lease competition and rotation callbacks", async () => {
  const storage = new MemoryStorage();
  const timers: Array<() => void> = [];
  const rotations: string[] = [];
  const events: string[] = [];
  const responses = [new Response(JSON.stringify({ refresh_after_seconds: 60 })), new Response(JSON.stringify({ refresh_after_seconds: 60 }))];
  const controller = createSessionController({
    fetch: async () => responses.shift() ?? new Response("missing", { status: 500 }), storage, broadcast: null,
    now: () => 1_000, setTimeout: (callback) => { timers.push(callback); return timers.length; }, clearTimeout: () => {}, withLock: null,
    visibility: () => "visible", isConnectionOpen: () => true, lastUserActivityAt: () => 1_000, onRefreshDueAt: () => {}, onStatus: () => {},
    onSessionRotated: () => rotations.push("rotated"), onReconnectNeeded: (reason) => events.push(reason), onLoginRequired: () => events.push("login"), onReauthenticationRequired: () => {},
    reportClientEvent: (event) => events.push(event), browserSessionId: "tab-a"
  });
  await controller.refresh();
  assert.equal(controller.snapshot().refreshDueAt, 61_000);
  assert.deepEqual(rotations, ["rotated"]);
  assert.ok(events.includes("session_refresh_succeeded"));
  assert.equal(storage.getItem("sproyt.session-refresh-lease.v1"), null);
  // refresh, verification and the next scheduled refresh each own a bounded timer.
  assert.equal(timers.length, 3);
});

test("resume recovery discards a stale OPEN socket, unlocks pending requests and fences late events", () => {
  const sockets: FakeSocket[] = [];
  const lost: string[][] = [];
  const events: string[] = [];
  const telemetry: string[] = [];
  let request = 0;
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws", createSocket: () => { const socket = new FakeSocket(); sockets.push(socket); return socket; }, createRequestId: () => `resume-${++request}`,
    onCommandSent: () => {}, onBeforeConnect: () => {}, onOpen: (send) => { send("hello"); }, onEvent: (event) => events.push(event.type), onUnsupportedProtocol: () => {}, onStatus: () => {}, onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => {}, onRequestsLost: (ids) => lost.push([...ids]), onAuthenticationFailure: async () => {}, recover: async () => {}, onHandoffFallback: () => {}, reportClientEvent: (event) => telemetry.push(event),
    setTimeout: () => 1, clearTimeout: () => {}, setInterval: () => 1, clearInterval: () => {}
  });
  controller.start();
  const stale = sockets[0]; assert.notEqual(stale, undefined); if (!stale) return;
  stale.readyState = WebSocket.OPEN; stale.emit("open", new Event("open"));
  const pendingBody = "bevar meg\n[[media:media-1|image/jpeg|foto.jpg]]";
  const pendingRequest = controller.send("send_message", { channel_id: "c1", body: pendingBody });
  const pendingThreadRequest = controller.send("send_message", { channel_id: "c1", parent_message_id: "root-1", body: "trådsvar\n[[media:media-2|image/png|tråd.png]]" });
  assert.notEqual(pendingRequest, null); assert.notEqual(pendingThreadRequest, null);
  controller.recoverAfterResume();
  assert.equal(stale.readyState, WebSocket.CLOSED);
  assert.ok(lost[0]?.includes(pendingRequest ?? ""));
  assert.ok(lost[0]?.includes(pendingThreadRequest ?? ""));
  assert.deepEqual(telemetry, ["resume_recovery"]);
  const fresh = sockets[1]; assert.notEqual(fresh, undefined); if (!fresh) return;
  stale.emit("message", new MessageEvent("message", { data: JSON.stringify({ protocol: protocolId, type: "hello", payload: { participant_id: "late" } }) }));
  assert.deepEqual(events, []);
  fresh.readyState = WebSocket.OPEN; fresh.emit("open", new Event("open"));
  assert.equal(JSON.parse(fresh.sent[0] ?? "{}").type, "hello");
  assert.equal(controller.resend(pendingRequest ?? "", "send_message", { channel_id: "c1", body: pendingBody }), pendingRequest);
  assert.equal(controller.resend(pendingThreadRequest ?? "", "send_message", { channel_id: "c1", parent_message_id: "root-1", body: "trådsvar\n[[media:media-2|image/png|tråd.png]]" }), pendingThreadRequest);
  const retried = fresh.sent.slice(-2).map((frame) => JSON.parse(frame));
  assert.deepEqual(retried.map((frame) => frame.request_id), [pendingRequest, pendingThreadRequest]);
  assert.equal(retried[0]?.payload.body, pendingBody);
});

test("liveness watchdog reconnects an apparently OPEN socket that stops receiving server events", () => {
  const sockets: FakeSocket[] = [];
  const intervals: Array<() => void> = [];
  const timeouts: Array<() => void> = [];
  const telemetry: string[] = [];
  let currentTime = 0;
  let visible = false;
  let recoveries = 0;
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws", createSocket: () => { const socket = new FakeSocket(); sockets.push(socket); return socket; }, createRequestId: () => crypto.randomUUID(),
    onCommandSent: () => {}, onBeforeConnect: () => {}, onOpen: () => {}, onEvent: () => {}, onUnsupportedProtocol: () => {}, onStatus: () => {}, onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => {}, onRequestsLost: () => {}, onAuthenticationFailure: async () => {}, recover: async () => { recoveries += 1; }, onHandoffFallback: () => {}, reportClientEvent: (event) => telemetry.push(event), now: () => currentTime, isVisible: () => visible,
    setTimeout: (callback) => { timeouts.push(callback); return timeouts.length; }, clearTimeout: () => {}, setInterval: (callback) => { intervals.push(callback); return intervals.length; }, clearInterval: () => {}
  });
  controller.start();
  const socket = sockets[0]; assert.notEqual(socket, undefined); if (!socket) return;
  socket.readyState = WebSocket.OPEN; socket.emit("open", new Event("open"));
  currentTime = 29_000;
  intervals[1]?.();
  assert.equal(socket.readyState, WebSocket.OPEN);
  assert.equal(telemetry.length, 0);
  visible = true;
  currentTime = 29_001;
  intervals[1]?.();
  assert.equal(socket.readyState, WebSocket.CLOSED);
  assert.ok(telemetry.includes("liveness_timeout"));
  assert.equal(sockets.length, 1);
  timeouts.at(-1)?.();
  assert.equal(recoveries, 1);
});

test("a refresh candidate that never opens times out once and repeated refresh requests do not create more candidates", () => {
  const sockets: FakeSocket[] = [];
  const timers = new Map<number, () => void>();
  const telemetry: string[] = [];
  let nextTimer = 0;
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws", createSocket: () => { const socket = new FakeSocket(); sockets.push(socket); return socket; }, createRequestId: () => crypto.randomUUID(),
    onCommandSent: () => {}, onBeforeConnect: () => {}, onOpen: () => {}, onEvent: () => {}, onUnsupportedProtocol: () => {}, onStatus: () => {}, onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => {}, onRequestsLost: () => {}, onAuthenticationFailure: async () => {}, recover: async () => {}, onHandoffFallback: () => {}, reportClientEvent: (event) => telemetry.push(event),
    setTimeout: (callback) => { const id = ++nextTimer; timers.set(id, callback); return id; }, clearTimeout: (id) => { timers.delete(id); }, setInterval: () => ++nextTimer, clearInterval: () => {}
  });
  controller.start();
  const active = sockets[0]; assert.notEqual(active, undefined); if (!active) return;
  active.readyState = WebSocket.OPEN; active.emit("open", new Event("open"));
  controller.replaceAfterSessionRefresh(); controller.replaceAfterSessionRefresh(); controller.replaceAfterSessionRefresh();
  assert.equal(sockets.length, 2);
  assert.equal(controller.snapshot().handoffActive, true);
  for (const callback of [...timers.values()]) callback();
  assert.equal(controller.snapshot().handoffActive, false);
  assert.equal(active.readyState, WebSocket.OPEN);
  assert.equal(sockets[1]?.readyState, WebSocket.CLOSED);
  assert.deepEqual(telemetry, ["connect_timeout"]);
});

test("an unexpected active close keeps send delivery uncertain and retries with the original request id", () => {
  const sockets: FakeSocket[] = [];
  const uncertain: string[] = [];
  let request = 0;
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws", createSocket: () => { const socket = new FakeSocket(); sockets.push(socket); return socket; }, createRequestId: () => `close-${++request}`,
    onCommandSent: () => {}, onBeforeConnect: () => {}, onOpen: () => {}, onEvent: () => {}, onUnsupportedProtocol: () => {}, onStatus: () => {}, onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => {}, onRequestsLost: () => assert.fail("active pending send must be uncertain"), onUncertainRequests: (ids) => uncertain.push(...ids), onAuthenticationFailure: async () => {}, recover: async () => {}, onHandoffFallback: () => {},
    setTimeout: () => 1, clearTimeout: () => {}, setInterval: () => 1, clearInterval: () => {}
  });
  controller.start();
  const first = sockets[0]; assert.notEqual(first, undefined); if (!first) return;
  first.readyState = WebSocket.OPEN; first.emit("open", new Event("open"));
  const requestId = controller.send("send_message", { channel_id: "original-channel", body: "same body" });
  assert.notEqual(requestId, null);
  first.readyState = WebSocket.CLOSED;
  first.emit("close", Object.assign(new Event("close"), { code: 1006, reason: "radio lost" }));
  assert.deepEqual(uncertain, [requestId]);
  controller.connect(true);
  const second = sockets[1]; assert.notEqual(second, undefined); if (!second) return;
  second.readyState = WebSocket.OPEN; second.emit("open", new Event("open"));
  controller.resend(requestId ?? "", "send_message", { channel_id: "original-channel", body: "same body" });
  const retried = JSON.parse(second.sent.at(-1) ?? "{}");
  assert.equal(retried.request_id, requestId);
  assert.equal(retried.payload.channel_id, "original-channel");
});

test("handoff timeout cannot strand a pending send when the previous socket is already closing", () => {
  const sockets: FakeSocket[] = [];
  const timers: Array<() => void> = [];
  const uncertain: string[] = [];
  let request = 0;
  let lost = 0;
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws", createSocket: () => { const socket = new FakeSocket(); sockets.push(socket); return socket; }, createRequestId: () => `race-${++request}`,
    onCommandSent: () => {}, onBeforeConnect: () => {}, onOpen: () => {}, onEvent: () => {}, onUnsupportedProtocol: () => {}, onStatus: () => {}, onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => { lost += 1; }, onRequestsLost: () => {}, onUncertainRequests: (ids) => uncertain.push(...ids), onAuthenticationFailure: async () => {}, recover: async () => {}, onHandoffFallback: () => {},
    setTimeout: (callback) => { timers.push(callback); return timers.length; }, clearTimeout: () => {}, setInterval: () => 1, clearInterval: () => {}
  });
  controller.start();
  const active = sockets[0]; assert.notEqual(active, undefined); if (!active) return;
  active.readyState = WebSocket.OPEN; active.emit("open", new Event("open"));
  const requestId = controller.send("send_message", { channel_id: "c1", body: "pending" });
  controller.replaceAfterSessionRefresh();
  active.readyState = WebSocket.CLOSING;
  timers.at(-1)?.();
  assert.ok(uncertain.includes(requestId ?? ""));
  assert.equal(lost, 1);
  assert.equal(controller.snapshot().handoffActive, false);
  assert.equal(sockets.length, 3);
});

test("closing both handoff sockets clears stale routing before reconnect", () => {
  const sockets: FakeSocket[] = [];
  const uncertain: string[] = [];
  let request = 0;
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws", createSocket: () => { const socket = new FakeSocket(); sockets.push(socket); return socket; }, createRequestId: () => `both-${++request}`,
    onCommandSent: () => {}, onBeforeConnect: () => {}, onOpen: () => {}, onEvent: () => {}, onUnsupportedProtocol: () => {}, onStatus: () => {}, onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => {}, onRequestsLost: () => {}, onUncertainRequests: (ids) => uncertain.push(...ids), onAuthenticationFailure: async () => {}, recover: async () => {}, onHandoffFallback: () => {},
    setTimeout: () => 1, clearTimeout: () => {}, setInterval: () => 1, clearInterval: () => {}
  });
  controller.start();
  const active = sockets[0]; assert.notEqual(active, undefined); if (!active) return;
  active.readyState = WebSocket.OPEN; active.emit("open", new Event("open"));
  const requestId = controller.send("send_message", { channel_id: "c1", body: "pending" });
  controller.replaceAfterSessionRefresh();
  const candidate = sockets[1]; assert.notEqual(candidate, undefined); if (!candidate) return;
  candidate.readyState = WebSocket.CLOSED;
  active.readyState = WebSocket.CLOSED;
  active.emit("close", Object.assign(new Event("close"), { code: 1006, reason: "both gone" }));
  assert.equal(controller.snapshot().handoffActive, false);
  assert.ok(uncertain.includes(requestId ?? ""));
  controller.connect(true);
  assert.equal(sockets.length, 3);
});

test("only the committed socket owns heartbeat and liveness intervals across rotations", () => {
  const sockets: FakeSocket[] = [];
  const intervals = new Set<number>();
  let timer = 0;
  let request = 0;
  const controller = createConnectionController({
    websocketUrl: () => "ws://chat.example.test/ws", createSocket: () => { const socket = new FakeSocket(); sockets.push(socket); return socket; }, createRequestId: () => `generation-${++request}`,
    onCommandSent: () => {}, onBeforeConnect: () => {}, onOpen: (send) => { send("hello"); }, onEvent: () => {}, onUnsupportedProtocol: () => {}, onStatus: () => {}, onConnected: () => {}, onDisconnected: () => {}, onSocketError: () => {}, onConnectionLost: () => {}, onRequestsLost: () => {}, onAuthenticationFailure: async () => {}, recover: async () => {}, onHandoffFallback: () => {},
    setTimeout: () => ++timer, clearTimeout: () => {}, setInterval: () => { const id = ++timer; intervals.add(id); return id; }, clearInterval: (id) => { intervals.delete(id); }
  });
  const helloRequest = (socket: FakeSocket): string => { const value: unknown = JSON.parse(socket.sent.find((frame) => JSON.parse(frame).type === "hello") ?? "{}"); if (!isRecord(value) || typeof value.request_id !== "string") throw new Error("missing hello"); return value.request_id; };
  const hello = (socket: FakeSocket): void => socket.emit("message", new MessageEvent("message", { data: JSON.stringify({ protocol: protocolId, request_id: helloRequest(socket), type: "hello", payload: { participant_id: "u1" } }) }));
  controller.start();
  let active = sockets[0]; assert.notEqual(active, undefined); if (!active) return;
  active.readyState = WebSocket.OPEN; active.emit("open", new Event("open")); hello(active);
  assert.equal(intervals.size, 2);
  for (let rotation = 0; rotation < 3; rotation += 1) {
    controller.replaceAfterSessionRefresh();
    const candidate = sockets.at(-1); assert.notEqual(candidate, undefined); if (!candidate) return;
    candidate.readyState = WebSocket.OPEN; candidate.emit("open", new Event("open"));
    assert.equal(intervals.size, 2);
    hello(candidate);
    assert.equal(controller.snapshot().handoffActive, false);
    assert.equal(intervals.size, 2);
    active = candidate;
  }
});

test("auth recovery rotates a refreshed connection exactly once and retries when a lock is busy", async () => {
  const timers: Array<() => void> = [];
  let rotations = 0;
  let sessionChecks = 0;
  const controller = createSessionController({
    fetch: async (input) => {
      if (input === "/auth/refresh") return new Response(JSON.stringify({ refresh_after_seconds: 60 }));
      sessionChecks += 1;
      return sessionChecks === 1 ? new Response("expired", { status: 401 }) : new Response(JSON.stringify({ refresh_after_seconds: 60 }));
    },
    storage: new MemoryStorage(), broadcast: null, now: () => 1_000, setTimeout: (callback) => { timers.push(callback); return timers.length; }, clearTimeout: () => {},
    withLock: async (_wait, operation) => operation(), visibility: () => "visible", isConnectionOpen: () => true, lastUserActivityAt: () => 1_000,
    onRefreshDueAt: () => {}, onStatus: () => {}, onSessionRotated: () => { rotations += 1; }, onReconnectNeeded: () => {}, onLoginRequired: () => {}, onReauthenticationRequired: () => {}, reportClientEvent: () => {}, browserSessionId: "tab-a"
  });
  await controller.recoverAuthentication();
  assert.equal(rotations, 1);
  let crossTabRotations = 0;
  const renewedByOtherTab = createSessionController({
    fetch: async () => new Response(JSON.stringify({ refresh_after_seconds: 90 })), storage: new MemoryStorage(), broadcast: null, now: () => 1_000,
    setTimeout: (callback) => { timers.push(callback); return timers.length; }, clearTimeout: () => {}, withLock: async (_wait, operation) => operation(),
    visibility: () => "visible", isConnectionOpen: () => false, lastUserActivityAt: () => 1_000, onRefreshDueAt: () => {}, onStatus: () => {},
    onSessionRotated: () => { crossTabRotations += 1; }, onReconnectNeeded: () => {}, onLoginRequired: () => {}, onReauthenticationRequired: () => {}, reportClientEvent: () => {}, browserSessionId: "tab-cross"
  });
  await renewedByOtherTab.recoverAuthentication();
  assert.equal(crossTabRotations, 1);
  const busy = createSessionController({
    fetch: async () => new Response("unused", { status: 500 }), storage: new MemoryStorage(), broadcast: null, now: () => 1_000, setTimeout: (callback) => { timers.push(callback); return timers.length; }, clearTimeout: () => {},
    withLock: async () => "busy", visibility: () => "hidden", isConnectionOpen: () => false, lastUserActivityAt: () => 0, onRefreshDueAt: () => {}, onStatus: () => {}, onSessionRotated: () => {}, onReconnectNeeded: () => {}, onLoginRequired: () => {}, onReauthenticationRequired: () => {}, reportClientEvent: () => {}, browserSessionId: "tab-b"
  });
  assert.equal(await busy.refresh(), false);
  assert.ok(timers.length >= 2);
});

test("session refresh does not publish stale connected status after the socket closes", async () => {
  let socketOpen = true;
  const statuses: string[] = [];
  const controller = createSessionController({
    fetch: async () => { socketOpen = false; return new Response("failed", { status: 503 }); }, storage: new MemoryStorage(), broadcast: null,
    now: () => 1_000, setTimeout: () => 1, clearTimeout: () => {}, withLock: null, visibility: () => "visible", isConnectionOpen: () => socketOpen,
    lastUserActivityAt: () => 1_000, onRefreshDueAt: () => {}, onStatus: (status) => statuses.push(status), onSessionRotated: () => {},
    onReconnectNeeded: () => {}, onLoginRequired: () => {}, onReauthenticationRequired: () => {}, reportClientEvent: () => {}, browserSessionId: "tab-status"
  });
  assert.equal(await controller.refresh(), false);
  assert.deepEqual(statuses, ["Fornyar økta …"]);
});

test("malformed refresh JSON fails closed and recent-user recovery is re-evaluated", async () => {
  const events: string[] = [];
  const timers: Array<() => void> = [];
  let now = 1_000;
  let refreshCalls = 0;
  const controller = createSessionController({
    fetch: async (input) => { if (input === "/auth/refresh") { refreshCalls += 1; return refreshCalls === 1 ? new Response("not-json") : new Response("expired", { status: 401 }); } return new Response("expired", { status: 401 }); },
    storage: new MemoryStorage(), broadcast: null, now: () => now, setTimeout: (callback) => { timers.push(callback); return timers.length; }, clearTimeout: () => {}, withLock: null,
    visibility: () => "visible", isConnectionOpen: () => true, lastUserActivityAt: () => 1_000, onRefreshDueAt: () => {}, onStatus: (status) => events.push(status),
    onSessionRotated: () => {}, onReconnectNeeded: () => {}, onLoginRequired: () => events.push("login"), onReauthenticationRequired: (required) => events.push(`reauth:${required}`), reportClientEvent: (event) => events.push(event), browserSessionId: "tab-invalid"
  });
  assert.equal(await controller.refresh(), false);
  assert.ok(events.includes("session_refresh_failed"));
  assert.ok(timers.length > 0);
  await controller.recoverAuthentication();
  assert.ok(events.includes("reauth:true"));
  controller.reauthenticateNow();
  assert.ok(events.includes("login"));
  const recoveryTimer = timers.at(-1);
  assert.notEqual(recoveryTimer, undefined);
  now = 122_000;
  recoveryTimer?.();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.ok(events.includes("login"));
});

test("Rust serde-shaped server frames decode while malformed nested values fail closed", () => {
  const channel = { id: "c1", slug: "prat", name: "Prat", kind: "public", circle_id: "circle-1", created_by: "u1" };
  const profile = { id: "u1", kind: "human", display_name: "Ada", external_provider: null, external_subject: null, created_at: "2026-08-20T08:00:00Z", status_text: "", status_emoji: "", status_expires_at: null };
  const message = { id: "m1", channel_id: "c1", sender_id: "u1", sender_display_name: "Ada", body: "Hei", sequence: 1, sent_at: "2026-08-20T08:00:00Z" };
  const fixtures: unknown[] = [
    { protocol: protocolId, type: "users_listed", payload: { users: [profile] } },
    { protocol: protocolId, type: "direct_channel_opened", payload: { channel } },
    { protocol: protocolId, type: "joinable_channels_listed", payload: { channels: [{ channel, description: "Open kanal" }] } },
    { protocol: protocolId, type: "messages_loaded", payload: { channel_id: "c1", messages: [message] } },
    { protocol: protocolId, type: "chat", payload: { event: { type: "message_reaction_changed", change: { message_id: "m1", channel_id: "c1", user_id: "u1", emoji: "👍", added: true, count: 1 } } } },
    { protocol: protocolId, type: "lagged", payload: { channel_id: "c1", last_seen_sequence: 1, latest_known_sequence: 4, skipped: 3, hint: "last inn att" } },
    { protocol: protocolId, type: "pong" }
  ];
  for (const frame of fixtures) assert.notEqual(asWireEvent(frame), null);

  const malformed: unknown[] = [
    { protocol: protocolId, type: "joinable_channels_listed", payload: { channels: [{ channel: { ...channel, created_by: 7 }, description: "Open" }] } },
    { protocol: protocolId, type: "joinable_channels_listed", payload: { channels: [{ channel, description: 7 }] } },
    { protocol: protocolId, type: "messages_loaded", payload: { channel_id: "c1", messages: [{ ...message, sequence: 2 ** 53 }] } },
    { protocol: protocolId, type: "lagged", payload: { channel_id: "c1", last_seen_sequence: 1, latest_known_sequence: 4, skipped: 2 ** 53, hint: "last inn att" } },
    { protocol: protocolId, type: "users_listed", payload: { users: [{ ...profile, status_expires_at: 1 }] } }
  ];
  for (const frame of malformed) assert.equal(asWireEvent(frame), null);
});

test("shared Rust serde golden fixture stays decodable by the browser boundary", () => {
  const fixturePath = fileURLToPath(new URL("./fixtures/rust-serde-joinable-channels-listed.json", import.meta.url));
  const fixture: unknown = JSON.parse(readFileSync(fixturePath, "utf8"));
  assert.notEqual(asWireEvent(fixture), null);
});

test("shared Rust client-command fixture covers exactly the TypeScript discriminant union", () => {
  const fixturePath = fileURLToPath(new URL("./fixtures/rust-serde-client-commands.json", import.meta.url));
  const parsed: unknown = JSON.parse(readFileSync(fixturePath, "utf8"));
  assert.ok(Array.isArray(parsed));
  const variants = new Set<string>();
  for (const frame of parsed) {
    assert.ok(isRecord(frame));
    assert.equal(frame.protocol, protocolId);
    assert.equal(typeof frame.request_id, "string");
    const candidate: unknown = "payload" in frame ? { type: frame.type, payload: frame.payload } : { type: frame.type };
    assert.equal(isClientCommand(candidate), true, `Rust command ${String(frame.type)} must pass the browser boundary`);
    if (typeof frame.type === "string") variants.add(frame.type);
  }
  assert.deepEqual([...variants].sort(), [...clientCommandTypes].sort());
  assert.equal(isClientCommand({ type: "add_channel_member", payload: { channel_id: "c1", participant_id: "u1" } }), false);
  assert.equal(isClientCommand({ type: "send_message", payload: { channel_id: "c1", body: "hei", unexpected: true } }), false);
  assert.equal(isClientCommand({ type: "set_task_done", payload: { task_id: "t1", done: "yes" } }), false);
  assert.equal(isClientCommand({ type: "hello", payload: {} }), false);
});
