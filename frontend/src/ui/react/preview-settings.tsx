import { Button, Dialog, Status, TextField } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { NotificationPreferences, NotificationSettings } from "../../api";
import type { UserProfile } from "../../types";

export interface PreviewSettingsHost {
  profile(): UserProfile | undefined;
  profileFor?(userId: string): UserProfile | undefined;
  saveName(name: string): Promise<void>;
  saveStatus(text: string, emoji: string): Promise<void>;
  loadNotifications(): Promise<NotificationSettings>;
  saveNotifications(preferences: NotificationPreferences): Promise<void>;
  enablePush(publicKey: string): Promise<void>;
}

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

export function PreviewProfile({ host }: { host: PreviewSettingsHost }) {
  const profile = host.profile();
  const [name, setName] = useState(profile?.display_name ?? "");
  const [text, setText] = useState(profile?.status_text ?? "");
  const [emoji, setEmoji] = useState(profile?.status_emoji ?? "");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  const initialized = useRef(Boolean(profile));
  useEffect(() => {
    if (!initialized.current && profile) {
      initialized.current = true;
      setName(profile.display_name); setText(profile.status_text); setEmoji(profile.status_emoji);
    }
  }, [profile]);
  async function save(action: () => Promise<void>, message: string) {
    if (busy) return;
    setBusy(true); setError(""); setNotice("");
    try { await action(); setNotice(message); }
    catch (error) { setError(errorText(error)); }
    finally { setBusy(false); }
  }
  if (!profile) return <Status>Profilen blir lasta. Lukk og opne innstillingane igjen om det tek lang tid.</Status>;
  return <div style={{ display: "grid", gap: 16 }}>
    {profile.early_adopter && <p className="sp-profile-early-adopter" title="Du er blant dei første 50 på Sprøyt">
      <span aria-hidden="true">✨</span> Første 50 på Sprøyt
    </p>}
    {profile.handle && <p>Offentleg brukarnamn: @{profile.handle}</p>}
    <form onSubmit={event => { event.preventDefault(); void save(() => host.saveName(name.trim()), "Namnet er lagra."); }}>
      <TextField label="Visningsnamn" value={name} onChange={event => setName(event.target.value)} required disabled={busy} autoFocus />
      <Button type="submit" busy={busy} disabled={!name.trim()}>Lagre namn</Button>
    </form>
    <form onSubmit={event => { event.preventDefault(); void save(() => host.saveStatus(text, emoji), "Statusen er lagra."); }}>
      <TextField label="Statusemoji" value={emoji} onChange={event => setEmoji(event.target.value)} disabled={busy} />
      <TextField label="Statusmelding" value={text} onChange={event => setText(event.target.value)} disabled={busy} />
      <Button type="submit" busy={busy}>Lagre status</Button>
      <Button disabled={busy} onClick={() => void save(async () => {
        await host.saveStatus("", ""); setText(""); setEmoji("");
      }, "Statusen er tømd.")}>Tøm status</Button>
    </form>
    {error && <Status tone="error">{error}</Status>}
    {notice && <Status>{notice}</Status>}
  </div>;
}

export function PreviewNotifications({ host }: { host: PreviewSettingsHost }) {
  const [settings, setSettings] = useState<NotificationSettings>();
  const [preferences, setPreferences] = useState<NotificationPreferences>();
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let current = true;
    setBusy(true); setError("");
    host.loadNotifications().then(value => {
      if (current) { setSettings(value); setPreferences(value.preferences); }
    }).catch(error => { if (current) setError(errorText(error)); })
      .finally(() => { if (current) setBusy(false); });
    return () => { current = false; };
  }, [host, attempt]);
  async function run(action: () => Promise<void>, message: string) {
    if (busy) return;
    setBusy(true); setError(""); setNotice("");
    try { await action(); setNotice(message); }
    catch (error) { setError(errorText(error)); }
    finally { setBusy(false); }
  }
  const supported = "Notification" in window && "PushManager" in window && "serviceWorker" in navigator;
  const permission = supported ? Notification.permission : "unsupported";
  return <div style={{ display: "grid", gap: 16 }}>
    {!settings && busy && <Status>Lastar varslingsinnstillingar …</Status>}
    {preferences && <form onSubmit={event => {
      event.preventDefault(); void run(() => host.saveNotifications(preferences), "Varslingsinnstillingane er lagra.");
    }}>
      <label htmlFor="preview-notification-mode">Varslingsmodus</label>
      <select id="preview-notification-mode" value={preferences.mode} disabled={busy} onChange={event => {
        const mode = event.target.value;
        if (mode === "instant" || mode === "weekly" || mode === "muted") setPreferences({ ...preferences, mode });
      }}>
        <option value="instant">Direkte</option><option value="weekly">Kvar veke</option><option value="muted">Ingen varsel</option>
      </select>
      <label style={{ display: "block", paddingBlock: 8 }}><input type="checkbox" checked={preferences.directMessages} disabled={busy}
        onChange={event => setPreferences({ ...preferences, directMessages: event.target.checked })} /> Direktemeldingar</label>
      <label style={{ display: "block", paddingBlock: 8 }}><input type="checkbox" checked={preferences.mentions} disabled={busy}
        onChange={event => setPreferences({ ...preferences, mentions: event.target.checked })} /> Omtalar</label>
      <Button type="submit" busy={busy}>Lagre varslingsinnstillingar</Button>
    </form>}
    {settings && <section aria-label="Nettlesarvarsling">
      <p>{!settings.enabled ? "Push er ikkje konfigurert på serveren enno."
        : `${settings.subscriptions} eining(ar) tek imot varsel.`}</p>
      <p>{permission === "unsupported" ? "Denne nettlesaren støttar ikkje push-varsel."
        : permission === "denied" ? "Varsel er blokkerte i nettlesaren. Endre løyvet i nettlesarinnstillingane."
        : permission === "granted" ? "Nettlesaren har tillate varsel." : "Nettlesaren har ikkje fått løyve til å vise varsel."}</p>
      <Button disabled={busy || !settings.enabled || !supported || permission === "denied"}
        onClick={() => void run(async () => {
          await host.enablePush(settings.publicKey);
          setSettings(await host.loadNotifications());
        }, "Varsel er slått på på denne eininga.")}>Slå på varsel på denne eininga</Button>
    </section>}
    {error && <Status tone="error">{error} {!settings && <Button disabled={busy} onClick={() => setAttempt(value => value + 1)}>Prøv igjen</Button>}</Status>}
    {notice && <Status>{notice}</Status>}
  </div>;
}

export function PreviewSettingsDialog({ kind, host, onClose }: {
  kind: "profile" | "notifications"; host: PreviewSettingsHost; onClose(): void;
}) {
  return <Dialog open title={kind === "profile" ? "Profil og status" : "Varslingsinnstillingar"} closeLabel="Tilbake til menyen" onClose={onClose}>
    {kind === "profile" ? <PreviewProfile host={host} /> : <PreviewNotifications host={host} />}
  </Dialog>;
}
