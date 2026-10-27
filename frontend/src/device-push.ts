import type { NotificationSettings } from "./api";

export type DevicePushStatus = "needs-setup" | "blocked" | "none";

/** A missing or inconclusive check is deliberately not treated as disabled. */
export async function devicePushStatus(dependencies: {
  supported: boolean;
  permission: NotificationPermission | "unsupported";
  settings(): Promise<NotificationSettings>;
  subscription(): Promise<{ endpoint: string } | null>;
  registered(endpoint: string): Promise<boolean>;
  admitted(): boolean;
}): Promise<DevicePushStatus> {
  if (!dependencies.supported || dependencies.permission === "unsupported" || !dependencies.admitted()) return "none";
  try {
    const settings = await dependencies.settings();
    if (!dependencies.admitted() || !settings.enabled || settings.preferences.mode === "muted") return "none";
    if (dependencies.permission === "denied") return "blocked";
    if (dependencies.permission === "default") return "needs-setup";
    const subscription = await dependencies.subscription();
    if (!dependencies.admitted()) return "none";
    if (!subscription) return "needs-setup";
    const registered = await dependencies.registered(subscription.endpoint);
    if (!dependencies.admitted()) return "none";
    return registered ? "none" : "needs-setup";
  } catch {
    return "none";
  }
}
