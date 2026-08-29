import type { PersistedSendAdmission, TransportReadiness } from "./send-admission";
import { loadClientCoreWasm, type ClientCoreWasmLoader, type RawClientCoreInstance } from "./client-core-wasm";

const queueUntilSubscribed: PersistedSendAdmission = "queue_until_subscribed";
const dispatchNow: PersistedSendAdmission = "dispatch_now";

type RawSendAdmissionExports = Readonly<{
  sproyt_admit_persisted_send(connected: number, channelMatches: number, handoffActive: number): number;
}>;

export type SendAdmissionPolicy = Readonly<{
  admit(channelId: string, readiness: TransportReadiness): PersistedSendAdmission;
  ready: Promise<boolean>;
  usingWasm(): boolean;
}>;

function fallbackAdmission(channelId: string, readiness: TransportReadiness): PersistedSendAdmission {
  return readiness.connected && !readiness.handoffActive && readiness.subscribedChannelId === channelId
    ? dispatchNow
    : queueUntilSubscribed;
}

function rawExports(instance: RawClientCoreInstance): RawSendAdmissionExports | null {
  const exports = instance.exports;
  const admit = exports.sproyt_admit_persisted_send;
  return typeof admit === "function"
    ? { sproyt_admit_persisted_send: admit as RawSendAdmissionExports["sproyt_admit_persisted_send"] }
    : null;
}

function admitWithWasm(exports: RawSendAdmissionExports, channelId: string, readiness: TransportReadiness): PersistedSendAdmission {
  const result = exports.sproyt_admit_persisted_send(
    Number(readiness.connected), Number(readiness.subscribedChannelId === channelId), Number(readiness.handoffActive)
  );
  if (result === 1) return dispatchNow;
  if (result === 0) return queueUntilSubscribed;
  throw new Error("WASM admission returned an unknown result");
}

/**
 * Load the deterministic Rust policy without making browser sending depend on
 * an optional asset. Until it is ready, and after any loader/ABI/call failure,
 * the verified TypeScript mirror fails safely to the existing behaviour.
 */
export function createSendAdmissionPolicy(loader: ClientCoreWasmLoader = loadClientCoreWasm): SendAdmissionPolicy {
  let active: ((channelId: string, readiness: TransportReadiness) => PersistedSendAdmission) | null = null;
  const ready = loader().then((instance) => {
    const exports = rawExports(instance);
    if (!exports) return false;
    active = (channelId, readiness) => admitWithWasm(exports, channelId, readiness);
    return true;
  }).catch(() => false);
  return {
    admit(channelId, readiness) {
      try {
        return active?.(channelId, readiness) ?? fallbackAdmission(channelId, readiness);
      } catch {
        active = null;
        return fallbackAdmission(channelId, readiness);
      }
    },
    ready,
    usingWasm: () => active !== null
  };
}
