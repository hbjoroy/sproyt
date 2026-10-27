import { DestinationPage } from "./workspace-page";
import { Button, Status } from "@sproyt/ui/react";
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import type { CircleChatAgent, CircleChatAgentApi, ChannelChatAgents } from "../../chat-agents";
import type { Channel } from "../../types";
import type { AgentSettingsPanel, ManagementTarget } from "../../application/management-target";
import { AgentHandoff } from "./agent-handoff";
import { ChannelChatAgentsPanel } from "./channel-chat-agents";
import { AgentSettingsDrafts, type AgentEditorPart } from "./agent-settings-drafts";
import { AgentSettingsEditor, agentPartTitles, type AgentCatalogue } from "./agent-settings-editor";
import { SettingsContext, SettingsRow, SettingsSection, SettingsSummary } from "./settings-presentation";

type AgentTarget = Extract<ManagementTarget, { kind: "chat-agents" }>;
const errorText = (cause: unknown) => cause instanceof Error ? cause.message : "Kunne ikkje hente oppsettet.";
const editorParts = Object.keys(agentPartTitles) as AgentEditorPart[];

/** Read one circle list, then details for the addressed agent. Channel access
 * is fetched for one selected channel, never as an agent/channel matrix. */
export function CircleChatAgentsDialog({ api, circleId, circleName, channels = [], target, onNavigate, onClose,
  drafts: providedDrafts, ownerId, admitted = () => true, role = "owner", preferredChannelId }: {
  api: CircleChatAgentApi; circleId: string; circleName: string; channels?: readonly Channel[];
  target?: AgentTarget; onNavigate?: (target: AgentTarget) => void; onClose(): void;
  drafts?: AgentSettingsDrafts; ownerId?: string; admitted?: () => boolean; role?: string; preferredChannelId?: string;
}) {
  const [localTarget, setLocalTarget] = useState<AgentTarget>({ kind: "chat-agents", circleId });
  const [channelChoiceTouched, setChannelChoiceTouched] = useState(false);
  const [localDrafts] = useState(() => new AgentSettingsDrafts());
  const drafts = providedDrafts ?? localDrafts;
  useSyncExternalStore(drafts.subscribe, drafts.getSnapshot);
  const view = target ?? localTarget, panel = view.panel ?? "overview";
  const key = JSON.stringify(view), viewRef = useRef(key); viewRef.current = key;
  const guard = useRef(admitted); guard.current = admitted;
  const live = useRef(true);
  const scopedApi = useMemo(() => ownerId ? api.forOwner(ownerId, () => guard.current()) : api, [api, ownerId, circleId]);
  const [loadedCatalogue, setCatalogue] = useState<AgentCatalogue | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [actionError, setActionError] = useState<{ agentId: string; message: string } | null>(null);
  const reads = useRef(0);
  const mutation = drafts.successEpoch;
  const catalogue = loadedCatalogue && { ...loadedCatalogue, agents: loadedCatalogue.agents.map(agent => {
    const confirmed = drafts.get(circleId, agent.agentId)?.baseline;
    return confirmed && confirmed.revision > agent.revision ? confirmed : agent;
  }) };
  const navigate = (next: AgentTarget) => { if (!guard.current()) return; if (!target) setLocalTarget(next); onNavigate?.(next); };
  const updateAgent = (saved: CircleChatAgent) => setCatalogue(previous => previous ? { ...previous,
    agents: [...previous.agents.filter(agent => agent.agentId !== saved.agentId), saved].sort((a, b) => a.displayName.localeCompare(b.displayName)) } : previous);
  const refresh = async () => {
    const read = ++reads.current;
    const write = drafts.writeEpoch;
    if (live.current) { setLoading(true); setError(""); }
    try {
      const latest = await scopedApi.list(circleId);
      if (!guard.current() || read !== reads.current || write !== drafts.writeEpoch) return;
      if (latest.agents.some(agent => agent.circleId !== circleId)) throw new Error("Agentlista høyrer ikkje til denne kretsen.");
      for (const agent of latest.agents) drafts.confirmed(circleId, agent);
      if (live.current) { setCatalogue(latest); setActionError(null); }
    } catch (cause) {
      if (guard.current() && live.current && read === reads.current) { setCatalogue(null); setError(errorText(cause)); }
    } finally { if (live.current && read === reads.current) setLoading(false); }
  };
  useEffect(() => {
    live.current = true; void refresh();
    return () => { live.current = false; reads.current++; };
  }, [scopedApi, circleId, mutation]);

  const managedChannels = channels.filter(channel => channel.circle_id === circleId && !channel.is_direct
    && (["owner", "moderator"].includes(channel.role) || (channel.kind !== "private" && channel.role === "member")));
  const selectedChannel = managedChannels.find(channel => channel.id === (view.channelId ?? (!channelChoiceTouched ? preferredChannelId : undefined)));
  const agent = catalogue?.agents.find(agent => agent.agentId === view.agentId) ?? null;
  const draft = view.agentId ? drafts.get(circleId, view.agentId) : undefined;
  const pending = !!draft?.pending || !!draft?.handoff?.pending;
  const openAgent = (agentId: string, next: AgentSettingsPanel = "overview") => {
    if (agentId === "new" && drafts.get(circleId, "new")?.baseline) drafts.remove(circleId, "new");
    const channelId = selectedChannel?.id;
    navigate({ kind: "chat-agents", circleId, agentId, panel: next, ...(channelId ? { channelId } : {}) });
  };
  const agentList = (): AgentTarget => ({ kind: "chat-agents", circleId, ...(selectedChannel ? { channelId: selectedChannel.id } : {}) });
  const openPart = (next: AgentSettingsPanel) => navigate({ ...view, panel: next });
  const chooseChannel = (channelId: string) => { setChannelChoiceTouched(true); navigate({ ...view, channelId: channelId || undefined }); };
  const canActivate = (item: CircleChatAgent) => !!catalogue?.workerAvailable && item.workerAvailable
    && (!item.weather || catalogue.weatherAvailable) && (!item.seaTemperature || catalogue.seaTemperatureAvailable)
    && (!item.ferryPort || catalogue.ferryAvailable) && (!item.visionEnabled || item.visionAvailable)
    && (!item.imageGeneration?.enabled || item.imageGenerationAvailable);
  const toggle = async (item: CircleChatAgent) => {
    if (!guard.current() || pending || draft?.needsRead || draft?.conflicts.length) return;
    const expected = drafts.get(circleId, item.agentId);
    drafts.pending(circleId, item.agentId); setActionError(null);
    try {
      const saved = await scopedApi.update(circleId, item.agentId, { displayName: item.displayName,
        triggerWords: item.triggerWords, responsePhrases: item.responsePhrases, enabled: !item.enabled, revision: item.revision });
      if (!guard.current()) return;
      if (saved.circleId !== circleId || saved.agentId !== item.agentId) throw new Error("Tenaren stadfesta ikkje den venta agenten. Hent oppsettet på nytt.");
      if (!drafts.confirmed(circleId, saved, "activation", expected)) return;
      if (live.current) updateAgent(saved);
    } catch (cause) {
      if (guard.current()) {
        drafts.failed(circleId, item.agentId, errorText(cause), expected);
        if (live.current) setActionError({ agentId: item.agentId, message: errorText(cause) });
      }
    }
  };
  const skillDetail = (item: CircleChatAgent, part: AgentEditorPart) => {
    switch (part) {
      case "memory": return item.memoryEnabled ? "Tillate for agenten · kvar brukar vel eige samtykke" : "Av";
      case "vision": return item.visionEnabled ? item.visionAvailable ? "På" : "På · bilettolking er ikkje tilgjengeleg" : "Av";
      case "images": return item.imageGeneration?.enabled ? item.imageGenerationAvailable ? "På" : "På · biletskaping er ikkje tilgjengeleg" : "Av";
      case "weather": return item.weather ? `${item.weather.location}${catalogue?.weatherAvailable ? "" : " · manglar tenesteoppsett"}` : "Ingen fast stad";
      case "sea": return item.seaTemperature ? `${item.seaTemperature.locations.length} stader${catalogue?.seaTemperatureAvailable ? "" : " · manglar tenesteoppsett"}` : "Ingen stader valde";
      case "ferry": return item.ferryPort ? `Parikia, Paros${catalogue?.ferryAvailable ? "" : " · manglar tenesteoppsett"}` : "Av";
      default: return "Namn, triggerfrasar og svarføringar";
    }
  };
  const channelPicker = <div className="sp-agent-channel-picker">
    <label htmlFor="agent-settings-channel">Kanal</label><select id="agent-settings-channel" value={selectedChannel?.id ?? ""} onChange={event => chooseChannel(event.target.value)}>
      <option value="">Vel kanal</option>
      {managedChannels.map(channel => <option key={channel.id} value={channel.id}>{channel.name}{channel.kind === "private" ? " (privat)" : ""}</option>)}
    </select>
    {!managedChannels.length && <p>Ingen kanalar du kan administrere. Private kanalar krev eiga kanalrolle.</p>}
    {view.channelId && !selectedChannel && <Status>Du kan ikkje vise eller endre agenttilgang til den valde kanalen.</Status>}
  </div>;

  let content;
  if (loading && !catalogue) content = <Status>Lastar agentar …</Status>;
  else if (!catalogue) content = <><Status tone="error">{error || "Oppsettet er ikkje tilgjengeleg."}</Status><Button onClick={() => void refresh()}>Hent agentane på nytt</Button></>;
  else if (view.agentId === "new" && draft?.baseline) content = <><Status>Agenten {draft.baseline.displayName} er oppretta. Ingen ny agent blir oppretta frå dette utkastet.</Status><Button onClick={() => openAgent(draft.baseline!.agentId)}>Opne agenten</Button><Button onClick={() => navigate(agentList())}>Til agentlista</Button></>;
  else if (view.agentId === "new") content = <AgentSettingsEditor key="new" api={scopedApi} circleId={circleId} agentId="new" agent={null} part="behaviour"
    catalogue={catalogue} drafts={drafts} admitted={() => guard.current()} onBack={onClose}
    onReload={async () => { await refresh(); if (guard.current() && live.current && viewRef.current === key) navigate(agentList()); }}
    onSaved={saved => { if (live.current) updateAgent(saved); if (live.current && viewRef.current === key) openAgent(saved.agentId); }} />;
  else if (view.agentId && !agent) content = <><Status>Agenten finst ikkje her, eller du har ikkje tilgang til han.</Status><Button onClick={() => navigate(agentList())}>Til agentlista</Button></>;
  else if (agent && editorParts.includes(panel as AgentEditorPart)) content = <AgentSettingsEditor key={`${agent.agentId}:${panel}`} api={scopedApi}
    circleId={circleId} agentId={agent.agentId} agent={agent} part={panel as AgentEditorPart} catalogue={catalogue} drafts={drafts}
    admitted={() => guard.current()} onBack={onClose} onReload={refresh}
    onSaved={saved => { if (live.current) updateAgent(saved); if (live.current && viewRef.current === key) openPart("overview"); }} />;
  else if (agent && panel === "handoff") {
    let entry;
    try { entry = drafts.start(circleId, agent.agentId, agent); } catch { /* Report a bounded workspace, rather than discard another draft. */ }
    content = entry ? <AgentHandoff key={agent.agentId} api={scopedApi} circleId={circleId} source={agent} agents={catalogue.agents}
      initialDraft={entry.handoff} admitted={() => guard.current()} onClose={onClose}
      onDraftChange={(value, expected) => {
        if (!guard.current()) return false;
        const current = drafts.get(circleId, agent.agentId);
        if (!current || (expected && current.handoff !== expected)) return false;
        if (!drafts.handoff(circleId, agent.agentId, value, expected)) return false;
        if (value.policy && value.policy.agentRevision !== current.baseline?.revision) drafts.requireRead(circleId, agent.agentId);
        return true;
      }} onSaved={async () => {
        drafts.requireRead(circleId, agent.agentId); await refresh();
        if (guard.current() && live.current && viewRef.current === key) openPart("overview");
      }} /> : <Status>Kunne ikkje opne utkastet. Fullfør eit anna uferdig agentutkast først.</Status>;
  } else if (panel === "channels") content = <SettingsSection title="Kanaltilgang" description="Dette endrar kva agentar som får lese og svare i den valde kanalen.">
    {channelPicker}
    {selectedChannel && <ChannelChatAgentsPanel key={selectedChannel.id} api={scopedApi} channelId={selectedChannel.id}
      privateChannel={selectedChannel.kind === "private"} agentId={agent?.agentId} />}
  </SettingsSection>;
  else if (agent && panel === "skills") content = <SettingsSection title="Ferdigheiter" description="Opne éi ferdigheit om gongen. Lagring endrar berre den delen du arbeider med.">
    <div className="sp-agent-skill-list">{editorParts.filter(part => part !== "behaviour").map(part => <SettingsRow key={part} label={agentPartTitles[part]}
      detail={skillDetail(agent, part)} onClick={() => openPart(part)} />)}</div>
  </SettingsSection>;
  else if (agent) {
    let entry;
    try { entry = drafts.start(circleId, agent.agentId, agent); } catch { /* Reading remains possible when the draft workspace is full. */ }
    content = <>
      <SettingsSection title="Aktivering">
        <div className="sp-agent-activation"><span>I kretsen: <strong>{agent.enabled ? "På" : "Avslått"}</strong></span>
          <Button variant="quiet" aria-label={`${agent.enabled ? "Slå av" : "Slå på"} ${agent.displayName}`} aria-pressed={agent.enabled}
            disabled={pending || !entry || !!entry.needsRead || !!entry.conflicts.length || (!agent.enabled && !canActivate(agent))} onClick={() => void toggle(agent)}>{agent.enabled ? "Slå av" : "Slå på"}</Button>
        </div>
        {!agent.enabled && !canActivate(agent) && <p className="sp-agent-status-explanation">Agenten kan ikkje aktiverast med dette oppsettet enno. Kontroller modelloppsett og ferdigheiter nedanfor.</p>}
        {actionError?.agentId === agent.agentId && <Status tone="error">{actionError.message}</Status>}
        {entry?.needsRead && <><p>Ei tidlegare endring må kontrollerast før du lagrar meir.</p><Button disabled={pending || loading} onClick={() => void refresh()}>Hent lagra oppsett</Button></>}
      </SettingsSection>
      <SettingsSection title="Kan agenten svare her?">
        {channelPicker}
        {selectedChannel ? <AgentChannelStatus key={`${agent.agentId}:${selectedChannel.id}:${agent.revision}`} api={scopedApi} channel={selectedChannel} agent={agent}
          modelConfigured={catalogue.workerAvailable} admitted={() => guard.current()} onAccess={() => openPart("channels")} />
          : <p className="sp-agent-status-explanation">Vel ein kanal for å kontrollere tilgangen. Ingen kanalval blir endra av å opne denne sida.</p>}
      </SettingsSection>
      <SettingsSection title="Oppsett">
        <SettingsRow label="Oppførsel" detail={`@${agent.displayName} · ${agent.triggerWords.length} triggerfrasar`} onClick={() => openPart("behaviour")} />
        <SettingsRow label="Samarbeid" detail="Videresending til andre agentar" onClick={() => openPart("handoff")} />
      </SettingsSection>
      <SettingsSection title="Ferdigheiter" description="Vel ei ferdigheit for å endre oppsettet for heile kretsen.">
        <div className="sp-agent-skill-shortcuts">{editorParts.filter(part => part !== "behaviour").map(part => <SettingsRow key={part}
          label={agentPartTitles[part]} detail={skillDetail(agent, part)} onClick={() => openPart(part)} />)}</div>
      </SettingsSection>
      {drafts.isDirty(circleId, agent.agentId) && <Status>Du har eit uferdig utkast for denne agenten. Det blir teke vare på medan du navigerer i appen.</Status>}
    </>;
  } else content = <>
    <p className="sp-agent-status-explanation">Vel ein agent for oppsett og kanaltilgang. Ein agent må både vere på i kretsen og ha tilgang til kanalen.</p>
    {!catalogue.workerAvailable && <Status>Modelloppsett er ikkje aktivert i dette miljøet. Du kan framleis sjå og lagre agentoppsett.</Status>}
    <SettingsSection title="Agentar">
      {!catalogue.agents.length && <p>Ingen agentar her enno. Opprett ein agent med namn og nokre korte føringar.</p>}
      <div className="sp-agent-overview-list">{catalogue.agents.map(item => <SettingsRow key={item.agentId} label={item.displayName}
        detail={`${item.enabled ? "På i kretsen" : "Avslått – svarar ikkje"}${drafts.isDirty(circleId, item.agentId) ? " · uferdig utkast" : ""}`} onClick={() => openAgent(item.agentId)} />)}</div>
      <Button onClick={() => openAgent("new", "behaviour")}>Ny agent</Button>
    </SettingsSection>
    <SettingsRow label="Kanaltilgang" detail="Vel kanal og kontroller kva agentar som har tilgang" onClick={() => navigate({ kind: "chat-agents", circleId, panel: "channels",
      ...(selectedChannel ? { channelId: selectedChannel.id } : {}) })} />
  </>;
  return <DestinationPage open title={`Agentar i ${circleName}`} closeLabel="Lukk agentar" onClose={onClose}>
    <div className="sp-agent-settings">
      <SettingsContext scope={`${circleName} / Agentar${agent ? ` / ${agent.displayName}` : ""}`} role={role === "owner" ? "Eigar" : "Moderator"} />
      {view.agentId && <Button className="sp-agent-list-return" variant="quiet" aria-label="Til agentlista" onClick={() => navigate(agentList())}>← Agentar</Button>}
      {agent && <h3 className="sp-agent-detail-heading">{agent.displayName}</h3>}
      {agent && <p className="sp-agent-status-explanation">Oppsettet for {agent.displayName} gjeld heile {circleName}. Kanaltilgang gjeld berre den valde kanalen.</p>}
      {content}
    </div>
  </DestinationPage>;
}

function AgentChannelStatus({ api, channel, agent, modelConfigured, admitted, onAccess }: {
  api: CircleChatAgentApi; channel: Channel; agent: CircleChatAgent; modelConfigured: boolean; admitted(): boolean; onAccess(): void;
}) {
  const [state, setState] = useState<ChannelChatAgents | null>(null);
  const [error, setError] = useState("");
  const [reload, setReload] = useState(0);
  const guard = useRef(admitted); guard.current = admitted;
  useEffect(() => {
    let live = true; setState(null); setError("");
    void api.listChannel(channel.id).then(value => { if (live && guard.current()) setState(value); })
      .catch(cause => { if (live && guard.current()) setError(errorText(cause)); });
    return () => { live = false; };
  }, [api, channel.id, reload]);
  const choice = state?.agents.find(item => item.agentId === agent.agentId);
  let status = "Kontrollerer kanaltilgang …";
  if (error) status = "Status ukjend";
  else if (state && !state.selectionAvailable) status = "Status ukjend – kanalval er ikkje aktivert";
  else if (state && !modelConfigured) status = "Modelloppsett er ikkje aktivert";
  else if (state && !agent.enabled) status = "Avslått i kretsen";
  else if (state && !choice) status = "Agenten er ikkje tilgjengeleg i denne kanalen";
  else if (state && !choice?.enabled) status = "Ingen kanaltilgang";
  else if (state) status = agent.workerAvailable ? "Tilgjengeleg her" : "Kanaltilgang er på – kontroller ferdigheitsoppsettet";
  return <>
    <SettingsSummary label={channel.name} value={status} tone={error || (state && !choice?.enabled) ? "warning" : "neutral"} />
    <p className="sp-agent-status-explanation">Statusen viser oppsett og tilgang. Triggerfrasar og samtalekontekst avgjer om agenten svarar.</p>
    {error && <><Status tone="error">Kunne ikkje kontrollere kanaltilgangen.</Status><Button onClick={() => setReload(value => value + 1)}>Prøv tilgangskontrollen igjen</Button></>}
    <SettingsRow label="Endre kanaltilgang" detail={`Agentval for ${channel.name}`} onClick={onAccess} />
  </>;
}
