import { Avatar, Button, Status } from "@sproyt/ui/react";
import { useEffect, useState } from "react";
import { avatarChanged, ProfileAvatar } from "./profile-avatar";

export type AvatarStyle = "pencil" | "comic" | "oil";
export type AvatarDraft = { id: string; state: string; preview?: string };
export interface AvatarActions {
  current(owner: string, signal: AbortSignal): Promise<AvatarDraft | null>;
  generate(owner: string, photo: File, style: AvatarStyle, request: string): Promise<AvatarDraft>;
  review(owner: string, job: string, approve: boolean): Promise<void>;
}

export function AvatarEditor({ owner, name, actions }: { owner: string; name: string; actions: AvatarActions }) {
  const [file, setFile] = useState<File | null>(null);
  const [style, setStyle] = useState<AvatarStyle>("pencil");
  const [draft, setDraft] = useState<AvatarDraft | null>(null);
  const [busy, setBusy] = useState(false);
  const [previewLoaded, setPreviewLoaded] = useState("");
  const [checking, setChecking] = useState(true);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [refresh, setRefresh] = useState(0);
  const [request, setRequest] = useState(() => crypto.randomUUID());
  useEffect(() => {
    const controller = new AbortController();
    let timer: number;
    async function check() {
      try {
        const candidate = await actions.current(owner, controller.signal);
        if (controller.signal.aborted) return;
        setDraft(candidate);
        if (candidate && !["ready", "failed"].includes(candidate.state)) timer = window.setTimeout(check, 3000);
      } catch (cause) {
        if (!controller.signal.aborted) setError(cause instanceof Error ? cause.message : "Kunne ikkje hente avatarutkastet.");
      } finally { if (!controller.signal.aborted) setChecking(false); }
    }
    void check();
    return () => { controller.abort(); clearTimeout(timer); };
  }, [owner, actions, refresh]);

  async function generate() {
    if (!file || busy) return;
    setBusy(true); setError(""); setNotice("");
    try {
      const candidate = await actions.generate(owner, file, style, request);
      setDraft(candidate); setFile(null); setRefresh(value => value + 1);
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Kunne ikkje lage avataren."); }
    finally { setBusy(false); }
  }

  async function review(approve: boolean) {
    if (!draft || busy) return;
    setBusy(true); setError("");
    try {
      await actions.review(owner, draft.id, approve);
      if (approve) avatarChanged(owner, draft.id);
      setNotice(approve ? "Avataren er i bruk." : "Utkastet er forkasta.");
      setDraft(null); setFile(null); setRequest(crypto.randomUUID());
    } catch (cause) { setError(cause instanceof Error ? cause.message : "Kunne ikkje lagre valet."); }
    finally { setBusy(false); }
  }

  return <section className="sp-avatar-editor" aria-label="Profilavatar">
    <h3>Profilavatar</h3>
    <ProfileAvatar person={owner} fallback={<Avatar name={name} />} />
    {!draft && <form onSubmit={event => { event.preventDefault(); void generate(); }}>
      <p>Vel eit tydeleg portrett teke framanfrå. Når du lagar ei førehandsvising, blir portrettet stilisert på Santorini.</p>
      <label>Vel eit foto av deg sjølv<input type="file" accept="image/jpeg,image/png,image/webp" disabled={busy || checking}
        onChange={event => { setFile(event.target.files?.[0] ?? null); setRequest(crypto.randomUUID()); setError(""); setNotice(""); }} /></label>
      <label>Stil<select value={style} disabled={busy} onChange={event => { setStyle(event.target.value as AvatarStyle); setRequest(crypto.randomUUID()); }}>
        <option value="pencil">Fargeblyant</option><option value="comic">Teikneserie</option><option value="oil">Oljemåleri</option>
      </select></label>
      <Button type="submit" busy={busy} disabled={!file || busy || checking}>Lag førehandsvising</Button>
    </form>}
    {draft?.state === "ready" && draft.preview && <>
      <img className="sp-avatar-preview" src={draft.preview} alt="Førehandsvising av den nye avataren"
        onLoad={() => setPreviewLoaded(draft.id)} onError={() => { setPreviewLoaded(""); setError("Kunne ikkje vise førehandsvisinga."); }} />
      <Button disabled={busy || previewLoaded !== draft.id} onClick={() => void review(true)}>Bruk denne avataren</Button>
    </>}
    {draft?.state === "failed" && <Status tone="error">Avataren kunne ikkje lagast. Forkast utkastet og prøv igjen.</Status>}
    {draft && !["ready", "failed"].includes(draft.state) && <Status>Lagar avataren …</Status>}
    {draft && ["ready", "failed"].includes(draft.state) && <Button variant="quiet" disabled={busy} onClick={() => void review(false)}>Forkast utkast</Button>}
    {error && <Status tone="error">{error} <Button variant="quiet" disabled={busy} onClick={() => { setError(""); setRefresh(value => value + 1); }}>Hent status på nytt</Button></Status>}
    {notice && <Status>{notice}</Status>}
  </section>;
}
