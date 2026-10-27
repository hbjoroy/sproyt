import type { ConnectionSnapshot } from "./connection";
import type { ConnectionHealthState } from "./ui/react/connection-health";

/** Presentation follows transport/session state, never translated status text. */
export function connectionPresentation(connection: ConnectionSnapshot, options: {
  online: boolean; authenticationRequired: boolean; retrying: boolean; detail: string;
}): ConnectionHealthState {
  const phase = options.authenticationRequired ? "authentication" : !options.online ? "offline"
    : connection.connected ? "connected" : connection.connecting || options.retrying ? "connecting" : "offline";
  const label = phase === "authentication" ? "Logg inn på nytt for å halde fram."
    : phase === "offline" ? "Sambandet er brote. Nye meldingar kan mangle."
      : phase === "connecting" ? "Koplar til Sprøyt …" : "Tilkopla";
  return { phase, label, detail: options.detail, transport: connection.transport,
    retrying: options.retrying || Boolean(connection.connecting) };
}

/** One manual request joins the existing recovery path; it never opens a socket. */
export function createManualConnectionRecovery(options: {
  authenticationRequired(): boolean; recover(): Promise<void>; onPending(pending: boolean): void;
}): () => Promise<void> {
  let pending: Promise<void> | null = null;
  return () => {
    if (pending) return pending;
    if (options.authenticationRequired()) return Promise.resolve();
    options.onPending(true);
    pending = Promise.resolve().then(options.recover).finally(() => { pending = null; options.onPending(false); });
    return pending;
  };
}
