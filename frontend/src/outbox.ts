import type { ClientCommand, WireCommand } from "./types";

export interface SocketWritable { readonly readyState: number; send(data: string): void; }

export type RequestTracker = Readonly<{
  register(command: ClientCommand, requestId?: string): WireCommand;
}>;

export function createRequestTracker(
  createRequestId: () => string,
  protocol: WireCommand["protocol"]
): RequestTracker {
  return Object.freeze({
    register(command: ClientCommand, requestId?: string): WireCommand {
      const request_id = requestId ?? createRequestId();
      return "payload" in command
        ? { protocol, request_id, type: command.type, payload: command.payload }
        : { protocol, request_id, type: command.type };
    }
  });
}

export type Outbox = Readonly<{
  send(socket: SocketWritable | null, command: WireCommand): boolean;
}>;

export function createOutbox(): Outbox {
  return Object.freeze({
    send(socket: SocketWritable | null, command: WireCommand): boolean {
      if (socket === null || socket.readyState !== WebSocket.OPEN) return false;
      socket.send(JSON.stringify(command));
      return true;
    }
  });
}
