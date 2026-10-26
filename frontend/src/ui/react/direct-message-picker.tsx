import { Avatar, Button, Status, TextField } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { ConversationSnapshot } from "../../application/conversation-snapshot";
import { existingDirectChannel } from "../../direct-conversation";
import type { UserProfile } from "../../types";
import type { CommunityHost } from "./preview-community";

/** An operation keeps the owner/resource ticket from when it started. */
export function useCommunityViewScope(host: Pick<CommunityHost, "selfId">, resource: string) {
  const owner = host.selfId();
  const current = useRef({ owner, resource, live: true });
  if (current.current.owner !== owner || current.current.resource !== resource) current.current = { owner, resource, live: true };
  useEffect(() => {
    const scope = current.current; scope.live = true;
    return () => { scope.live = false; };
  }, [owner, resource]);
  return { owner, capture: () => {
    const scope = current.current;
    return () => scope.live && current.current === scope && scope.owner !== null && host.selfId() === scope.owner;
  } };
}

/** One compact person picker shared by the directory and new-DM entry points. */
export function DirectMessagePicker({ host, snapshot, onClose, onSelectChannel }: {
  host: CommunityHost; snapshot: ConversationSnapshot; onClose(): void; onSelectChannel?: (channelId: string) => void;
}) {
  const scope = useCommunityViewScope(host, "direct-picker");
  const [people, setPeople] = useState<readonly UserProfile[]>([]);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [loaded, setLoaded] = useState(false);
  const [readError, setReadError] = useState("");
  const [openError, setOpenError] = useState("");
  const [opening, setOpening] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const field = useRef<HTMLInputElement>(null);
  const reads = useRef(0), pending = useRef(false);
  useEffect(() => {
    const frame = requestAnimationFrame(() => field.current?.focus({ preventScroll: true }));
    return () => cancelAnimationFrame(frame);
  }, []);
  useEffect(() => {
    const admitted = scope.capture(), read = ++reads.current;
    if (!admitted()) { setLoading(false); return; }
    setLoading(true); setReadError("");
    void host.users().then(users => {
      if (admitted() && read === reads.current) { setPeople(users.filter(person => person.id !== scope.owner)); setLoaded(true); }
    }).catch(cause => {
      if (admitted() && read === reads.current) setReadError(cause instanceof Error ? cause.message : "Kunne ikkje hente personane.");
    }).finally(() => { if (admitted() && read === reads.current) setLoading(false); });
    return () => { reads.current++; };
  }, [scope.owner, reload]);
  const open = async (person: UserProfile) => {
    const admitted = scope.capture();
    if (!admitted() || pending.current) return;
    pending.current = true; setOpening(person.id); setOpenError("");
    try {
      const existing = existingDirectChannel(snapshot.channels, person.id);
      if (existing && onSelectChannel) onSelectChannel(existing.id);
      else await host.openDirect(person.id);
      if (admitted()) onClose();
    } catch (cause) {
      if (admitted()) setOpenError(cause instanceof Error ? cause.message : "Kunne ikkje opne samtalen. Prøv igjen.");
    } finally { if (admitted()) { pending.current = false; setOpening(null); } }
  };
  const needle = query.trim().toLocaleLowerCase();
  const visible = people.filter(person => `${person.display_name} ${person.handle ?? ""}`.toLocaleLowerCase().includes(needle));
  return <section className="sp-direct-picker" aria-label="Vel person">
    <TextField ref={field} label="Finn person" type="search" autoComplete="off" value={query} onChange={event => setQuery(event.target.value)} />
    {loading && <Status>Hentar personar …</Status>}
    {readError && <div className="sp-direct-picker-feedback"><Status tone="error">{readError}</Status>
      <Button variant="quiet" disabled={loading || !!opening} onClick={() => setReload(value => value + 1)}>Prøv igjen</Button></div>}
    {openError && <Status tone="error">{openError}</Status>}
    {loaded && !visible.length && <p className="sp-direct-picker-empty">{needle ? "Ingen personar passar søket." : "Ingen andre personar er tilgjengelege enno."}</p>}
    <ul className="sp-direct-picker-list">{visible.map(person => {
      const existing = existingDirectChannel(snapshot.channels, person.id);
      const action = existing ? "Opne samtale" : "Start samtale";
      return <li key={person.id}><Button className="sp-direct-person" variant="quiet" disabled={!!opening} busy={opening === person.id}
        aria-label={`${action} med ${person.display_name}`} onClick={() => void open(person)}>
        <Avatar name={person.display_name} />
        <span className="sp-direct-person-copy"><span className="sp-direct-person-name">{person.display_name}</span>
          <span className="sp-direct-person-detail">{[person.handle ? `@${person.handle}` : "", existing ? "Opne samtale" : "Ny samtale"].filter(Boolean).join(" · ")}</span></span>
        <span className="sp-direct-person-arrow" aria-hidden="true">→</span>
      </Button></li>;
    })}</ul>
  </section>;
}
