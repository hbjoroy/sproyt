/**
 * Staged contract mirror for `sproyt-client-core::admit_persisted_send`.
 *
 * The Rust core is pure in this stage, so this is deliberately not a browser
 * WASM binding yet. A shared fixture keeps this mirror aligned while the
 * CSP-safe asset pipeline is designed.
 */
export type PersistedSendAdmission = "dispatch_now" | "queue_until_subscribed";
export type TransportReadiness = Readonly<{ connected: boolean; subscribedChannelId: string | null; handoffActive: boolean }>;

export function admitPersistedSend(channelId: string, readiness: TransportReadiness): PersistedSendAdmission {
  return readiness.connected && !readiness.handoffActive && readiness.subscribedChannelId === channelId
    ? "dispatch_now"
    : "queue_until_subscribed";
}
