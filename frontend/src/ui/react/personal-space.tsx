import { useEffect, useId, useRef, useState } from "react";
import { Button, Status } from "@sproyt/ui/react";
import { HttpError } from "../../api";
import type { PersonalNote, PersonalSpaceApi } from "../../personal-space";
import { MarkdownContent } from "./markdown-content";

export function PersonalSpaceShortcut({ active, ready, onOpen }: {
  active: boolean; ready: boolean; onOpen: () => Promise<void>;
}) {
  const [pending, setPending] = useState(false);
  const [error, setError] = useState(false);
  return <div>
    <Button className="sp-personal-shortcut" variant="quiet" aria-current={active ? "page" : undefined}
      busy={pending} disabled={!ready} onClick={async () => {
        setPending(true);
        setError(false);
        try { await onOpen(); }
        catch { setError(true); }
        finally { setPending(false); }
      }}>Området mitt</Button>
    {error && <Status tone="error">Området kunne ikkje opnast. Prøv igjen.</Status>}
  </div>;
}

export function PersonalNotePanel({ active, api, owner }: { active: boolean; api: PersonalSpaceApi; owner: string }) {
  const heading = useId();
  const editor = useRef<HTMLTextAreaElement>(null);
  const loading = useRef(false);
  const storageKey = `sproyt.personal-note-draft.v1.${encodeURIComponent(owner)}`;
  const [note, setNote] = useState<PersonalNote | null>(null);
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [draftStorageFailed, setDraftStorageFailed] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [conflict, setConflict] = useState<PersonalNote | null>(null);
  const dirty = note !== null && note.markdown !== draft;
  const tooLong = draft.length > 64 * 1024 || new TextEncoder().encode(draft).length > 64 * 1024;

  function rememberDraft(next: PersonalNote | null) {
    try {
      if (next) sessionStorage.setItem(storageKey, JSON.stringify(next));
      else sessionStorage.removeItem(storageKey);
      setDraftStorageFailed(false);
    } catch {
      setDraftStorageFailed(next !== null);
    }
  }

  function applyNote(next: PersonalNote) {
    rememberDraft(null);
    setNote(next);
    setDraft(next.markdown);
    setConflict(null);
  }

  async function load() {
    if (loading.current) return;
    loading.current = true;
    setLoadFailed(false);
    try {
      const latest = await api.note(owner);
      let recovered: PersonalNote | null = null;
      try {
        const value = JSON.parse(sessionStorage.getItem(storageKey) ?? "null");
        if (value && typeof value.markdown === "string" && Number.isSafeInteger(value.revision) && value.revision >= 0) {
          recovered = value;
        }
      } catch { /* An unavailable draft does not prevent loading the note. */ }
      if (recovered && recovered.markdown !== latest.markdown) {
        setNote({ ...latest, revision: recovered.revision });
        setDraft(recovered.markdown);
        setEditing(true);
        if (recovered.revision !== latest.revision) setConflict(latest);
      } else {
        applyNote(latest);
      }
    }
    catch { setLoadFailed(true); }
    finally { loading.current = false; }
  }

  useEffect(() => {
    if (active && !note) void load();
  }, [active, api, owner]);

  useEffect(() => {
    if (active && editing) editor.current?.focus({ preventScroll: true });
  }, [active, editing]);

  useEffect(() => {
    if (!draftStorageFailed || !dirty) return;
    const warn = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [draftStorageFailed, dirty]);

  async function save(revision: number) {
    setSaving(true);
    setFeedback("");
    setConflict(null);
    try {
      applyNote(await api.saveNote(owner, { markdown: draft, revision }));
      setFeedback("Notatet er lagra.");
    } catch (error) {
      if (error instanceof HttpError && error.status === 409) {
        const latest = await api.note(owner).catch(() => null);
        if (latest?.markdown === draft) {
          applyNote(latest);
          setFeedback("Notatet er lagra.");
        } else if (latest) {
          setConflict(latest);
        } else {
          setFeedback("Notatet har ei nyare utgåve. Prøv å lagre på nytt for å hente henne.");
        }
      } else {
        setFeedback("Notatet kunne ikkje lagrast. Prøv igjen.");
      }
    } finally {
      setSaving(false);
    }
  }

  return <section className="sp-personal-note" hidden={!active} data-expanded={active && expanded}
    aria-labelledby={heading}>
    <div className="sp-personal-note-header">
      <h2 id={heading}>Notat</h2>
      <div className="sp-personal-note-actions">
        {note && <Button variant="quiet" disabled={saving} onClick={() => setEditing(!editing)}>{editing ? "Vis" : "Rediger"}</Button>}
        <Button variant="quiet" aria-expanded={expanded} aria-controls={`${heading}-content`}
          onClick={() => setExpanded(!expanded)}>{expanded ? "Minimer" : "Maksimer"}</Button>
        {note && dirty && <Button variant="primary" busy={saving} disabled={tooLong || !!conflict}
          onClick={() => void save(note.revision)}>Lagre</Button>}
      </div>
    </div>
    {active && <div className="sp-personal-note-content" id={`${heading}-content`}>
      {loadFailed ? <Status tone="error">Notatet kunne ikkje hentast. <Button onClick={() => void load()}>Prøv igjen</Button></Status>
        : !note ? <Status>Hentar notatet …</Status>
          : editing ? <textarea ref={editor} aria-label="Notat i Markdown" placeholder="Skriv eit notat i Markdown"
            value={draft} readOnly={saving} onChange={event => {
              const markdown = event.currentTarget.value;
              setDraft(markdown);
              setFeedback("");
              rememberDraft(markdown === note.markdown ? null : { markdown, revision: note.revision });
            }} />
            : draft.trim() ? <MarkdownContent source={draft} className="sp-personal-note-markdown sp-message-content" />
              : <p className="sp-personal-note-empty">Her kan du skrive eit notat i Markdown.</p>}
      {tooLong && <Status tone="error">Notatet kan vere på høgst 64 KiB.</Status>}
      {draftStorageFailed && dirty && <Status tone="error">Nettlesaren kunne ikkje ta vare på utkastet. Lagre notatet før du lastar sida på nytt.</Status>}
      {feedback && <Status tone={feedback === "Notatet er lagra." ? "info" : "error"}>{feedback}</Status>}
      {conflict && <div className="sp-personal-note-conflict">
        <Status tone="error">Notatet vart endra ein annan stad. Utkastet ditt er framleis her.</Status>
        <details><summary>Vis nyare lagra utgåve</summary>
          <MarkdownContent source={conflict.markdown} className="sp-personal-note-markdown sp-message-content" />
        </details>
        <Button onClick={() => applyNote(conflict)}>Bruk lagra utgåve</Button>
        <Button disabled={tooLong} onClick={() => void save(conflict.revision)}>Lagre mitt utkast</Button>
      </div>}
    </div>}
  </section>;
}
