import { loadClientCoreWasm, type ClientCoreWasmLoader, type RawClientCoreInstance } from "./client-core-wasm";

export type RefreshOutcome = "rotated" | "retryable_failure" | "authentication_rejected";
export type CurrentSessionOutcome = "not_checked" | "valid" | "authentication_rejected" | "retryable_failure";
export type RefreshDisposition = "schedule_server_deadline" | "retry_shortly" | "retry_after_authentication_rejection";
export type RecoveryDecision = "session_rotated" | "wait_for_network" | "probe_current_session" | "use_current_session" | "wait_for_reauthentication" | "require_login";
export type StartDecision = "schedule_server_deadline" | "refresh_now" | "retry_shortly";

type RawSessionExports = Readonly<{
  sproyt_refresh_disposition(outcome: number): number;
  sproyt_recovery_decision(outcome: number, current: number, foreground: number, recent: number): number;
  sproyt_start_session_decision(probe: number): number;
}>;

export type SessionPolicy = Readonly<{
  refreshDisposition(outcome: RefreshOutcome): RefreshDisposition;
  recoveryDecision(outcome: RefreshOutcome, current: CurrentSessionOutcome, foreground: boolean, recent: boolean): RecoveryDecision;
  startDecision(probe: RefreshOutcome): StartDecision;
  ready: Promise<boolean>;
  usingWasm(): boolean;
}>;

const refreshAbi = (value: RefreshOutcome): number => value === "rotated" ? 0 : value === "authentication_rejected" ? 2 : 1;
const currentAbi = (value: CurrentSessionOutcome): number => value === "valid" ? 1 : value === "authentication_rejected" ? 2 : value === "retryable_failure" ? 3 : 0;

function fallbackRefresh(outcome: RefreshOutcome): RefreshDisposition {
  return outcome === "rotated" ? "schedule_server_deadline" : outcome === "authentication_rejected" ? "retry_after_authentication_rejection" : "retry_shortly";
}

function fallbackRecovery(outcome: RefreshOutcome, current: CurrentSessionOutcome, foreground: boolean, recent: boolean): RecoveryDecision {
  if (outcome === "rotated") return "session_rotated";
  if (outcome === "retryable_failure" || current === "retryable_failure") return "wait_for_network";
  if (current === "not_checked") return "probe_current_session";
  if (current === "valid") return "use_current_session";
  return foreground && recent ? "wait_for_reauthentication" : "require_login";
}

function rawExports(instance: RawClientCoreInstance): RawSessionExports | null {
  const value = instance.exports;
  return typeof value.sproyt_refresh_disposition === "function"
    && typeof value.sproyt_recovery_decision === "function"
    && typeof value.sproyt_start_session_decision === "function"
    ? value as RawSessionExports
    : null;
}

export function createSessionPolicy(loader: ClientCoreWasmLoader = loadClientCoreWasm): SessionPolicy {
  let active: RawSessionExports | null = null;
  const ready = loader().then((instance) => {
    active = rawExports(instance);
    return active !== null;
  }).catch(() => false);
  const disable = (): void => { active = null; };
  return {
    refreshDisposition(outcome) {
      if (!active) return fallbackRefresh(outcome);
      const result = active.sproyt_refresh_disposition(refreshAbi(outcome));
      if (result === 0) return "schedule_server_deadline";
      if (result === 1) return "retry_shortly";
      if (result === 2) return "retry_after_authentication_rejection";
      disable(); return fallbackRefresh(outcome);
    },
    recoveryDecision(outcome, current, foreground, recent) {
      if (!active) return fallbackRecovery(outcome, current, foreground, recent);
      const result = active.sproyt_recovery_decision(refreshAbi(outcome), currentAbi(current), Number(foreground), Number(recent));
      const decisions: RecoveryDecision[] = ["session_rotated", "wait_for_network", "probe_current_session", "use_current_session", "wait_for_reauthentication", "require_login"];
      const decision = decisions[result];
      if (decision) return decision;
      disable(); return fallbackRecovery(outcome, current, foreground, recent);
    },
    startDecision(probe) {
      if (!active) return probe === "rotated" ? "schedule_server_deadline" : probe === "authentication_rejected" ? "refresh_now" : "retry_shortly";
      const result = active.sproyt_start_session_decision(refreshAbi(probe));
      if (result === 0) return "schedule_server_deadline";
      if (result === 1) return "refresh_now";
      if (result === 2) return "retry_shortly";
      disable(); return probe === "rotated" ? "schedule_server_deadline" : probe === "authentication_rejected" ? "refresh_now" : "retry_shortly";
    },
    ready,
    usingWasm: () => active !== null
  };
}
