import { Button, Dialog, Status, TextField } from "@sproyt/ui/react";
import { useEffect, useLayoutEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { ConversationSnapshot } from "../../application/conversation-snapshot";
import { managementParent, type ManagementTarget } from "../../application/management-target";
import { PreviewSettingsDialog, type PreviewSettingsHost } from "./preview-settings";
import { PreviewCommunity, type CommunityHost } from "./preview-community";
import { PreviewAdvanced } from "./preview-advanced";
import type { AdvancedHost } from "../../application/advanced-host";
import type { CircleChatAgentApi } from "../../chat-agents";
import { CircleChatAgentsDialog } from "./circle-chat-agents";
import { AgentMemoryDialog } from "./agent-memory";
import type { AppUpdate, UpdatePosition } from "../../app-update";
import { AppUpdateAction } from "./app-update-action";

export type { ManagementTarget } from "../../application/management-target";
type SettingsRow = { label: string; detail: string; target: ManagementTarget; aliases?: string };
const directory: ManagementTarget = { kind: "community", destination: { kind: "circles" } };
const administrator = (role: string) => role === "owner" || role === "moderator";
const roleName = (role: string) => role === "owner" ? "Eigar" : role === "moderator" ? "Moderator" : "Medlem";

/** A single controlled destination, with a local fallback for standalone previews.
 * Opening this directory creates no invitation, credential or process. */
export function PreviewManagement({ snapshot, capabilities, settings, community, advanced, chatAgents, appUpdate, captureUpdatePosition, target, onNavigate, onBack, onSelectChannel }: {
  snapshot: ConversationSnapshot;
  capabilities: { agent: boolean; heart: boolean };
  settings: PreviewSettingsHost;
  community: CommunityHost;
  advanced: AdvancedHost;
  chatAgents: CircleChatAgentApi;
  appUpdate: AppUpdate;
  captureUpdatePosition: () => readonly UpdatePosition[];
  target?: ManagementTarget | null;
  onNavigate?: (target: ManagementTarget | null) => void;
  onBack?: () => void;
  onSelectChannel?: (channelId: string) => void;
}) {
  const [localTarget, setLocalTarget] = useState<ManagementTarget | null>(null);
  const selfId = community.selfId();
  const [owner, setOwner] = useState(selfId);
  const [query, setQuery] = useState("");
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const [portalHost, setPortalHost] = useState<Element | null>(null);
  useLayoutEffect(() => { setPortalHost(trigger.current?.closest(".sp-theme") ?? null); }, []);
  const openedFromTrigger = useRef(false);
  const rows = useRef(new Map<string, HTMLButtonElement>());
  const entries = useRef(new Map<string, string>());
  const returnFocus = useRef<string | undefined>(undefined);
  const current = target === undefined ? localTarget : target;
  // Gate immediately: the old account's form must not survive until an effect.
  const active = owner === selfId && selfId ? current : null;
  const navigate = (next: ManagementTarget | null) => {
    if (target === undefined) setLocalTarget(next);
    onNavigate?.(next);
  };
  useEffect(() => {
    if (owner === selfId) return;
    setOwner(selfId); setQuery(""); returnFocus.current = undefined; entries.current.clear();
    if (target === undefined) setLocalTarget(null);
    if (owner !== null) onNavigate?.(null);
  }, [selfId, owner, target === undefined, onNavigate]);
  const activeKey = JSON.stringify(active);
  const previousTarget = useRef(activeKey);
  useEffect(() => {
    const previous = previousTarget.current;
    previousTarget.current = activeKey;
    const frame = requestAnimationFrame(() => {
      const focusKey = returnFocus.current ?? entries.current.get(previous);
      const button = focusKey ? rows.current.get(focusKey) : undefined;
      if (button?.isConnected) { button.focus({ preventScroll: true }); returnFocus.current = undefined; }
      else if (!active && previous !== "null") {
        if (openedFromTrigger.current) trigger.current?.focus({ preventScroll: true });
        openedFromTrigger.current = false;
      }
    });
    return () => cancelAnimationFrame(frame);
  }, [activeKey]);
  const close = () => {
    if (!active) return;
    returnFocus.current = entries.current.get(JSON.stringify(active));
    if (onBack) onBack();
    else navigate(managementParent(active, snapshot));
  };
  const row = ({ label, detail, target: destination }: SettingsRow) => {
    const key = JSON.stringify(destination);
    const entryKey = `${key}:${label}`;
    const detailId = `${id}-${encodeURIComponent(entryKey)}`;
    return <Button key={entryKey} variant="quiet" className="sp-settings-row"
      aria-label={label} aria-describedby={detailId}
      ref={element => { if (element) rows.current.set(entryKey, element); else rows.current.delete(entryKey); }}
      onClick={event => { event.currentTarget.focus({ preventScroll: true }); entries.current.set(key, entryKey); navigate(destination); }}>
      <span><strong>{label}</strong><span id={detailId} className="sp-settings-detail">{detail}</span></span><span aria-hidden="true">›</span>
    </Button>;
  };
  const circle = active && "circleId" in active ? snapshot.circles.find(circle => circle.id === active.circleId) : undefined;
  let available = true;
  if (active && "circleId" in active) {
    available = Boolean(circle) && (active.kind !== "chat-agents" || administrator(circle!.role))
      && (active.kind !== "memory" || Boolean(community.agentMemory));
  } else if (active?.kind === "community") {
    const destination = active.destination;
    if ("circleId" in destination && destination.circleId) {
      const scopedCircle = snapshot.circles.find(circle => circle.id === destination.circleId);
      available = Boolean(scopedCircle) && (destination.kind !== "invite" || administrator(scopedCircle!.role))
        && (destination.kind !== "rename-circle" || scopedCircle!.role === "owner");
    } else if (destination.kind === "channel") {
      available = snapshot.channels.some(channel => channel.id === destination.channelId);
    }
  } else if (active?.kind === "advanced") {
    const channel = snapshot.channels.find(channel => channel.id === active.channelId);
    available = capabilities[active.feature] && Boolean(channel)
      && (active.feature !== "agent" || administrator(channel!.role));
  }
  const isDirectory = active?.kind === "community" && active.destination.kind === "circles";
  const groups: { title: string; rows: SettingsRow[] }[] = [
    { title: "Dine innstillingar", rows: [
      { label: "Profil og status", detail: "Namn og status · kontoen din", target: { kind: "profile" }, aliases: "namn emoji" },
      { label: "Varslingsinnstillingar", detail: "Varsel og push · kontoen din og denne eininga", target: { kind: "notifications" }, aliases: "bjelle varsling push" },
      ...(community.agentMemory ? [{ label: "Personvern og mitt agentminne", detail: "Vel krets for å sjå og styre ditt eige minne", target: directory, aliases: "minne samtykke personvern" }] : [])
    ] },
    { title: "Kretsar og Felles", rows: [
      { label: "Administrer krets", detail: "Vel krets · medlemmer, kanalar, invitasjonar og agentar", target: directory, aliases: "kretsadministrasjon bot agent invitere" },
      { label: "Kanalar i Felles", detail: "Finn og opprett kanalar i fellesområdet", target: { kind: "community", destination: { kind: "global-channels" } } },
      { label: "Inviter ny brukar til Sprøyt", detail: "Registreringsinvitasjon til Sprøyt og Felles", target: { kind: "community", destination: { kind: "global-invite" } }, aliases: "invitere" },
      { label: "Ny vennekrets", detail: "Opprett ein krets for ei gruppe du kjenner", target: { kind: "community", destination: { kind: "create-circle" } } },
      { label: "Bruk invitasjonskode", detail: "Bli med i ein krets med kode eller lenkje", target: directory }
    ] },
    { title: "Samtalar", rows: [
      { label: "Personar og ny direktemelding", detail: "Finn ein person og start ei samtale", target: { kind: "community", destination: { kind: "people" } } },
      ...(snapshot.activeChannel ? [{ label: "Kanaldetaljar, medlemmer og integrasjonar", detail: `${snapshot.activeChannel.circle_id ? snapshot.circles.find(circle => circle.id === snapshot.activeChannel!.circle_id)?.name ?? "Krets" : "Felles"} / ${snapshot.activeChannel.name}`, target: { kind: "community", destination: { kind: "channel", channelId: snapshot.activeChannel.id } } } as SettingsRow] : [])
    ] },
    { title: "Integrasjonar og prosessar", rows: [
      ...(capabilities.agent && snapshot.activeChannel && administrator(snapshot.activeChannel.role) ? [{ label: "Mellombels tilgang for ekstern agent", detail: `Kortliva tilgang til ${snapshot.activeChannel.name}`, target: { kind: "advanced", feature: "agent", channelId: snapshot.activeChannel.id }, aliases: "agenttilgang credential" } as SettingsRow] : []),
      ...(capabilities.heart && snapshot.activeChannel ? [{ label: "Heart og planlegging", detail: `Prosessar i ${snapshot.activeChannel.name}`, target: { kind: "advanced", feature: "heart", channelId: snapshot.activeChannel.id }, aliases: "prosess event" } as SettingsRow] : [])
    ] }
  ];
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const scopedResults: SettingsRow[] = normalizedQuery ? snapshot.circles.flatMap(circle => [
    { label: circle.name, detail: `Krets · ${roleName(circle.role)}`, target: { kind: "circle", circleId: circle.id } } as SettingsRow,
    ...(administrator(circle.role) ? [{ label: `Agentar i ${circle.name}`, detail: `Krets · ${roleName(circle.role)}`, target: { kind: "chat-agents", circleId: circle.id }, aliases: "bot agent" } as SettingsRow,
      { label: `Inviter til ${circle.name}`, detail: "Inviter personar og nye brukarar", target: { kind: "community", destination: { kind: "invite", circleId: circle.id } }, aliases: "invitere" } as SettingsRow] : []),
    ...(community.agentMemory ? [{ label: `Mitt agentminne i ${circle.name}`, detail: "Personleg · berre ditt eige minne", target: { kind: "memory", circleId: circle.id }, aliases: "minne personvern samtykke" } as SettingsRow] : [])
  ]) : [];
  const matches = (item: SettingsRow) => `${item.label} ${item.detail} ${item.aliases ?? ""}`.toLocaleLowerCase().includes(normalizedQuery);
  const matchedGroups = [...groups, { title: "I kretsane dine", rows: scopedResults }].map(group => ({ ...group, rows: group.rows.filter(matches) })).filter(group => group.rows.length);
  return <>
    <Button ref={trigger} onClick={event => {
      event.currentTarget.focus({ preventScroll: true }); openedFromTrigger.current = true; setQuery(""); navigate({ kind: "overview" });
    }}>Innstillingar</Button>
    {portalHost && createPortal(<>
    {active && !available && <Dialog open title="Ikkje tilgjengeleg" closeLabel="Tilbake" onClose={close}>
      <div className="sp-settings-page"><Status>Du har ikkje tilgang til denne sida no. Kretsen, kanalen eller rettane dine kan ha endra seg.</Status><Button onClick={() => navigate({ kind: "overview" })}>Til innstillingar</Button></div>
    </Dialog>}
    {active?.kind === "overview" && <Dialog open title="Innstillingar" closeLabel="Lukk innstillingane" onClose={close}>
      <div className="sp-settings-page">
        <p className="sp-settings-intro">Dine val og administrasjon av kretsane dine. Samtalen og utkasta dine blir tekne vare på.</p>
        <TextField label="Søk i innstillingar" type="search" value={query} onChange={event => setQuery(event.target.value)} placeholder="Til dømes varsel, agent eller kretsnamn" />
        {matchedGroups.map(group => <section className="sp-settings-group" aria-label={group.title} key={group.title}><h3>{group.title}</h3>{group.rows.map(row)}</section>)}
        {!matchedGroups.length && <p role="status">Ingen innstillingar passar søket.</p>}
        {!normalizedQuery && <section className="sp-settings-group" aria-label="Om appen"><h3>Om appen</h3><AppUpdateAction update={appUpdate} capture={captureUpdatePosition} /></section>}
      </div>
    </Dialog>}
    {active?.kind === "circle" && available && circle && <Dialog open title={circle.name} closeLabel="Tilbake til kretsane" onClose={close}>
      <div className="sp-settings-page"><p className="sp-settings-scope">Krets / {circle.name} · {roleName(circle.role)}</p>
        <p>Endringar her gjeld denne kretsen. Mitt agentminne er personleg.</p>
        <section className="sp-settings-group" aria-label="Kretsinnstillingar">
          {row({ label: "Medlemmer og roller", detail: "Sjå medlemmer og kretsroller", target: { kind: "community", destination: { kind: "circle-members", circleId: circle.id } } })}
          {row({ label: "Kanalar og medlemskap", detail: "Finn, opprett og administrer kanalar", target: { kind: "community", destination: { kind: "channels", circleId: circle.id } } })}
          {administrator(circle.role) && row({ label: "Inviter personar og nye brukarar", detail: "Invitasjonar til denne kretsen", target: { kind: "community", destination: { kind: "invite", circleId: circle.id } } })}
          {administrator(circle.role) && row({ label: "Agentar", detail: "Samtaleagentar og kanaltilgang i kretsen", target: { kind: "chat-agents", circleId: circle.id } })}
          {community.agentMemory && row({ label: "Mitt agentminne", detail: "Sjå, rett og slett ditt eige minne", target: { kind: "memory", circleId: circle.id } })}
          {circle.role === "owner" && row({ label: "Endre kretsnamn", detail: "Namnet alle medlemmene ser", target: { kind: "community", destination: { kind: "rename-circle", circleId: circle.id } } })}
        </section>
      </div>
    </Dialog>}
    {isDirectory && available && <CircleDirectory key={selfId} snapshot={snapshot} host={community} onClose={close} row={row} />}
    {active && available && (active.kind === "profile" || active.kind === "notifications") && <PreviewSettingsDialog key={`${selfId}:${active.kind}`} kind={active.kind} host={settings} onClose={close} />}
    {active?.kind === "community" && !isDirectory && available && <PreviewCommunity key={`${selfId}:${activeKey}`} destination={active.destination} snapshot={snapshot} host={community} onSelectChannel={onSelectChannel}
      onNavigate={destination => navigate({ kind: "community", destination })} onClose={close} />}
    {active?.kind === "advanced" && available && <PreviewAdvanced key={`${selfId}:${activeKey}`} kind={active.feature} host={advanced}
      snapshot={{ ...snapshot, activeChannel: snapshot.channels.find(channel => channel.id === active.channelId) ?? null,
        circles: snapshot.circles.filter(circle => circle.id === snapshot.channels.find(channel => channel.id === active.channelId)?.circle_id) }} onClose={close} />}
    {active?.kind === "memory" && available && circle && community.agentMemory && <AgentMemoryDialog key={`${selfId}:${circle.id}`} api={community.agentMemory} circleId={circle.id} circleName={circle.name}
      channels={snapshot.channels.filter(channel => channel.circle_id === circle.id)} onClose={close} />}
    {active?.kind === "chat-agents" && available && circle && <CircleChatAgentsDialog key={`${selfId}:${circle.id}`} api={chatAgents} circleId={circle.id} circleName={circle.name}
      channels={snapshot.channels.filter(channel => channel.circle_id === circle.id)} onClose={close} />}
    </>, portalHost)}
  </>;
}

function CircleDirectory({ snapshot, host, onClose, row }: {
  snapshot: ConversationSnapshot; host: CommunityHost; onClose(): void; row(item: SettingsRow): ReactNode;
}) {
  const [query, setQuery] = useState("");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const live = useRef(true);
  useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);
  const visible = snapshot.circles.filter(circle => circle.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  return <Dialog open title="Vel krets" closeLabel="Tilbake til innstillingar" onClose={onClose}><div className="sp-settings-page">
    <p className="sp-settings-intro">Vel kva krets du vil sjå. Du finn medlemmer, kanalar, agentar og ditt eige agentminne inne i kretsen.</p>
    <TextField label="Finn vennekrets" type="search" value={query} onChange={event => setQuery(event.target.value)} />
    <section className="sp-settings-group" aria-label="Vennekretsar">{visible.map(circle => row({ label: circle.name, detail: `Krets · ${roleName(circle.role)}`, target: { kind: "circle", circleId: circle.id } }))}</section>
    {!visible.length && <p role="status">{snapshot.circles.length ? "Ingen kretsar passar søket." : "Du er ikkje med i ein krets enno. Opprett ein krets eller bruk ein invitasjonskode."}</p>}
    <details className="sp-settings-invitation"><summary>Bruk invitasjonskode</summary>
      <form onSubmit={async event => {
        event.preventDefault(); if (busy) return; setBusy(true); setError(""); setNotice("");
        try { await host.accept(token); if (live.current) { setToken(""); setNotice("Invitasjonen er godteken. Samtalane blir lasta inn."); } }
        catch (error) { if (live.current) setError(error instanceof Error ? error.message : "Kunne ikkje godta invitasjonen. Prøv igjen."); }
        finally { if (live.current) setBusy(false); }
      }}><TextField label="Invitasjonskode eller lenkje" value={token} onChange={event => setToken(event.target.value)} required />
        <Button type="submit" busy={busy} disabled={!token.trim()}>Godta invitasjon</Button>
      </form>
      {error && <Status tone="error">{error}</Status>}{notice && <Status>{notice}</Status>}
    </details>
  </div></Dialog>;
}
