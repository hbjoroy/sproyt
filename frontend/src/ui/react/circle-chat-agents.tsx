import { Button, Dialog, Status, TextField } from "@sproyt/ui/react";
import { useEffect, useState } from "react";
import type { AgentImageGeneration, AgentImageIdentity, AgentSeaTemperatureLocation, CircleChatAgent, CircleChatAgentApi, CircleChatAgentInput } from "../../chat-agents";
import { validAgentSeaTemperature, validAgentWeather } from "../../chat-agents";
import { AgentHandoff } from "./agent-handoff";
import type { Channel } from "../../types";
import { ChannelChatAgentsPanel } from "./channel-chat-agents";

const lines = (value: string) => value.split(/\r?\n/u).map(item => item.trim()).filter(Boolean);
const errorText = (error: unknown) => error instanceof Error ? error.message : "Prøv igjen.";

export function CircleChatAgentsDialog({ api, circleId, circleName, channels = [], onClose }: {
  api: CircleChatAgentApi; circleId: string; circleName: string; channels?: readonly Channel[]; onClose: () => void;
}) {
  const [agents, setAgents] = useState<CircleChatAgent[]>([]);
  const [available, setAvailable] = useState(false);
  const [weatherAvailable, setWeatherAvailable] = useState(false);
  const [seaAvailable, setSeaAvailable] = useState(false);
  const [seaCatalogue, setSeaCatalogue] = useState<AgentSeaTemperatureLocation[]>([]);
  const [seaEnabled, setSeaEnabled] = useState(false);
  const [seaLocations, setSeaLocations] = useState<string[]>([]);
  const [seaDefault, setSeaDefault] = useState("");
  const [ferryAvailable, setFerryAvailable] = useState(false);
  const [ferryEnabled, setFerryEnabled] = useState(false);
  const [visionAvailable, setVisionAvailable] = useState(false);
  const [visionEnabled, setVisionEnabled] = useState(false);
  const [imagesAvailable, setImagesAvailable] = useState(false);
  const [imageIdentities, setImageIdentities] = useState<AgentImageIdentity[]>([]);
  const [imagesEnabled, setImagesEnabled] = useState(false);
  const [imageIdentity, setImageIdentity] = useState("");
  const [imagesOccasional, setImagesOccasional] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [reload, setReload] = useState(0);
  const [channelId, setChannelId] = useState("");
  const [showChannels, setShowChannels] = useState(false);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState<CircleChatAgent | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [handoffAgent, setHandoffAgent] = useState<CircleChatAgent | null>(null);
  const [name, setName] = useState("");
  const [triggers, setTriggers] = useState("");
  const [phrases, setPhrases] = useState("");
  const [enabled, setEnabled] = useState(false);
  const [memoryEnabled, setMemoryEnabled] = useState(false);
  const [weatherEnabled, setWeatherEnabled] = useState(false);
  const [location, setLocation] = useState("Parikia");
  const [latitude, setLatitude] = useState("37.085");
  const [longitude, setLongitude] = useState("25.148");
  const weather = weatherEnabled ? { location: location.trim(), latitude: Number(latitude), longitude: Number(longitude) } : null;
  const weatherValid = !weather || (!!latitude.trim() && !!longitude.trim() && validAgentWeather(weather));
  const seaTemperature = seaEnabled ? { defaultLocation: seaDefault, locations: seaLocations } : null;
  const previousSea = selected?.seaTemperature ?? null;
  const seaUnchanged = seaTemperature === null ? previousSea === null : !!previousSea
    && seaTemperature.defaultLocation === previousSea.defaultLocation
    && seaTemperature.locations.length === previousSea.locations.length
    && seaTemperature.locations.every((id, index) => id === previousSea.locations[index]);
  const seaValid = !seaTemperature || (validAgentSeaTemperature(seaTemperature)
    && (seaUnchanged || seaLocations.every(id => seaCatalogue.some(location => location.id === id))));
  const canUseVision = selected ? selected.visionAvailable : visionAvailable;
  const canUseImages = selected ? selected.imageGenerationAvailable : imagesAvailable;
  const imageGeneration: AgentImageGeneration | null = imageIdentity
    ? { identityId: imageIdentity, enabled: imagesEnabled, occasional: imagesEnabled && imagesOccasional } : null;
  const previousImages = selected?.imageGeneration ?? null;
  const imagesUnchanged = imageGeneration?.identityId === previousImages?.identityId
    && imageGeneration?.enabled === previousImages?.enabled && imageGeneration?.occasional === previousImages?.occasional;
  const imagesValid = !imagesEnabled || (imageIdentities.some(identity => identity.id === imageIdentity) && (canUseImages || imagesUnchanged));
  const workerAvailable = available && (!weatherEnabled || weatherAvailable) && (!seaEnabled || seaAvailable)
    && (!ferryEnabled || ferryAvailable) && (!visionEnabled || canUseVision);

  // This dialog is only opened for circle managers. Private channels still require a channel role.
  const managedChannels = channels.filter(channel => channel.circle_id === circleId && !channel.is_direct
    && (["owner", "moderator"].includes(channel.role) || (channel.kind !== "private" && channel.role === "member")));
  const selectedChannel = managedChannels.find(channel => channel.id === channelId) ?? managedChannels[0];

  useEffect(() => {
    let live = true;
    setLoading(true); setError("");
    void api.list(circleId).then(result => {
      if (!live) return;
      setAgents(result.agents); setAvailable(result.workerAvailable); setWeatherAvailable(result.weatherAvailable); setLoading(false);
      setSeaAvailable(result.seaTemperatureAvailable); setSeaCatalogue(result.seaTemperatureLocations);
      setFerryAvailable(result.ferryAvailable);
      setVisionAvailable(result.visionAvailable);
      setImagesAvailable(result.imageGenerationAvailable); setImageIdentities(result.imageIdentities);
    }).catch(cause => { if (live) { setError(errorText(cause)); setLoading(false); } });
    return () => { live = false; };
  }, [api, circleId, reload]);

  const canActivate = (agent: CircleChatAgent) => available && agent.workerAvailable
    && (!agent.weather || weatherAvailable) && (!agent.seaTemperature || seaAvailable)
    && (!agent.ferryPort || ferryAvailable) && (!agent.visionEnabled || agent.visionAvailable)
    && (!agent.imageGeneration?.enabled || agent.imageGenerationAvailable);
  const toggleAgent = async (agent: CircleChatAgent) => {
    if (saving || formOpen || handoffAgent) return;
    setSaving(true); setError("");
    try {
      // Omit optional settings: toggling must never overwrite skills or memory preferences.
      const saved = await api.update(circleId, agent.agentId, { displayName: agent.displayName,
        triggerWords: agent.triggerWords, responsePhrases: agent.responsePhrases,
        enabled: !agent.enabled, revision: agent.revision });
      setAgents(previous => previous.map(item => item.agentId === saved.agentId ? saved : item));
    } catch (cause) { setError(errorText(cause)); }
    finally { setSaving(false); }
  };

  const edit = (agent: CircleChatAgent | null) => {
    setSelected(agent); setName(agent?.displayName ?? "");
    setTriggers(agent?.triggerWords.join("\n") ?? "");
    setPhrases(agent?.responsePhrases.join("\n") ?? "");
    setWeatherEnabled(!!agent?.weather); setLocation(agent?.weather?.location ?? "Parikia");
    setSeaEnabled(!!agent?.seaTemperature);
    setSeaLocations([...(agent?.seaTemperature?.locations ?? [])]);
    setSeaDefault(agent?.seaTemperature?.defaultLocation ?? "");
    setFerryEnabled(agent?.ferryPort === "paros");
    setVisionEnabled(agent?.visionEnabled ?? false);
    setImagesEnabled(agent?.imageGeneration?.enabled ?? false);
    setImageIdentity(agent?.imageGeneration?.identityId ?? "");
    setImagesOccasional(agent?.imageGeneration?.occasional ?? false);
    setLatitude(String(agent?.weather?.latitude ?? 37.085)); setLongitude(String(agent?.weather?.longitude ?? 25.148));
    setEnabled(agent?.enabled ?? false); setMemoryEnabled(agent?.memoryEnabled ?? false); setError(""); setFormOpen(true);
  };
  const save = async () => {
    if (saving || !weatherValid || !seaValid || !imagesValid) return;
    setError(""); setSaving(true);
    const input: CircleChatAgentInput = { displayName: name.trim(), triggerWords: lines(triggers),
      responsePhrases: lines(phrases), enabled, memoryEnabled, revision: selected?.revision, weather,
      seaTemperature: seaUnchanged ? undefined : seaTemperature,
      ferryPort: ferryEnabled ? "paros" : null,
      visionEnabled: selected && visionEnabled === selected.visionEnabled ? undefined : visionEnabled,
      imageGeneration: imagesUnchanged ? undefined : imageGeneration };
    try {
      const saved = selected ? await api.update(circleId, selected.agentId, input) : await api.create(circleId, input);
      setAgents(previous => [...previous.filter(item => item.agentId !== saved.agentId), saved]
        .sort((a, b) => a.displayName.localeCompare(b.displayName)));
      setFormOpen(false); setSelected(null);
    } catch (cause) { setError(errorText(cause)); }
    finally { setSaving(false); }
  };
  return <Dialog open title={`Agentar i ${circleName}`} closeLabel="Lukk agentar" onClose={onClose}>
    <p>Slå på agenten her, og vel kvar han får svare under Kanaltilgang. Skriv @agentnamnet eller eit triggeruttrykk i ei ny melding for å få svar.</p>
    {!formOpen && !handoffAgent && <div className="sp-agent-tabs" role="group" aria-label="Agentoppsett">
      <Button variant="quiet" aria-pressed={!showChannels} disabled={saving} onClick={() => setShowChannels(false)}>Agentar</Button>
      <Button variant="quiet" aria-pressed={showChannels} disabled={saving} onClick={() => setShowChannels(true)}>Kanaltilgang</Button>
    </div>}
    {showChannels && !formOpen && !handoffAgent ? <section className="sp-agent-channel-access" aria-label="Kanaltilgang">
      <label htmlFor="circle-agent-channel">Kanal</label>
      <select id="circle-agent-channel" value={selectedChannel?.id ?? ""} onChange={event => setChannelId(event.target.value)}>
        {!managedChannels.length && <option value="">Ingen kanalar du kan administrere</option>}
        {managedChannels.map(channel => <option key={channel.id} value={channel.id}>{channel.name}{channel.kind === "private" ? " (privat)" : ""}</option>)}
      </select>
      {selectedChannel ? <ChannelChatAgentsPanel key={selectedChannel.id} api={api} channelId={selectedChannel.id} privateChannel={selectedChannel.kind === "private"} />
        : <Status>Berre kanalar du er medlem i og har rett til å administrere, blir viste her.</Status>}
    </section> : <>
    {!available && <Status tone="error">Modelltenesta for samtaleagentar er ikkje aktiv. Du kan lagre oppsettet, men ikkje slå på ein agent enno.</Status>}
    {loading ? <Status>Lastar agentar …</Status> : <div style={{ display: "grid", gap: 8 }}>
      {agents.length === 0 && <Status>Ingen agentar i denne kretsen enno.</Status>}
      {agents.map(agent => <div key={agent.agentId} className="sp-circle-agent-row" style={{ flexWrap: "wrap" }}>
        <span className="sp-agent-summary"><strong>{agent.displayName}</strong><small>{agent.enabled ? "På i kretsen" : "Avslått – svarar ikkje"}</small>
          <small>Skriv @{agent.displayName} eller {agent.triggerWords.map(word => `«${word}»`).join(", ")}</small>
          {!agent.enabled && !canActivate(agent) && <small>Ei teneste agenten brukar, er ikkje tilgjengeleg no.</small>}
        </span>
        <div style={{ display: "flex", flexWrap: "wrap", gap: ".25rem" }}>
        <Button variant="quiet" aria-label={`${agent.enabled ? "Slå av" : "Slå på"} ${agent.displayName}`} aria-pressed={agent.enabled}
          disabled={loading || saving || formOpen || handoffAgent !== null || (!agent.enabled && !canActivate(agent))}
          onClick={() => void toggleAgent(agent)}>{agent.enabled ? "Slå av" : "Slå på"}</Button>
        <Button variant="quiet" disabled={saving || formOpen || handoffAgent !== null} onClick={() => edit(agent)}>Rediger</Button>
        <Button variant="quiet" disabled={saving || formOpen || handoffAgent !== null} aria-label={`Videresending frå ${agent.displayName}`}
          onClick={() => setHandoffAgent(agent)}>Videresending</Button>
        </div>
      </div>)}
      {!formOpen && !handoffAgent && <Button disabled={saving || loading} onClick={() => edit(null)}>Ny agent</Button>}
    </div>}
    {handoffAgent && <AgentHandoff key={handoffAgent.agentId} api={api} circleId={circleId} source={handoffAgent} agents={agents}
      onClose={() => setHandoffAgent(null)} onSaved={revision => {
        setAgents(previous => previous.map(agent => agent.agentId === handoffAgent.agentId ? { ...agent, revision } : agent));
        setHandoffAgent(null);
      }} />}
    {formOpen && <form className="sp-circle-agent-form" onSubmit={event => { event.preventDefault(); void save(); }}>
      <h3>{selected ? `Rediger ${selected.displayName}` : "Ny agent"}</h3>
      <label className="sp-circle-agent-enabled"><input type="checkbox" checked={enabled}
        disabled={saving || (!workerAvailable && !enabled)} onChange={event => setEnabled(event.target.checked)} />Aktiv</label>
      <p>Aktiv må vere på for at agenten skal svare. Private kanalar krev også eit val under Kanaltilgang.</p>
      <TextField label="Namn" value={name} maxLength={80} onChange={event => setName(event.target.value)} />
      <label htmlFor="circle-agent-triggers">Triggerord eller -frasar, eitt per linje</label>
      <textarea id="circle-agent-triggers" rows={4} value={triggers} onChange={event => setTriggers(event.target.value)} />
      <label htmlFor="circle-agent-phrases">Stikkord og svarføringar, eitt per linje</label>
      <textarea id="circle-agent-phrases" rows={4} value={phrases} onChange={event => setPhrases(event.target.value)} />
      <p>Agenten brukar dette som innhald og tone, ikkje som eit ferdig svar. Skriv gjerne kva han bør vite eller spørje om.</p>
      <label className="sp-circle-agent-enabled"><input type="checkbox" checked={visionEnabled}
        disabled={saving || (!canUseVision && !visionEnabled)} onChange={event => setVisionEnabled(event.target.checked)} />Tolk bilete i meldingar</label>
      {visionEnabled && <p>Agenten kan tolke opptil to bilete i meldinga som utløyser svaret. Bileta blir tilpassa før tolking.</p>}
      {!canUseVision && <Status>Bilettolking er ikkje tilgjengeleg enno. Vanlege tekstsvar kan framleis brukast.</Status>}
      <label className="sp-circle-agent-enabled"><input type="checkbox" checked={imagesEnabled}
        disabled={saving || (!canUseImages && !imagesEnabled)} onChange={event => {
          setImagesEnabled(event.target.checked); if (!event.target.checked) setImagesOccasional(false);
        }} />Lag bilete av agenten</label>
      {imagesEnabled && <div className="sp-agent-image-settings" role="group" aria-label="Bilete frå agenten">
        <label htmlFor="circle-agent-image-identity">Visuell identitet</label>
        <select id="circle-agent-image-identity" value={imageIdentity} disabled={saving || !canUseImages}
          onChange={event => setImageIdentity(event.target.value)}>
          <option value="">Vel identitet</option>
          {imageIdentity && !imageIdentities.some(identity => identity.id === imageIdentity)
            && <option value={imageIdentity}>Tidlegare identitet (ikkje tilgjengeleg)</option>}
          {imageIdentities.map(identity => <option key={identity.id} value={identity.id}>{identity.label}</option>)}
        </select>
        <p>Agenten kan svare med eit generert bilete når nokon ber om det. Maksimalt to bilete per døgn.</p>
        <label className="sp-circle-agent-enabled"><input type="checkbox" checked={imagesOccasional}
          disabled={saving || !canUseImages} onChange={event => setImagesOccasional(event.target.checked)} />Del eit bilete av og til i samtalen</label>
        <p>Dette er valfritt og skjer høgst éin gong per døgn, berre som del av ein aktiv samtale.</p>
        {!canUseImages && <Status>Biletgenerering er ikkje tilgjengeleg no. Du kan slå henne av her.</Status>}
      </div>}
      <label className="sp-circle-agent-enabled"><input type="checkbox" checked={weatherEnabled}
        disabled={saving} onChange={event => setWeatherEnabled(event.target.checked)} />Vêrdata</label>
      {weatherEnabled && <>
        <p>Hent vêrdata for ein fast stad. Koordinatane blir lagra i oppsettet; vi brukar ikkje GPS-posisjonen din.</p>
        <TextField label="Stad" value={location} maxLength={80} disabled={saving} onChange={event => setLocation(event.target.value)} />
        <TextField label="Breiddegrad" type="number" step="any" min={-90} max={90} value={latitude} disabled={saving} onChange={event => setLatitude(event.target.value)} />
        <TextField label="Lengdegrad" type="number" step="any" min={-180} max={180} value={longitude} disabled={saving} onChange={event => setLongitude(event.target.value)} />
        {!weatherValid && <Status tone="error">Skriv ein stad med 1–80 teikn, breiddegrad frå −90 til 90 og lengdegrad frå −180 til 180.</Status>}
        {!weatherAvailable && <Status tone="error">Vêrtenesta er ikkje klar. Du kan lagre oppsettet, men ikkje aktivere vêragenten enno.</Status>}
      </>}
      <label className="sp-circle-agent-enabled"><input type="checkbox" checked={seaEnabled}
        disabled={saving || (!seaCatalogue.length && !seaEnabled)} onChange={event => setSeaEnabled(event.target.checked)} />Sjøtemperatur</label>
      {seaEnabled && <div role="group" aria-label="Sjøtemperaturstader">
        <p>Vel opptil åtte stader agenten kan slå opp, og ein standardstad for spørsmål utan stadnamn.</p>
        {seaCatalogue.map(location => <label key={location.id} className="sp-circle-agent-enabled">
          <input type="checkbox" checked={seaLocations.includes(location.id)}
            disabled={saving || (!seaLocations.includes(location.id) && seaLocations.length >= 8)} onChange={event => {
              const next = event.target.checked ? [...seaLocations, location.id] : seaLocations.filter(id => id !== location.id);
              setSeaLocations(next); if (!next.includes(seaDefault)) setSeaDefault(next[0] ?? "");
            }} />{location.name}
        </label>)}
        {seaLocations.filter(id => !seaCatalogue.some(location => location.id === id)).map(id => <label key={id} className="sp-circle-agent-enabled">
          <input type="checkbox" checked disabled={saving} onChange={() => {
            const next = seaLocations.filter(location => location !== id);
            setSeaLocations(next); if (!next.includes(seaDefault)) setSeaDefault(next[0] ?? "");
          }} />Tidlegare stad (ikkje tilgjengeleg)
        </label>)}
        <label htmlFor="circle-agent-sea-default">Standardstad</label>
        <select id="circle-agent-sea-default" value={seaDefault} disabled={saving || !seaLocations.length}
          onChange={event => setSeaDefault(event.target.value)}>
          {!seaLocations.length && <option value="">Vel ein stad</option>}
          {seaLocations.map(id => <option key={id} value={id}>{seaCatalogue.find(location => location.id === id)?.name ?? "Tidlegare stad (ikkje tilgjengeleg)"}</option>)}
        </select>
        {!seaValid && <Status tone="error">Vel ein til åtte tilgjengelege stader og ein standardstad frå utvalet.</Status>}
        {!seaAvailable && <Status tone="error">Sjøtemperaturtenesta er ikkje klar. Du kan lagre oppsettet, men ikkje aktivere agenten enno.</Status>}
      </div>}
      <label className="sp-circle-agent-enabled"><input type="checkbox" checked={ferryEnabled}
        disabled={saving} onChange={event => setFerryEnabled(event.target.checked)} />Fergeruter for Paros</label>
      {ferryEnabled && <>
        <p>Agenten kan slå opp dagens planlagde anløp i Parikia. Rutetider stadfestar ikkje kva ferge som faktisk har kome inn.</p>
        {!ferryAvailable && <Status tone="error">Fergeoppslaget er ikkje klart. Du kan lagre oppsettet, men ikkje aktivere agenten enno.</Status>}
      </>}
      <label className="sp-circle-agent-enabled"><input type="checkbox" checked={memoryEnabled}
        disabled={saving} onChange={event => setMemoryEnabled(event.target.checked)} />Tillat minne for agenten</label>
      <p>Kvar brukar må også tillate eige minne under Mitt minne i kretsmenyen.</p>
      {error && <Status tone="error">{error}</Status>}
      <div className="sp-row"><Button type="button" onClick={() => { setFormOpen(false); setError(""); }}>Avbryt</Button>
        <Button type="submit" busy={saving} disabled={!name.trim() || !lines(triggers).length || !lines(phrases).length || !weatherValid || !seaValid || !imagesValid || (enabled && !workerAvailable)}>Lagre agent</Button></div>
    </form>}
    </>}
    {!formOpen && error && <><Status tone="error">{error}</Status><Button disabled={saving} onClick={() => setReload(value => value + 1)}>Hent agentane på nytt</Button></>}
  </Dialog>;
}
