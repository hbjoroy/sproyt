/** Installation is a browser/device capability, not an account preference.
 * This module must be imported at app startup: Chromium can dispatch
 * beforeinstallprompt before the React interface is mounted. */
export type DeviceInstallOption = "native" | "ios" | null;

type InstallPrompt = Event & { prompt(): Promise<unknown> };
type DeviceNavigator = Pick<Navigator, "userAgent" | "platform" | "maxTouchPoints"> & { standalone?: boolean };
type DeviceWindow = Pick<Window, "addEventListener" | "removeEventListener" | "matchMedia"> & { navigator: DeviceNavigator };

export function createDeviceInstall(target: DeviceWindow | null) {
  let deferred: InstallPrompt | null = null;
  let installed = false;
  const listeners = new Set<() => void>();
  const modes: MediaQueryList[] = [];
  if (target && typeof target.matchMedia === "function") {
    for (const mode of ["standalone", "fullscreen", "minimal-ui"]) {
      try { modes.push(target.matchMedia(`(display-mode: ${mode})`)); } catch { /* unavailable */ }
    }
  }
  const isIos = !!target && (/iPad|iPhone|iPod/u.test(target.navigator.userAgent)
    || (target.navigator.platform === "MacIntel" && target.navigator.maxTouchPoints > 1));
  const current = (): DeviceInstallOption => {
    if (!target || installed || target.navigator.standalone === true || modes.some(mode => mode.matches)) return null;
    if (deferred) return "native";
    // iOS exposes no install prompt and does not reveal installations in a
    // different browser or home-screen instance. Present guidance, not a
    // claim about what is installed elsewhere.
    return isIos ? "ios" : null;
  };
  let snapshot = current();
  const refresh = () => {
    const next = current();
    if (next === snapshot) return;
    snapshot = next;
    listeners.forEach(listener => listener());
  };
  const beforeInstall: EventListener = event => {
    if (!("prompt" in event) || typeof (event as InstallPrompt).prompt !== "function" || installed) return;
    event.preventDefault();
    deferred = event as InstallPrompt;
    refresh();
  };
  const appInstalled: EventListener = () => { installed = true; deferred = null; refresh(); };
  target?.addEventListener("beforeinstallprompt", beforeInstall);
  target?.addEventListener("appinstalled", appInstalled);
  for (const mode of modes) mode.addEventListener?.("change", refresh);
  return Object.freeze({
    getSnapshot: () => snapshot,
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    async install(): Promise<void> {
      // A prompt can be used once and must be called directly from a user
      // action; consume it before awaiting so double clicks cannot reuse it.
      const prompt = deferred;
      if (!prompt || current() !== "native") return;
      deferred = null;
      refresh();
      await prompt.prompt();
    },
    dispose() {
      target?.removeEventListener("beforeinstallprompt", beforeInstall);
      target?.removeEventListener("appinstalled", appInstalled);
      for (const mode of modes) mode.removeEventListener?.("change", refresh);
      listeners.clear();
    }
  });
}

/** Import directly in the app entry point, before dynamically importing UI. */
export const deviceInstall = createDeviceInstall(typeof window === "undefined" ? null : window);
