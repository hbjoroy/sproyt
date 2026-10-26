import { Button } from "@sproyt/ui/react";
import { useId, type ReactNode } from "react";

type SettingsSectionProps = { title: string; description?: string; children: ReactNode };

export function SettingsSection({ title, description, children }: SettingsSectionProps) {
  return <section className="sp-settings-section">
    <h3>{title}</h3>
    {description && <p className="sp-settings-section-description">{description}</p>}
    {children}
  </section>;
}

export function SettingsContext({ scope, role }: { scope: string; role?: string }) {
  return <p className="sp-settings-context"><span>{scope}</span>{role && <><span aria-hidden="true"> · </span><span>{role}</span></>}</p>;
}

type SettingsRowProps = { label: string; detail?: string; onClick: () => void; disabled?: boolean; children?: ReactNode };

export function SettingsRow({ label, detail, onClick, disabled, children }: SettingsRowProps) {
  const detailId = useId();
  // The description supplements the explicit accessible name of the action.
  return <Button className="sp-settings-choice" variant="quiet" aria-label={label}
    aria-describedby={detail ? detailId : undefined} onClick={onClick} disabled={disabled}>
    <span className="sp-settings-choice-copy"><span className="sp-settings-choice-label">{label}</span>
      {detail && <span className="sp-settings-choice-detail" id={detailId}>{detail}</span>}</span>
    {children && <span className="sp-settings-choice-extra">{children}</span>}
    <span className="sp-settings-choice-arrow" aria-hidden="true">›</span>
  </Button>;
}

export function SettingsSummary({ label, value, tone = "neutral" }: { label: string; value: string; tone?: "neutral" | "warning" }) {
  // Text remains present in both tones so colour is never the only signal.
  return <div className="sp-settings-summary" data-tone={tone}><span>{label}</span><strong>{value}</strong></div>;
}
