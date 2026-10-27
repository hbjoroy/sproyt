import { Button } from "@sproyt/ui/react";
import { useId, useState } from "react";
import noticeCss from "../device-setup-notice.css";

export type DeviceNoticeKind = "notifications" | "install";
export type DeviceNoticePermission = "needs-setup" | "blocked" | "none";
export type DeviceNoticeInstallation = "native" | "ios" | "none";

/** The host owns eligibility, storage and the actual browser operations. */
export type DeviceSetupNoticeProps = {
  notification: DeviceNoticePermission;
  install: DeviceNoticeInstallation;
  installError?: string;
  dismissedNotifications: boolean;
  dismissedInstall: boolean;
  onDismiss: (kind: DeviceNoticeKind) => void;
  onOpenNotifications: () => void;
  onInstall: () => void;
};

/** Keep the conversation visible while presenting one optional action at a time. */
export function DeviceSetupNotice({ notification, install, installError, dismissedNotifications, dismissedInstall,
  onDismiss, onOpenNotifications, onInstall }: DeviceSetupNoticeProps) {
  const [instructionsOpen, setInstructionsOpen] = useState(false);
  const instructionsId = useId();
  const hasNotifications = notification !== "none" && !dismissedNotifications;
  const hasInstall = (install !== "none" || Boolean(installError)) && !dismissedInstall;
  // iOS web push requires an installed web app, so introduce that step first.
  const kind: DeviceNoticeKind | null = (install === "ios" || Boolean(installError)) && hasInstall ? "install"
    : hasNotifications ? "notifications" : hasInstall ? "install" : null;
  if (!kind) return null;

  const ios = kind === "install" && install === "ios";
  return <section className="sp-device-notice" aria-label={kind === "notifications" ? "Påminning om varsel" : "Påminning om Sprøyt som app"}>
    <style>{noticeCss}</style>
    <div className="sp-device-notice-row">
      <p className="sp-device-notice-copy">{kind === "notifications"
        ? notification === "blocked" ? "Varsel er blokkerte i nettlesaren." : "Få varsel på denne eininga."
        : installError ? "Kunne ikkje installere appen." : "Legg Sprøyt på heimskjermen for rask tilgang."}</p>
      {kind === "notifications" ? <Button variant="quiet" className="sp-device-notice-action" aria-label="Opne varslingsinnstillingar" onClick={onOpenNotifications}>Innstillingar</Button>
        : ios ? <Button variant="quiet" className="sp-device-notice-action" aria-expanded={instructionsOpen}
          aria-controls={instructionsId} onClick={() => setInstructionsOpen(open => !open)}>Slik gjer du</Button>
          : install === "native" ? <Button variant="quiet" className="sp-device-notice-action" onClick={onInstall}>{installError ? "Prøv igjen" : "Installer appen"}</Button> : null}
      <Button variant="symbol" className="sp-device-notice-dismiss"
        aria-label={kind === "notifications" ? "Lukk påminning om varsel" : "Lukk påminning om Sprøyt som app"}
        onClick={() => onDismiss(kind)}><span aria-hidden="true">×</span></Button>
    </div>
    {ios && <p id={instructionsId} className="sp-device-notice-instructions" hidden={!instructionsOpen}>
      Opne delingsmenyen i nettlesaren, og vel «Legg til på heimskjerm».
    </p>}
    {kind === "install" && installError && <p className="sp-device-notice-error" role="alert">{installError}</p>}
  </section>;
}
