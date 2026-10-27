import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { deviceInstall } from "../../device-install";
import type { DevicePushStatus } from "../../device-push";
import { DeviceSetupNotice, type DeviceNoticeKind } from "./device-setup-notice";

const keys: Record<DeviceNoticeKind, string> = {
  notifications: "sproyt.device-notice.notifications.dismissed",
  install: "sproyt.device-notice.install.dismissed"
};
const memory = new Set<DeviceNoticeKind>();
function dismissed(kind: DeviceNoticeKind): boolean {
  try { return memory.has(kind) || localStorage.getItem(keys[kind]) === "1"; }
  catch { return memory.has(kind); }
}

/** These are browser-local choices; account switches never reset them. */
export function DeviceSetupReminders({ owner, active, connected, install, checkPush, openNotifications }: {
  owner: string;
  active: boolean;
  connected: boolean;
  install: typeof deviceInstall;
  checkPush: (owner: string, admitted: () => boolean) => Promise<DevicePushStatus>;
  openNotifications: () => void;
}) {
  const installOption = useSyncExternalStore(install.subscribe, install.getSnapshot);
  const [push, setPush] = useState<DevicePushStatus>("none");
  const [dismissedNotifications, setDismissedNotifications] = useState(() => dismissed("notifications"));
  const [dismissedInstall, setDismissedInstall] = useState(() => dismissed("install"));
  const [installError, setInstallError] = useState("");
  const checkRef = useRef(checkPush); checkRef.current = checkPush;
  useEffect(() => {
    let live = true, generation = 0;
    const refresh = () => {
      const attempt = ++generation;
      if (!connected || dismissedNotifications || document.visibilityState === "hidden") { setPush("none"); return; }
      const admitted = () => live && generation === attempt;
      void checkRef.current(owner, admitted).then(result => { if (admitted()) setPush(result); }).catch(() => { if (admitted()) setPush("none"); });
    };
    const storageChanged = (event: StorageEvent) => {
      if (event.key === keys.notifications) setDismissedNotifications(dismissed("notifications"));
      if (event.key === keys.install) setDismissedInstall(dismissed("install"));
    };
    document.addEventListener("visibilitychange", refresh);
    window.addEventListener("sproyt:push-settings-changed", refresh);
    window.addEventListener("storage", storageChanged);
    refresh();
    return () => { live = false; generation++; document.removeEventListener("visibilitychange", refresh);
      window.removeEventListener("sproyt:push-settings-changed", refresh); window.removeEventListener("storage", storageChanged); };
  }, [owner, connected, dismissedNotifications]);
  const dismiss = (kind: DeviceNoticeKind) => {
    memory.add(kind);
    try { localStorage.setItem(keys[kind], "1"); } catch { /* This session still respects the choice. */ }
    if (kind === "notifications") setDismissedNotifications(true); else { setDismissedInstall(true); setInstallError(""); }
  };
  if (!active || !connected) return null;
  return <DeviceSetupNotice notification={push} install={installOption ?? "none"} installError={installError}
    dismissedNotifications={dismissedNotifications} dismissedInstall={dismissedInstall} onDismiss={dismiss}
    onOpenNotifications={openNotifications} onInstall={() => {
      setInstallError("");
      void install.install().catch(() => setInstallError("Installasjonen kunne ikkje opnast. Prøv nettlesarmenyen."));
    }} />;
}
