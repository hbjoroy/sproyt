import { asWireEvent, protocolId, type ServerEvent, type WireCommand } from "./types";
import type { ConnectionSocket } from "./connection";

/** SSE receives broadcasts; ordinary protocol commands use authenticated HTTP.
 * This adapter keeps the existing connection controller responsible for all
 * reconnects, request tracking and transport handoffs.
 */
export class SseSocket extends EventTarget implements ConnectionSocket {
  readyState: number = WebSocket.CONNECTING;
  private readonly eventsUrl: URL;
  private readonly commandsUrl: URL;
  private source: EventSource | null = null;
  private generation = 0;
  private opened = false;
  private activeChannel: string | null = null;
  private pendingSubscription: { channelId: string; requestId: string } | null = null;
  private readonly requests = new Set<AbortController>();
  private queue: Promise<void> = Promise.resolve();
  private lastHeartbeat = Date.now();
  private watchdog: number | null = null;
  private subscriptionTimer: number | null = null;
  private readonly lastSequence: Map<string, number>;

  constructor(websocketUrl: string, lastSequence: Map<string, number>) {
    super();
    const url = new URL(websocketUrl);
    url.protocol = url.protocol === "wss:" ? "https:" : "http:";
    url.pathname = "/api/v1/events";
    this.eventsUrl = url;
    this.commandsUrl = new URL(url);
    this.commandsUrl.pathname = "/api/v1/commands";
    this.lastSequence = lastSequence;
    this.openStream(null, null);
    this.watchdog = window.setInterval(() => {
      if (this.readyState === WebSocket.OPEN && Date.now() - this.lastHeartbeat > 40_000) this.fail();
    }, 5_000);
  }

  private emitFrame(data: string): void {
    const frame = (() => { try { return asWireEvent(JSON.parse(data)); } catch { return null; } })();
    if (frame !== null) this.rememberSequence(frame);
    this.dispatchEvent(new MessageEvent("message", { data }));
  }

  private rememberSequence(frame: ServerEvent): void {
    const message = frame.type === "chat" && frame.payload.event.type === "message_accepted"
      ? frame.payload.event.message
      : frame.type === "message_accepted" ? frame.payload.message : null;
    if (message !== null) this.lastSequence.set(message.channel_id, Math.max(this.lastSequence.get(message.channel_id) ?? 0, message.sequence));
    if (frame.type === "messages_loaded") {
      const last = frame.payload.messages.at(-1);
      if (last) this.lastSequence.set(frame.payload.channel_id, Math.max(this.lastSequence.get(frame.payload.channel_id) ?? 0, last.sequence));
    }
    if (frame.type === "subscription_started") {
      const last = frame.payload.history.at(-1);
      if (last) this.lastSequence.set(frame.payload.channel_id, Math.max(this.lastSequence.get(frame.payload.channel_id) ?? 0, last.sequence));
    }
  }

  private openStream(channelId: string | null, requestId: string | null): void {
    // Replacing an unacknowledged stream must also settle its request in the
    // connection controller; otherwise a later WebSocket handoff waits forever.
    if (this.pendingSubscription !== null) {
      const previous = this.pendingSubscription;
      this.pendingSubscription = null;
      this.emitFrame(JSON.stringify({ protocol: protocolId, request_id: previous.requestId,
        type: "subscription_ended", payload: { channel_id: previous.channelId } }));
    }
    this.generation += 1;
    const generation = this.generation;
    this.source?.close();
    this.source = null;
    if (this.subscriptionTimer !== null) window.clearTimeout(this.subscriptionTimer);
    this.subscriptionTimer = null;
    this.activeChannel = channelId;
    const url = new URL(this.eventsUrl);
    if (!this.opened && channelId === null) url.searchParams.set("bootstrap", "true");
    if (channelId !== null && requestId !== null) {
      this.pendingSubscription = { channelId, requestId };
      url.searchParams.set("channel_id", channelId);
      url.searchParams.set("request_id", requestId);
      const after = this.lastSequence.get(channelId);
      if (after !== undefined && after > 0) url.searchParams.set("after", String(after));
      this.subscriptionTimer = window.setTimeout(() => {
        if (generation === this.generation) this.fail();
      }, 15_000);
    }
    const source = new EventSource(url, { withCredentials: true });
    this.source = source;
    source.addEventListener("open", () => {
      if (generation !== this.generation || this.readyState === WebSocket.CLOSED) return;
      this.lastHeartbeat = Date.now();
      if (!this.opened) {
        this.opened = true;
        this.readyState = WebSocket.OPEN;
        this.dispatchEvent(new Event("open"));
      }
    });
    source.addEventListener("message", (event) => {
      if (generation !== this.generation || this.readyState !== WebSocket.OPEN || !(event instanceof MessageEvent)) return;
      this.lastHeartbeat = Date.now();
      const frame = (() => { try { return asWireEvent(JSON.parse(event.data)); } catch { return null; } })();
      if (frame?.type === "subscription_started" && frame.request_id === requestId) {
        this.pendingSubscription = null;
        if (this.subscriptionTimer !== null) window.clearTimeout(this.subscriptionTimer);
        this.subscriptionTimer = null;
      }
      this.emitFrame(event.data);
    });
    source.addEventListener("heartbeat", () => {
      if (generation === this.generation) this.lastHeartbeat = Date.now();
    });
    source.addEventListener("error", () => {
      if (generation === this.generation && this.readyState !== WebSocket.CLOSED) this.fail();
    });
  }

  private fail(code = 1006): void {
    if (this.readyState === WebSocket.CLOSED) return;
    this.dispatchEvent(new Event("error"));
    this.close(code, "event stream interrupted");
  }

  send(data: string): void {
    if (this.readyState !== WebSocket.OPEN) throw new Error("SSE transport is closed");
    const envelope: WireCommand = JSON.parse(data);
    this.queue = this.queue.then(async () => {
      if (this.readyState !== WebSocket.OPEN) return;
      if (envelope.type === "subscribe_channel" && envelope.payload && typeof envelope.payload.channel_id === "string") {
        this.openStream(envelope.payload.channel_id, envelope.request_id);
        return;
      }
      if (envelope.type === "unsubscribe_channel" && envelope.payload && typeof envelope.payload.channel_id === "string") {
        if (this.activeChannel === envelope.payload.channel_id) this.openStream(null, null);
        this.emitFrame(JSON.stringify({ protocol: envelope.protocol, request_id: envelope.request_id, type: "subscription_ended", payload: { channel_id: envelope.payload.channel_id } }));
        return;
      }
      const controller = new AbortController();
      this.requests.add(controller);
      const timeout = window.setTimeout(() => controller.abort(), 15_000);
      try {
        const response = await fetch(this.commandsUrl, {
          method: "POST", credentials: "same-origin", cache: "no-store", signal: controller.signal,
          headers: { "content-type": "application/json", "accept": "application/json" }, body: JSON.stringify(envelope)
        });
        if (!response.ok) { this.fail(response.status === 401 ? 1008 : 1006); return; }
        const body = await response.text();
        if (this.readyState !== WebSocket.OPEN) return;
        if (asWireEvent(JSON.parse(body)) === null) { this.fail(); return; }
        this.emitFrame(body);
      } catch {
        if (this.readyState === WebSocket.OPEN) this.fail();
      } finally {
        window.clearTimeout(timeout);
        this.requests.delete(controller);
      }
    }).catch(() => { if (this.readyState === WebSocket.OPEN) this.fail(); });
  }

  close(code = 1000, reason = "closed"): void {
    if (this.readyState === WebSocket.CLOSED) return;
    this.readyState = WebSocket.CLOSED;
    this.generation += 1;
    this.source?.close();
    this.source = null;
    if (this.subscriptionTimer !== null) window.clearTimeout(this.subscriptionTimer);
    if (this.watchdog !== null) window.clearInterval(this.watchdog);
    this.subscriptionTimer = null;
    this.watchdog = null;
    for (const request of this.requests) request.abort();
    this.requests.clear();
    this.dispatchEvent(new CloseEvent("close", { code, reason }));
  }
}

export function createSseSocketFactory(): (websocketUrl: string) => ConnectionSocket {
  const lastSequence = new Map<string, number>();
  return (websocketUrl) => new SseSocket(websocketUrl, lastSequence);
}
