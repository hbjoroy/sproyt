import type { ServerEvent, ServerEventType } from "../types";

/** Correlates presentation requests with the existing socket owner. */
export function createCommunityRequests() {
  const pending = new Map<string, { type: ServerEventType; resolve: (event: ServerEvent) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  return {
    request<T extends ServerEventType>(type: T, send: () => string | null): Promise<Extract<ServerEvent, { type: T }>> {
      const id = send();
      if (!id) return Promise.reject(new Error("Sprøyt er ikkje tilkopla. Vent litt og prøv igjen."));
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          pending.delete(id);
          reject(new Error("Serveren har ikkje stadfesta handlinga. Last inn på nytt og kontroller resultatet før du prøver igjen."));
        }, 20_000);
        pending.set(id, { type, resolve: event => resolve(event as Extract<ServerEvent, { type: T }>), reject, timer });
      });
    },
    observe(event: ServerEvent) {
      const entry = event.request_id ? pending.get(event.request_id) : undefined;
      if (!entry || (event.type !== entry.type && event.type !== "error")) return;
      pending.delete(event.request_id!);
      clearTimeout(entry.timer);
      if (event.type === "error") entry.reject(new Error(event.payload.message));
      else entry.resolve(event);
    }
  };
}
