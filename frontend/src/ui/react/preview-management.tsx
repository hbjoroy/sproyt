import { Button, Dialog, TextField } from "@sproyt/ui/react";
import { useRef, useState } from "react";
import type { ConversationSnapshot } from "../../application/conversation-snapshot";
import { PreviewSettingsDialog, type PreviewSettingsHost } from "./preview-settings";
import { PreviewCommunity, type CommunityDestination, type CommunityHost } from "./preview-community";
import { PreviewAdvanced } from "./preview-advanced";
import type { AdvancedHost } from "../../application/advanced-host";

export type ManagementDestination =
  | { kind: "create-circle" | "circles" | "people" | "channel" | "global-channels" | "global-invite" | "profile" | "notifications" | "agent" | "heart" }
  | { kind: "channels" | "invite"; circleId: string };

/** Focused management tasks share the host's commands and state. Opening the
 * directory never creates invitations, credentials or processes. */
export function PreviewManagement({ snapshot, onNavigate, capabilities, settings, community, advanced }: {
  snapshot: ConversationSnapshot;
  onNavigate: (destination: ManagementDestination) => void;
  capabilities: { agent: boolean; heart: boolean };
  settings: PreviewSettingsHost;
  community: CommunityHost;
  advanced: AdvancedHost;
}) {
  const [open, setOpen] = useState(false);
  const [setting, setSetting] = useState<"profile" | "notifications">();
  const settingsTrigger = useRef<HTMLButtonElement | null>(null);
  const [query, setQuery] = useState("");
  const [destination, setDestination] = useState<CommunityDestination>();
  const communityTrigger = useRef<HTMLButtonElement | null>(null);
  const [advancedKind, setAdvancedKind] = useState<"agent" | "heart">();
  const advancedTrigger = useRef<HTMLButtonElement | null>(null);
  const action = (label: string, destination: ManagementDestination) =>
    <Button key={label} onClick={event => {
      if (["people", "channel", "create-circle", "circles", "global-channels", "global-invite", "channels", "invite"].includes(destination.kind)) {
        communityTrigger.current = event.currentTarget;
        setOpen(false);
        if (destination.kind === "channel") {
          if (snapshot.activeChannel) setDestination({ kind: "channel", channelId: snapshot.activeChannel.id });
        } else setDestination(destination as CommunityDestination);
      } else if (destination.kind === "agent" || destination.kind === "heart") {
        advancedTrigger.current = event.currentTarget;
        setAdvancedKind(destination.kind);
      } else onNavigate(destination);
    }}>{label}</Button>;
  return <>
    <Button onClick={event => {
      // Safari does not focus buttons on pointer activation. Give the native
      // dialog a stable return target instead of leaving focus on the draft.
      event.currentTarget.focus({ preventScroll: true });
      setOpen(true);
    }}>Meny og innstillingar</Button>
    <Dialog open={open} title="Meny og innstillingar" closeLabel="Lukk menyen" onClose={() => setOpen(false)}>
      <p>Samtalen og utkasta dine blir tekne vare på.</p>
      <div style={{ display: "grid", gap: 8 }}>
        {action("Personar og ny direktemelding", { kind: "people" })}
        {snapshot.activeChannel && action("Kanaldetaljar, medlemmer og integrasjonar", { kind: "channel" })}
        {action("Kanalar i Felles", { kind: "global-channels" })}
        {action("Inviter ny brukar til Sprøyt", { kind: "global-invite" })}
        {action("Ny vennekrets", { kind: "create-circle" })}
        {action("Kretsadministrasjon og invitasjonskode", { kind: "circles" })}
        <Button onClick={event => { settingsTrigger.current = event.currentTarget; setSetting("profile"); }}>Profil og status</Button>
        <Button onClick={event => { settingsTrigger.current = event.currentTarget; setSetting("notifications"); }}>Varslingsinnstillingar</Button>
        {capabilities.agent && action("Agenttilgang", { kind: "agent" })}
        {capabilities.heart && action("Heart og planlegging", { kind: "heart" })}
      </div>
      {snapshot.circles.length > 0 && <section aria-label="Vennekretsar">
        <TextField label="Finn vennekrets" type="search" value={query} onChange={event => setQuery(event.target.value)} />
        {snapshot.circles.filter(circle => circle.name.toLocaleLowerCase().includes(query.toLocaleLowerCase())).map(circle =>
          <section key={circle.id} aria-label={circle.name} style={{ borderTop: "1px solid var(--sp-line)", paddingBlock: 12 }}>
            <h3>{circle.name}</h3>
            {action("Kanalar og medlemskap", { kind: "channels", circleId: circle.id })}
            {circle.role === "owner" && action("Inviter personar og nye brukarar", { kind: "invite", circleId: circle.id })}
          </section>)}
      </section>}
    </Dialog>
    {setting && <PreviewSettingsDialog key={setting} kind={setting} host={settings} onClose={() => {
      setSetting(undefined);
      requestAnimationFrame(() => settingsTrigger.current?.focus({ preventScroll: true }));
    }} />}
    {destination && <PreviewCommunity key={JSON.stringify(destination)} destination={destination} snapshot={snapshot} host={community}
      onNavigate={setDestination} onClose={() => { setDestination(undefined); setOpen(true); requestAnimationFrame(() => communityTrigger.current?.focus({ preventScroll: true })); }} />}
    {advancedKind && <PreviewAdvanced kind={advancedKind} host={advanced} snapshot={snapshot} onClose={() => {
      setAdvancedKind(undefined); requestAnimationFrame(() => advancedTrigger.current?.focus({ preventScroll: true }));
    }} />}
  </>;
}
