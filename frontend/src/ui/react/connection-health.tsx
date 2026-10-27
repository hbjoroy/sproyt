import { Button } from "@sproyt/ui/react";
import { useState } from "react";
import connectionCss from "../connection-health.css";

export type ConnectionHealthState = {
  phase: "connected" | "connecting" | "offline" | "authentication";
  label: string;
  detail: string;
  transport: "websocket" | "sse" | null;
  retrying: boolean;
};

export function ConnectionHealth({ state, onRetry, onLogin, onReload, details = false }: {
  state: ConnectionHealthState;
  onRetry: () => Promise<void>;
  onLogin: () => void;
  onReload: () => void;
  details?: boolean;
}) {
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  if (!details && state.phase === "connected") return null;
  const retry = async () => {
    if (state.retrying || pending) return;
    setError(""); setPending(true);
    try { await onRetry(); }
    catch (cause) { setError(cause instanceof Error ? cause.message : "Kunne ikkje kople til att. Prøv igjen."); }
    finally { setPending(false); }
  };
  const transport = state.transport === "sse" ? "Transport: strauming (SSE)." : state.transport === "websocket" ? "Transport: WebSocket." : "Ingen aktiv transport.";
  return <div className={details ? "sp-connection-health sp-connection-health-details" : "sp-connection-health"}>
    <style>{connectionCss}</style>
    <div className="sp-connection-health-copy" role="status">
      <strong>{state.label}</strong>
      {state.phase === "authentication" && <p>Utkasta dine blir lagra først når du går vidare til innlogging.</p>}
      {details && <><p>{state.detail}</p><p>{transport}</p></>}
    </div>
    <div className="sp-connection-health-actions">
      {state.phase === "authentication" ? <Button variant="quiet" onClick={onLogin}>Logg inn på nytt</Button>
        : (details || state.phase !== "connected") && <Button variant="quiet" disabled={state.retrying || pending} busy={state.retrying || pending} onClick={() => void retry()}>Kopla til att</Button>}
      {details && <Button variant="quiet" onClick={onReload}>Last inn på nytt</Button>}
    </div>
    {error && <p className="sp-connection-health-error" role="alert">{error}</p>}
  </div>;
}

/** The mark supplements the logo; it is not a control or a colour-only signal. */
export function ConnectionLogoMark({ phase }: { phase: ConnectionHealthState["phase"] }) {
  if (phase === "connected") return null;
  const label = phase === "connecting" ? "Koplar til" : phase === "authentication" ? "Innlogging krevst" : "Fråkopla";
  return <svg className="sp-connection-logo-mark" data-phase={phase} viewBox="0 0 16 16" role="img" aria-label={label}
    fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    {phase === "connecting" ? <><circle cx="8" cy="8" r="5" strokeDasharray="2 2" /><path d="M8 5v3l2 1" /></>
      : phase === "authentication" ? <><rect x="3.5" y="7" width="9" height="6.5" rx="1" /><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2M8 9.5v1.5" /></>
      : <><path d="M8 2 14 13H2L8 2Z" /><path d="M8 6v3M8 11h.01" /></>}
  </svg>;
}
