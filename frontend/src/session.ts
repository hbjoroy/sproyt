import { isRecord } from "./types";

export function refreshDelayMilliseconds(seconds: number): number { return Number.isFinite(seconds) && seconds > 0 ? seconds * 1_000 : 1_000; }
export function sessionRefreshAfterSeconds(value: unknown): number { if (!isRecord(value)) return 300; const seconds = value.refresh_after_seconds; return typeof seconds === "number" && Number.isSafeInteger(seconds) && seconds > 0 ? seconds : 300; }
export type SessionRefreshLease = Readonly<{ owner: string; expiresAt: number }>;
export function parseSessionRefreshLease(value: string | null): SessionRefreshLease | null { if (value === null) return null; try { const parsed: unknown = JSON.parse(value); const expiresAt = isRecord(parsed) ? parsed.expiresAt : undefined; return isRecord(parsed) && typeof parsed.owner === "string" && typeof expiresAt === "number" && Number.isFinite(expiresAt) ? { owner: parsed.owner, expiresAt } : null; } catch { return null; } }
export type SessionRefreshBroadcast = Readonly<{ type: "session_rotated"; refreshAfterSeconds: number }>;
export function parseSessionRefreshBroadcast(value: unknown): SessionRefreshBroadcast | null { if (!isRecord(value) || value.type !== "session_rotated") return null; const seconds = value.refreshAfterSeconds; return typeof seconds === "number" && Number.isSafeInteger(seconds) && seconds > 0 ? { type: "session_rotated", refreshAfterSeconds: seconds } : null; }

interface SessionState { refreshTimer: number | null; refreshDueAt: number; refreshPromise: Promise<boolean> | null; refreshRejected: boolean; authenticationRecoveryPromise: Promise<void> | null; }
export interface SessionSnapshot { readonly refreshDueAt: number; readonly refreshRejected: boolean; readonly refreshing: boolean; readonly recoveringAuthentication: boolean; }
export interface SessionStorage { getItem(key: string): string | null; setItem(key: string, value: string): void; removeItem(key: string): void; }
export interface SessionBroadcast { postMessage(message: SessionRefreshBroadcast): void; addEventListener(type: "message", listener: (event: Readonly<{ data: unknown }>) => void): void; }
export interface SessionController { snapshot(): SessionSnapshot; start(): Promise<void>; schedule(seconds: number): void; refresh(waitForLock?: boolean): Promise<boolean>; recoverAuthentication(): Promise<void>; reauthenticateNow(): void; useCurrentSession(): Promise<boolean>; }
export interface SessionDependencies {
  readonly fetch: (input: string, init?: RequestInit) => Promise<Response>; readonly storage: SessionStorage; readonly broadcast: SessionBroadcast | null;
  readonly now: () => number; readonly setTimeout: (callback: () => void, milliseconds: number) => number; readonly clearTimeout: (timer: number) => void;
  readonly withLock: ((wait: boolean, operation: () => Promise<boolean>) => Promise<boolean | "busy">) | null; readonly visibility: () => "visible" | "hidden";
  readonly isConnectionOpen: () => boolean; readonly lastUserActivityAt: () => number; readonly onRefreshDueAt: (dueAt: number) => void;
  readonly onStatus: (status: string) => void; readonly onSessionRotated: () => void; readonly onReconnectNeeded: (reason: string) => void;
  readonly onLoginRequired: () => void; readonly onReauthenticationRequired: (required: boolean) => void; readonly reportClientEvent: (event: "session_refresh_failed" | "session_refresh_succeeded") => void;
  readonly browserSessionId: string; readonly leaseKey?: string;
}
const sessionHeaders = { accept: "application/json" };
const sessionRequest: RequestInit = { credentials: "same-origin", cache: "no-store", headers: sessionHeaders };

export function createSessionController(dependencies: SessionDependencies): SessionController {
  const state: SessionState = { refreshTimer: null, refreshDueAt: 0, refreshPromise: null, refreshRejected: false, authenticationRecoveryPromise: null };
  const leaseKey = dependencies.leaseKey ?? "sproyt.session-refresh-lease.v1";
  const schedule = (seconds: number): void => { if (state.refreshTimer !== null) dependencies.clearTimeout(state.refreshTimer); const delay = refreshDelayMilliseconds(seconds); state.refreshDueAt = dependencies.now() + delay; dependencies.onRefreshDueAt(state.refreshDueAt); state.refreshTimer = dependencies.setTimeout(() => { refresh().catch(() => schedule(30)); }, delay); };
  const scheduleAuthenticationRecovery = (seconds: number): void => { if (state.refreshTimer !== null) dependencies.clearTimeout(state.refreshTimer); const delay = refreshDelayMilliseconds(seconds); state.refreshDueAt = dependencies.now() + delay; dependencies.onRefreshDueAt(state.refreshDueAt); state.refreshTimer = dependencies.setTimeout(() => { recoverAuthentication().catch(() => scheduleAuthenticationRecovery(30)); }, delay); };
  const restoreStatus = (): void => { if (dependencies.visibility() === "visible" && dependencies.isConnectionOpen()) dependencies.onStatus("Tilkopla"); };
  const performRefresh = async (): Promise<boolean> => {
    const visible = dependencies.visibility() === "visible" && dependencies.isConnectionOpen(); if (visible) dependencies.onStatus("Fornyar økta …");
    let response: Response; try { response = await dependencies.fetch("/auth/refresh", { method: "POST", credentials: "same-origin", headers: sessionHeaders }); } catch { dependencies.reportClientEvent("session_refresh_failed"); state.refreshRejected = false; schedule(30); restoreStatus(); return false; }
    if (!response.ok) { dependencies.reportClientEvent("session_refresh_failed"); state.refreshRejected = response.status === 401; schedule(30); restoreStatus(); return false; }
    state.refreshRejected = false;
    let seconds: number; try { seconds = sessionRefreshAfterSeconds(await response.json()); } catch { dependencies.reportClientEvent("session_refresh_failed"); schedule(30); restoreStatus(); return false; }
    let verification: Response; try { verification = await dependencies.fetch("/auth/session", sessionRequest); } catch { dependencies.reportClientEvent("session_refresh_failed"); schedule(30); restoreStatus(); return false; }
    if (!verification.ok) { dependencies.reportClientEvent("session_refresh_failed"); state.refreshRejected = verification.status === 401; schedule(30); restoreStatus(); return false; }
    schedule(seconds); dependencies.onReauthenticationRequired(false); dependencies.broadcast?.postMessage({ type: "session_rotated", refreshAfterSeconds: seconds }); dependencies.reportClientEvent("session_refresh_succeeded"); dependencies.onSessionRotated(); return true;
  };
  const withLease = async (): Promise<boolean> => {
    const now = dependencies.now(); const lease: SessionRefreshLease = { owner: dependencies.browserSessionId, expiresAt: now + 15_000 };
    try { const current = parseSessionRefreshLease(dependencies.storage.getItem(leaseKey)); if (current !== null && current.owner !== lease.owner && current.expiresAt > now) { schedule(Math.max(2, Math.ceil((current.expiresAt - now) / 1_000))); return false; } dependencies.storage.setItem(leaseKey, JSON.stringify(lease)); if (parseSessionRefreshLease(dependencies.storage.getItem(leaseKey))?.owner !== lease.owner) { schedule(5); return false; } } catch { try { dependencies.storage.removeItem(leaseKey); } catch {} return performRefresh(); }
    try { return await performRefresh(); } finally { try { if (parseSessionRefreshLease(dependencies.storage.getItem(leaseKey))?.owner === lease.owner) dependencies.storage.removeItem(leaseKey); } catch {} }
  };
  const useCurrentSession = async (): Promise<boolean> => { try { const response = await dependencies.fetch("/auth/session", sessionRequest); if (!response.ok) return false; schedule(sessionRefreshAfterSeconds(await response.json())); return true; } catch { return false; } };
  const refresh = async (waitForLock = false): Promise<boolean> => { if (state.refreshPromise !== null) return state.refreshPromise; state.refreshPromise = (async () => { if (dependencies.withLock === null) return withLease(); const result = await dependencies.withLock(waitForLock, async () => { if (waitForLock && await useCurrentSession()) { dependencies.onSessionRotated(); return true; } return performRefresh(); }); if (result === "busy") { schedule(30); return false; } return result; })(); try { return await state.refreshPromise; } finally { state.refreshPromise = null; } };
  const recoverAuthentication = async (): Promise<void> => { if (state.authenticationRecoveryPromise !== null) return state.authenticationRecoveryPromise; state.authenticationRecoveryPromise = (async () => { dependencies.onStatus("Fornyar økta …"); if (await refresh(true)) return; if (state.refreshRejected) { if (await useCurrentSession()) { dependencies.onReauthenticationRequired(false); dependencies.onSessionRotated(); return; } if (dependencies.visibility() === "visible" && dependencies.now() - dependencies.lastUserActivityAt() < 120_000) { dependencies.onReauthenticationRequired(true); dependencies.onStatus("Økta må stadfestast – vi ventar så du ikkje mistar arbeidet ditt"); scheduleAuthenticationRecovery(30); return; } dependencies.onReauthenticationRequired(false); dependencies.onStatus("Økta må stadfestast på nytt …"); dependencies.onLoginRequired(); return; } dependencies.onReconnectNeeded("ventar på nett for å fornye økta"); })(); try { await state.authenticationRecoveryPromise; } finally { state.authenticationRecoveryPromise = null; } };
  const start = async (): Promise<void> => { dependencies.broadcast?.addEventListener("message", (event) => { const message = parseSessionRefreshBroadcast(event.data); if (message !== null) { schedule(message.refreshAfterSeconds); dependencies.onSessionRotated(); } }); try { const response = await dependencies.fetch("/auth/session", sessionRequest); if (!response.ok) { if (response.status === 401 && await refresh(true)) return; schedule(30); return; } schedule(sessionRefreshAfterSeconds(await response.json())); } catch { schedule(30); } };
  return Object.freeze({ snapshot: (): SessionSnapshot => Object.freeze({ refreshDueAt: state.refreshDueAt, refreshRejected: state.refreshRejected, refreshing: state.refreshPromise !== null, recoveringAuthentication: state.authenticationRecoveryPromise !== null }), start, schedule, refresh, recoverAuthentication, reauthenticateNow: (): void => { dependencies.onReauthenticationRequired(false); dependencies.onLoginRequired(); }, useCurrentSession });
}
