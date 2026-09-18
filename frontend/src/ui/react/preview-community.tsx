import { Button, Dialog, PersonList, Status, TextField } from "@sproyt/ui/react";
import { useEffect, useState, type ReactNode } from "react";
import type { Channel, Circle, UserProfile } from "../../types";
import type { ConversationSnapshot } from "../../application/conversation-snapshot";
import { MarkdownContent } from "./markdown-content";

export type CommunityDestination = { kind: "people" | "create-circle" | "circles" | "global-channels" | "global-invite" } | { kind: "channel"; channelId: string } | { kind: "channels" | "invite"; circleId: string };
export interface CommunityHost {
  selfId(): string | null;
  users(): Promise<UserProfile[]>;
  members(channelId: string): Promise<UserProfile[]>;
  circleMembers(circleId: string): Promise<UserProfile[]>;
  openDirect(userId: string): Promise<void>;
  createCircle(name: string): Promise<void>;
  createChannel(circleId: string | null, name: string, kind: "public" | "local" | "private"): Promise<void>;
  joinable(circleId: string): Promise<Array<{ id: string; name: string; description: string }>>;
  join(channelId: string): Promise<void>;
  leaveChannel(channelId: string): Promise<void>;
  leaveCircle(circleId: string): Promise<void>;
  deleteCircle(circleId: string): Promise<void>;
  description(channelId: string, description: string): Promise<void>;
  addMember(channelId: string, userId: string): Promise<void>;
  invite(circleId: string, channelId?: string, userId?: string): Promise<string>;
  accept(value: string): Promise<void>;
  enroll(circleId: string, email: string, name: string): Promise<{ url: string; expiresAt: string }>;
  enrollGlobal(email: string, name: string): Promise<{ url: string; expiresAt: string }>;
  renderIntegration?: (channelId: string) => ReactNode;
}

function useOperation() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const run = async (action: () => Promise<unknown>, success = "") => {
    if (busy) return;
    setBusy(true); setError(""); setNotice("");
    try { await action(); setNotice(success); } catch (error) { setError(error instanceof Error ? error.message : "Handlinga feila. Prøv igjen."); }
    finally { setBusy(false); }
  };
  return { busy, run, feedback: <>{error && <Status tone="error">{error}</Status>}{notice && <Status>{notice}</Status>}</> };
}

function Markdown({ text }: { text: string }) {
  return <MarkdownContent source={text} />;
}

function People({ host, channel, onClose }: { host: CommunityHost; channel?: Readonly<Channel>; onClose(): void }) {
  const [people, setPeople] = useState<UserProfile[]>([]);
  const [eligible, setEligible] = useState<UserProfile[]>([]);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState("");
  const [description, setDescription] = useState(channel?.description ?? "");
  const [loaded, setLoaded] = useState(false);
  const [confirmLeave, setConfirmLeave] = useState(false);
  const op = useOperation();
  const load = () => op.run(async () => {
    const members = channel ? await host.members(channel.id) : await host.users();
    setPeople(members); setLoaded(true);
    if (channel && ["owner", "moderator"].includes(channel.role)) {
      const candidates = channel.circle_id ? await host.circleMembers(channel.circle_id) : await host.users();
      setEligible(candidates.filter(person => person.id !== host.selfId() && !members.some(member => member.id === person.id)));
    }
  });
  useEffect(() => { void load(); }, []);
  const visible = people.filter(person => (channel || person.id !== host.selfId())
    && `${person.display_name} ${person.handle ?? ""}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  return <>
    {channel && <section aria-label="Kanalomtale">
      <h3>Om {channel.name}</h3>
      {channel.description ? <Markdown text={channel.description} /> : <p>Ingen kanalomtale enno.</p>}
      {channel.role === "owner" && <form onSubmit={event => { event.preventDefault(); void op.run(() => host.description(channel.id, description), "Omtalen er lagra."); }}>
        <label htmlFor="community-description">Kanalomtale (Markdown)</label>
        <textarea id="community-description" value={description} onChange={event => setDescription(event.target.value)} rows={4} />
        <Button type="submit" busy={op.busy}>Lagre omtale</Button>
      </form>}
    </section>}
    <TextField label={channel ? "Finn kanalmedlem" : "Finn person"} type="search" value={query} onChange={event => setQuery(event.target.value)} />
    <Button onClick={() => void load()} busy={op.busy}>Last personlista på nytt</Button>
    {loaded && <PersonList people={visible.map(person => ({ id: person.id, name: person.display_name, detail: [person.handle ? `@${person.handle}` : "", person.status_emoji, person.status_text].filter(Boolean).join(" · ") }))}
      emptyLabel="Ingen personar passar søket." actions={person => person.id === host.selfId() ? null
        : <Button className="sp-community-direct" variant="quiet" aria-label={`Start samtale med ${person.name}`} title={`Start samtale med ${person.name}`}
          disabled={op.busy} onClick={() => void op.run(async () => { await host.openDirect(person.id); onClose(); })}><span aria-hidden="true">→</span></Button>} />}
    {channel && ["owner", "moderator"].includes(channel.role) && <section aria-label="Legg til kanalmedlem">
      <h3>Legg til kanalmedlem</h3>
      <label htmlFor="community-member">Vel person</label>
      <select id="community-member" value={selected} onChange={event => setSelected(event.target.value)}><option value="">Vel person</option>{eligible.map(person => <option key={person.id} value={person.id}>{person.display_name}</option>)}</select>
      <Button disabled={!selected || op.busy} onClick={() => void op.run(async () => { await host.addMember(channel.id, selected); setPeople(await host.members(channel.id)); setEligible(eligible.filter(person => person.id !== selected)); setSelected(""); }, "Personen er lagd til.")}>Legg til</Button>
      {channel.circle_id && <Button disabled={!selected || op.busy} onClick={() => void op.run(() => host.invite(channel.circle_id!, channel.id, selected), "Invitasjonen er sendt i direktemelding.")}>Inviter i direktemelding</Button>}
    </section>}
    {channel && !channel.is_direct && <section>
      <Button variant="danger" disabled={op.busy} onClick={() => setConfirmLeave(true)}>Forlat kanalen</Button>
      {confirmLeave && <div role="group" aria-label="Stadfest at du vil forlate kanalen"><p>Vil du forlate {channel.name}? Meldingane dine blir ståande.</p><Button onClick={() => setConfirmLeave(false)}>Avbryt</Button><Button variant="danger" disabled={op.busy} onClick={() => void op.run(async () => { await host.leaveChannel(channel.id); onClose(); })}>Ja, forlat kanalen</Button></div>}
    </section>}
    {channel && !channel.is_direct && ["owner", "moderator"].includes(channel.role) && host.renderIntegration?.(channel.id)}
    {op.feedback}
  </>;
}

function CreateCircle({ host, onClose }: { host: CommunityHost; onClose(): void }) {
  const [name, setName] = useState(""); const op = useOperation();
  return <form onSubmit={event => { event.preventDefault(); void op.run(async () => { await host.createCircle(name.trim()); onClose(); }); }}>
    <p>Prat blir laga automatisk.</p>
    <TextField label="Namn på vennekrets" value={name} required minLength={2} onChange={event => setName(event.target.value)} />
    <Button type="submit" busy={op.busy} disabled={name.trim().length < 2}>Opprett vennekrets</Button>{op.feedback}
  </form>;
}

function ScopeChannels({ host, circle, channels, onClose, onNavigate }: {
  host: CommunityHost; circle?: Readonly<Circle>; channels: readonly Readonly<Channel>[]; onClose(): void; onNavigate(destination: CommunityDestination): void;
}) {
  const [name, setName] = useState(""); const [kind, setKind] = useState<"public" | "local" | "private">("private");
  const [joinable, setJoinable] = useState<Array<{ id: string; name: string; description: string }>>([]);
  const [confirm, setConfirm] = useState(false); const op = useOperation();
  const load = () => circle && op.run(async () => setJoinable(await host.joinable(circle.id)));
  useEffect(() => { if (circle) void load(); }, [circle?.id]);
  const scopeName = circle?.name ?? "Felles";
  return <>
    <h3>Kanalar du er med i</h3><div className="sp-community-channel-list">{channels.filter(channel => channel.circle_id === (circle?.id ?? null)).map(channel => <Button key={channel.id} variant="quiet" onClick={() => onNavigate({ kind: "channel", channelId: channel.id })}>
      <span>{channel.is_direct ? channel.name : `# ${channel.name}`}</span><span>Vis medlemmer</span>
    </Button>)}</div>
    <section aria-label="Ny kanal"><h3>Ny kanal i {scopeName}</h3><form onSubmit={event => { event.preventDefault(); void op.run(async () => { await host.createChannel(circle?.id ?? null, name.trim(), kind); onClose(); }); }}>
      <TextField label="Kanalnamn" value={name} required onChange={event => setName(event.target.value)} />
      <label htmlFor="community-kind">Kanaltype</label><select id="community-kind" value={kind} onChange={event => setKind(event.target.value as typeof kind)}><option value="private">Privat</option><option value="public">Open</option><option value="local">Lokal</option></select>
      <Button type="submit" busy={op.busy} disabled={!name.trim()}>Opprett kanal</Button>
    </form></section>
    {!circle && <section aria-label="Registreringsinvitasjon til Sprøyt"><h3>Inviter ny brukar til Sprøyt</h3>
      <Button onClick={() => onNavigate({ kind: "global-invite" })}>Lag registreringsinvitasjon</Button>
    </section>}
    {circle && <><h3>Finn opne kanalar</h3><Button onClick={() => void load()} busy={op.busy}>Last kanalar på nytt</Button>
      {!joinable.length && <p>Ingen fleire opne kanalar akkurat no.</p>}
      {joinable.map(channel => <section key={channel.id}><h4>{channel.name}</h4><Markdown text={channel.description} /><Button disabled={op.busy} onClick={() => void op.run(async () => { await host.join(channel.id); onClose(); })}>Bli med i {channel.name}</Button></section>)}
      <Button variant="danger" disabled={op.busy} onClick={() => setConfirm(true)}>{circle.role === "owner" ? "Slett vennekrets" : "Forlat vennekrets"}</Button>
      {confirm && <div role="group" aria-label="Stadfest kretsendring"><p>{circle.role === "owner" ? `Slett ${circle.name} og all chat- og prosesshistorikk permanent?` : `Forlat ${circle.name}? Du mistar tilgang til kanalane i kretsen.`}</p><Button onClick={() => setConfirm(false)}>Avbryt</Button><Button variant="danger" disabled={op.busy} onClick={() => void op.run(async () => { await (circle.role === "owner" ? host.deleteCircle(circle.id) : host.leaveCircle(circle.id)); onClose(); })}>Ja, {circle.role === "owner" ? "slett kretsen" : "forlat kretsen"}</Button></div>}</>}
    {op.feedback}
  </>;
}

function CircleAdmin({ host, circles, onSelect }: { host: CommunityHost; circles: readonly Readonly<Circle>[]; onSelect(circleId: string): void }) {
  const [token, setToken] = useState(""); const op = useOperation();
  return <><form onSubmit={event => { event.preventDefault(); void op.run(async () => { await host.accept(token); setToken(""); }, "Invitasjonen er godteken. Samtalane blir lasta inn."); }}>
    <TextField label="Invitasjonskode eller lenkje" value={token} onChange={event => setToken(event.target.value)} required />
    <Button type="submit" busy={op.busy} disabled={!token.trim()}>Godta invitasjon</Button>
  </form>{circles.map(circle => <section key={circle.id}><h3>{circle.name}</h3><Button onClick={() => onSelect(circle.id)}>Administrer {circle.name}</Button></section>)}{op.feedback}</>;
}

function Invite({ host, circle }: { host: CommunityHost; circle: Readonly<Circle> }) {
  const [people, setPeople] = useState<UserProfile[]>([]); const [query, setQuery] = useState("");
  const [email, setEmail] = useState(""); const [name, setName] = useState(""); const [url, setUrl] = useState(""); const [expires, setExpires] = useState("");
  const op = useOperation();
  const load = () => op.run(async () => {
    const [users, members] = await Promise.all([host.users(), host.circleMembers(circle.id)]);
    setPeople(users.filter(person => person.id !== host.selfId() && !members.some(member => member.id === person.id)));
  });
  useEffect(() => { void load(); }, [circle.id]);
  if (circle.role !== "owner") return <Status>Berre eigaren kan invitere til vennekretsen.</Status>;
  return <>
    <h3>Inviter ein person som er på Sprøyt</h3>
    <TextField label="Finn person å invitere" type="search" value={query} onChange={event => setQuery(event.target.value)} />
    <Button busy={op.busy} onClick={() => void load()}>Last personlista på nytt</Button>
    <PersonList people={people.filter(person => `${person.display_name} ${person.handle ?? ""}`.toLocaleLowerCase().includes(query.toLocaleLowerCase())).map(person => ({ id: person.id, name: person.display_name }))} emptyLabel="Ingen nye personar passar søket." actions={person => <Button disabled={op.busy} onClick={() => void op.run(() => host.invite(circle.id, undefined, person.id), `Invitasjonen til ${person.name} er sendt i direktemelding.`)}>Inviter {person.name}</Button>} />
    <h3>Del invitasjonslenkje</h3><Button busy={op.busy} onClick={() => void op.run(async () => { setUrl(await host.invite(circle.id)); setExpires(""); }, "Invitasjonslenkja er klar.")}>Lag kretslenkje</Button>
    <h3>Inviter ein ny brukar</h3><form onSubmit={event => { event.preventDefault(); void op.run(async () => { const result = await host.enroll(circle.id, email.trim(), name.trim()); setUrl(result.url); setExpires(result.expiresAt); }, "Registreringsinvitasjonen er sendt på e-post."); }}>
      <TextField label="E-postadresse" type="email" required value={email} onChange={event => setEmail(event.target.value)} />
      <TextField label="Namn på ny brukar" value={name} onChange={event => setName(event.target.value)} />
      <Button type="submit" busy={op.busy}>Send registreringsinvitasjon</Button>
    </form>
    {url && <section aria-label="Invitasjonslenkje"><TextField label="Invitasjonslenkje" readOnly value={url} onFocus={event => event.target.select()} />{expires && <p>Gyldig til {new Date(expires).toLocaleString("nn-NO")}</p>}
      <Button onClick={() => void op.run(() => navigator.clipboard.writeText(url), "Lenkja er kopiert.")}>Kopier lenkje</Button>
      {typeof navigator.share === "function" && <Button onClick={() => void op.run(async () => { try { await navigator.share({ title: `Invitasjon til ${circle.name}`, url }); } catch (error) { if (!(error instanceof DOMException && error.name === "AbortError")) throw error; } })}>Del lenkje</Button>}
    </section>}{op.feedback}
  </>;
}

function GlobalInvite({ host }: { host: CommunityHost }) {
  const [email, setEmail] = useState(""); const [name, setName] = useState(""); const [url, setUrl] = useState(""); const [expires, setExpires] = useState("");
  const op = useOperation();
  return <>
    <p>Invitasjonen gjev tilgang til Sprøyt og Felles.</p>
    <form onSubmit={event => { event.preventDefault(); void op.run(async () => { const result = await host.enrollGlobal(email.trim(), name.trim()); setUrl(result.url); setExpires(result.expiresAt); }, "Registreringsinvitasjonen er sendt på e-post."); }}>
      <TextField label="E-postadresse" type="email" required value={email} onChange={event => setEmail(event.target.value)} />
      <TextField label="Namn på ny brukar" value={name} onChange={event => setName(event.target.value)} />
      <Button type="submit" busy={op.busy}>Send registreringsinvitasjon</Button>
    </form>
    {url && <section aria-label="Invitasjonslenkje"><TextField label="Invitasjonslenkje" readOnly value={url} onFocus={event => event.target.select()} />{expires && <p>Gyldig til {new Date(expires).toLocaleString("nn-NO")}</p>}
      <Button onClick={() => void op.run(() => navigator.clipboard.writeText(url), "Lenkja er kopiert.")}>Kopier lenkje</Button>
      {typeof navigator.share === "function" && <Button onClick={() => void op.run(async () => { try { await navigator.share({ title: "Invitasjon til Sprøyt", url }); } catch (error) { if (!(error instanceof DOMException && error.name === "AbortError")) throw error; } })}>Del lenkje</Button>}
    </section>}{op.feedback}
  </>;
}

export function PreviewCommunity({ destination, snapshot, host, onClose, onNavigate }: {
  destination: CommunityDestination; snapshot: ConversationSnapshot; host: CommunityHost; onClose(): void; onNavigate(destination: CommunityDestination): void;
}) {
  const circle = "circleId" in destination ? snapshot.circles.find(item => item.id === destination.circleId) : undefined;
  const channel = destination.kind === "channel" ? snapshot.channels.find(item => item.id === destination.channelId) : undefined;
  const title = destination.kind === "people" ? "Personar og ny direktemelding" : destination.kind === "channel" ? `Kanaldetaljar: ${channel?.name ?? "Kanal"}` : destination.kind === "create-circle" ? "Ny vennekrets" : destination.kind === "circles" ? "Kretsadministrasjon og invitasjonskode" : destination.kind === "global-channels" ? "Kanalar i Felles" : destination.kind === "global-invite" ? "Inviter ny brukar til Sprøyt" : destination.kind === "channels" ? `Kanalar i ${circle?.name ?? "vennekretsen"}` : `Inviter til ${circle?.name ?? "vennekretsen"}`;
  return <Dialog open title={title} closeLabel="Lukk" onClose={onClose}>
    <div className="sp-community-dialog">
      {destination.kind === "people" && <People host={host} onClose={onClose} />}
      {destination.kind === "channel" && (channel ? <People host={host} channel={channel} onClose={onClose} /> : <Status>Kanalen er ikkje lenger tilgjengeleg.</Status>)}
      {destination.kind === "create-circle" && <CreateCircle host={host} onClose={onClose} />}
      {destination.kind === "circles" && <CircleAdmin host={host} circles={snapshot.circles} onSelect={circleId => onNavigate({ kind: "channels", circleId })} />}
      {destination.kind === "global-channels" && <ScopeChannels host={host} channels={snapshot.channels} onClose={onClose} onNavigate={onNavigate} />}
      {destination.kind === "global-invite" && <GlobalInvite host={host} />}
      {destination.kind === "channels" && circle && <ScopeChannels host={host} circle={circle} channels={snapshot.channels} onClose={onClose} onNavigate={onNavigate} />}
      {destination.kind === "invite" && circle && <Invite host={host} circle={circle} />}
    </div>
  </Dialog>;
}
