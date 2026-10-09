import { Button, Dialog, Status, TextField } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { NotificationPreferences, NotificationSettings } from "../../api";
import type { UserProfile } from "../../types";
import type { SavedStatus } from "../../saved-statuses";

export interface PreviewSettingsHost {
  profile(): UserProfile | undefined;
  profileFor?(userId: string): UserProfile | undefined;
  saveName(name: string): Promise<void>;
  saveStatus(text: string, emoji: string): Promise<void>;
  loadStatuses(): Promise<readonly SavedStatus[]>;
  removeStatus(status: Pick<SavedStatus, "text" | "emoji">): Promise<void>;
  loadNotifications(): Promise<NotificationSettings>;
  saveNotifications(preferences: NotificationPreferences): Promise<void>;
  enablePush(publicKey: string): Promise<void>;
}

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

function SavedStatusChoices({ host, ownerId, busy, revision, onChoose }: {
  host: PreviewSettingsHost; ownerId: string; busy: boolean; revision: number;
  onChoose(status: SavedStatus): void;
}) {
  const latestHost = useRef(host); latestHost.current = host;
  const [items, setItems] = useState<readonly SavedStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let cancelled = false;
    setLoading(true); setError("");
    void latestHost.current.loadStatuses().then(items => { if (!cancelled) setItems(items); },
      error => { if (!cancelled) setError(errorText(error)); }).finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [ownerId, revision, attempt]);
  async function remove(status: SavedStatus) {
    setRemoving(true); setError("");
    try { await latestHost.current.removeStatus(status); setAttempt(value => value + 1); }
    catch (error) { setError(errorText(error)); }
    finally { setRemoving(false); }
  }
  return <details className="sp-saved-statuses">
    <summary>Tidlegare statusar</summary>
    <p className="sp-help">Mest brukte først. Vel eit forslag, og trykk «Lagre status».</p>
    {loading && <Status>Lastar statusval …</Status>}
    {!loading && !error && !items.length && <p className="sp-help">Statusar du lagrar, dukkar opp her.</p>}
    <ul>{items.map(status => {
      const label = [status.emoji, status.text].filter(Boolean).join(" ");
      return <li key={JSON.stringify([status.emoji, status.text])}>
        <Button variant="quiet" disabled={busy || removing || loading} onClick={() => onChoose(status)}
          aria-label={`Bruk status: ${label}`} title={label}>{status.emoji && <span>{status.emoji}</span>}<span>{status.text}</span></Button>
        <Button variant="symbol" disabled={busy || removing || loading} onClick={() => void remove(status)}
          aria-label={`Gløym status: ${label}`} title="Fjern frå lista"><span aria-hidden="true">×</span></Button>
      </li>;
    })}</ul>
    {error && <Status tone="error">{error} <Button disabled={busy || removing} onClick={() => setAttempt(value => value + 1)}>Prøv igjen</Button></Status>}
  </details>;
}

export function PreviewProfile({ host, focusStatus = false }: { host: PreviewSettingsHost; focusStatus?: boolean }) {
  const profile = host.profile();
  const [name, setName] = useState(profile?.display_name ?? "");
  const [text, setText] = useState(profile?.status_text ?? "");
  const [emoji, setEmoji] = useState(profile?.status_emoji ?? "");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  const [statusRevision, setStatusRevision] = useState(0);
  const initialized = useRef(Boolean(profile));
  const statusField = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!focusStatus || !profile) return;
    // React autofocus runs before the native dialog becomes modal.
    const frame = requestAnimationFrame(() => statusField.current?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(frame);
  }, [focusStatus, Boolean(profile)]);
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
      <TextField label="Visningsnamn" value={name} onChange={event => setName(event.target.value)} required disabled={busy} autoFocus={!focusStatus} />
      <Button type="submit" busy={busy} disabled={!name.trim()}>Lagre namn</Button>
    </form>
    <form onSubmit={event => { event.preventDefault(); void save(async () => {
      await host.saveStatus(text, emoji); setStatusRevision(value => value + 1);
    }, "Statusen er lagra."); }}>
      <SavedStatusChoices key={profile.id} host={host} ownerId={profile.id} busy={busy} revision={statusRevision}
        onChoose={status => { setText(status.text); setEmoji(status.emoji); setNotice(""); setError(""); statusField.current?.focus({ preventScroll: true }); }} />
      <TextField label="Statusemoji" value={emoji} onChange={event => setEmoji(event.target.value)} disabled={busy} />
      <TextField ref={statusField} label="Statusmelding" value={text} onChange={event => setText(event.target.value)} disabled={busy} />
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

export function PreviewSettingsDialog({ kind, host, onClose, focusStatus = false }: {
  kind: "profile" | "notifications"; host: PreviewSettingsHost; onClose(): void; focusStatus?: boolean;
}) {
  return <Dialog open title={kind === "profile" ? "Profil og status" : "Varslingsinnstillingar"} closeLabel={focusStatus ? "Lukk status" : "Tilbake til menyen"} onClose={onClose}>
    {kind === "profile" ? <PreviewProfile host={host} focusStatus={focusStatus} /> : <PreviewNotifications host={host} />}
  </Dialog>;
}

/** Other members' statuses are informational; only your own opens the editor. */
export function MessageProfileStatus({ host, userId, own }: { host: PreviewSettingsHost; userId: string; own: boolean }) {
  const profile = host.profileFor?.(userId);
  const status = [profile?.status_emoji.trim(), profile?.status_text.trim()].filter(Boolean).join(" ");
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  if (!status && !open) return null;
  if (!own) return <span className="sp-message-profile-status" title={status}>{status}</span>;
  return <>
    {status && <button ref={trigger} type="button" className="sp-message-profile-status sp-message-status-edit"
      aria-label={`Endre status: ${status}`} title="Endre status"
      onPointerDown={event => event.stopPropagation()}
      onClick={event => {
        event.stopPropagation(); event.currentTarget.focus({ preventScroll: true }); setOpen(true);
      }}>{status}</button>}
    {open && <PreviewSettingsDialog kind="profile" host={host} focusStatus onClose={() => {
      setOpen(false); requestAnimationFrame(() => trigger.current?.focus({ preventScroll: true }));
    }} />}
  </>;
}
