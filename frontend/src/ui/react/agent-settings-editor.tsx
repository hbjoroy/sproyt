import { Button, Status, TextField } from "@sproyt/ui/react";
import { useState, useSyncExternalStore, type ReactNode } from "react";
import type { CircleChatAgent, CircleChatAgentApi } from "../../chat-agents";
import { validAgentSeaTemperature, validAgentWeather } from "../../chat-agents";
import { AgentSettingsDrafts, agentPartFields, agentPartInput, agentValues, type AgentDraftValues, type AgentEditorDraft, type AgentEditorPart } from "./agent-settings-drafts";

export type AgentCatalogue = Awaited<ReturnType<CircleChatAgentApi["list"]>>;
export const agentPartTitles: Record<AgentEditorPart, string> = {
  behaviour: "Oppførsel", memory: "Minne", vision: "Bilettolking", images: "Biletskaping",
  weather: "Vêr", sea: "Sjøtemperatur", ferry: "Ferger"
};
const lines = (text: string) => text.split(/\r?\n/u).map(value => value.trim()).filter(Boolean);
const equal = (left: unknown, right: unknown) => JSON.stringify(left) === JSON.stringify(right);
const fieldLabels: Record<keyof AgentDraftValues, string> = {
  name: "Namn", triggers: "Triggerfrasar", phrases: "Svarføringar", memoryEnabled: "Minne", visionEnabled: "Bilettolking",
  imagesEnabled: "Biletskaping", imageIdentity: "Visuell identitet", imagesOccasional: "Bilete av og til",
  weatherEnabled: "Vêrdata", location: "Stad", latitude: "Breiddegrad", longitude: "Lengdegrad",
  seaEnabled: "Sjøtemperatur", seaLocations: "Stader", seaDefault: "Standardstad", ferryEnabled: "Fergeruter"
};

export function AgentSettingsEditor({ api, circleId, agentId, agent, part, catalogue, drafts, admitted, onSaved, onBack, onReload }: {
  api: CircleChatAgentApi; circleId: string; agentId: string; agent: CircleChatAgent | null; part: AgentEditorPart;
  catalogue: AgentCatalogue; drafts: AgentSettingsDrafts; admitted(): boolean;
  onSaved(agent: CircleChatAgent): void; onBack(): void; onReload(): Promise<void>;
}) {
  useSyncExternalStore(drafts.subscribe, drafts.getSnapshot);
  const [reading, setReading] = useState(false);
  let draft: AgentEditorDraft;
  try { draft = drafts.start(circleId, agentId, agent); }
  catch (cause) { return <Status tone="error">{cause instanceof Error ? cause.message : "Kunne ikkje opne utkastet."}</Status>; }
  const v = draft.values, baseline = draft.baseline, baselineValues = agentValues(baseline);
  const pending = draft.pending || !!draft.handoff?.pending;
  const conflict = draft.conflicts.some(field => agentPartFields[part].includes(field));
  const changed = !baseline || agentPartFields[part].some(field => !equal(v[field], baselineValues[field]));
  const visionAvailable = baseline?.visionAvailable ?? catalogue.visionAvailable;
  const imagesAvailable = baseline?.imageGenerationAvailable ?? catalogue.imageGenerationAvailable;
  const previousImages = baseline?.imageGeneration;
  let valid = true;
  if (part === "behaviour") valid = !!v.name.trim() && !!lines(v.triggers).length && !!lines(v.phrases).length;
  if (part === "weather" && v.weatherEnabled) valid = !!v.latitude.trim() && !!v.longitude.trim()
    && validAgentWeather({ location: v.location.trim(), latitude: Number(v.latitude), longitude: Number(v.longitude) });
  if (part === "sea" && v.seaEnabled) valid = validAgentSeaTemperature({ defaultLocation: v.seaDefault, locations: v.seaLocations })
    && (equal(v.seaLocations, baselineValues.seaLocations) || v.seaLocations.every(id => catalogue.seaTemperatureLocations.some(location => location.id === id)));
  if (part === "images" && v.imagesEnabled) valid = catalogue.imageIdentities.some(identity => identity.id === v.imageIdentity)
    && (imagesAvailable || (v.imageIdentity === previousImages?.identityId && v.imagesEnabled === previousImages?.enabled && v.imagesOccasional === previousImages?.occasional));
  const update = (values: Partial<AgentDraftValues>) => { if (admitted()) drafts.update(circleId, agentId, values); };
  const display = (field: keyof AgentDraftValues, value: AgentDraftValues[keyof AgentDraftValues]): string => {
    if (typeof value === "boolean") return value ? "På" : "Av";
    if (Array.isArray(value)) return value.map(id => catalogue.seaTemperatureLocations.find(location => location.id === id)?.name ?? "Tidlegare stad").join(", ") || "Ingen";
    if (field === "seaDefault") return catalogue.seaTemperatureLocations.find(location => location.id === value)?.name ?? "Tidlegare stad";
    if (field === "imageIdentity") return catalogue.imageIdentities.find(identity => identity.id === value)?.label ?? (value ? "Tidlegare identitet" : "Ingen");
    return value || "Tomt";
  };
  const checkbox = (label: string, checked: boolean, change: (value: boolean) => void, disabled = false) =>
    <label className="sp-circle-agent-enabled"><input type="checkbox" checked={checked} disabled={disabled}
      onChange={event => change(event.target.checked)} />{label}</label>;
  const unavailable = (available: boolean, text: string) => !available && <p className="sp-agent-status-explanation">{text}</p>;
  const save = async () => {
    if (!admitted() || pending || reading || draft.needsRead || conflict || !valid || !changed) return;
    const input = agentPartInput(draft, part);
    drafts.pending(circleId, agentId);
    try {
      const saved = baseline ? await api.update(circleId, agentId, input) : await api.create(circleId, input);
      if (!admitted()) return;
      if (saved.circleId !== circleId || (baseline && saved.agentId !== agentId)) throw new Error("Tenaren stadfesta ikkje den venta agenten. Hent oppsettet på nytt.");
      if (baseline ? !drafts.confirmed(circleId, saved, part, draft) : !drafts.created(circleId, saved, draft)) return;
      onSaved(saved);
    } catch (cause) {
      if (admitted()) drafts.failed(circleId, agentId, cause instanceof Error ? cause.message : "Lagringa kunne ikkje stadfestast.", draft);
    }
  };
  let fields: ReactNode;
  switch (part) {
    case "behaviour": fields = <>
      {!baseline && <p>Agenten blir oppretta avslått. Etterpå vel du ferdigheiter, kanaltilgang og aktivering.</p>}
      <TextField label="Namn" value={v.name} maxLength={80} onChange={event => update({ name: event.target.value })} />
      <label htmlFor="agent-behaviour-triggers">Triggerord eller -frasar, eitt per linje</label>
      <textarea id="agent-behaviour-triggers" rows={3} value={v.triggers} onChange={event => update({ triggers: event.target.value })} />
      <label htmlFor="agent-behaviour-phrases">Stikkord og svarføringar, eitt per linje</label>
      <textarea id="agent-behaviour-phrases" rows={4} value={v.phrases} onChange={event => update({ phrases: event.target.value })} />
      <p>Føringane styrer innhald og tone. Dei blir ikkje brukte som eit ferdig svar. Omtale med @namn fungerer i tillegg til triggerfrasane.</p>
    </>; break;
    case "memory": fields = <>
      {checkbox("Tillat minne for agenten", v.memoryEnabled, memoryEnabled => update({ memoryEnabled }))}
      <p>Kvar brukar må òg tillate sitt eige minne. Kretsoppsettet gir ingen innsyn i andre sine personlege minne.</p>
    </>; break;
    case "vision": fields = <>
      {checkbox("Tolk bilete i meldingar", v.visionEnabled, visionEnabled => update({ visionEnabled }), !visionAvailable && !v.visionEnabled)}
      <p>Agenten kan tolke opptil to bilete i meldinga som utløyser svaret. Bileta blir tilpassa før tolking.</p>
      {unavailable(visionAvailable, "Bilettolking er ikkje tilgjengeleg i dette oppsettet. Det seier ikkje at vanlege tekstsvar er utilgjengelege.")}
    </>; break;
    case "images": fields = <>
      {checkbox("Lag bilete av agenten", v.imagesEnabled, imagesEnabled => update({ imagesEnabled, ...(!imagesEnabled ? { imagesOccasional: false } : {}) }), !imagesAvailable && !v.imagesEnabled)}
      {v.imagesEnabled && <>
        <label htmlFor="agent-image-identity">Visuell identitet</label>
        <select id="agent-image-identity" value={v.imageIdentity} disabled={!imagesAvailable} onChange={event => update({ imageIdentity: event.target.value })}>
          <option value="">Vel identitet</option>
          {v.imageIdentity && !catalogue.imageIdentities.some(identity => identity.id === v.imageIdentity) && <option value={v.imageIdentity}>Tidlegare identitet (ikkje tilgjengeleg)</option>}
          {catalogue.imageIdentities.map(identity => <option key={identity.id} value={identity.id}>{identity.label}</option>)}
        </select>
        {checkbox("Del eit bilete av og til i samtalen", v.imagesOccasional, imagesOccasional => update({ imagesOccasional }), !imagesAvailable)}
        <p>Dette er valfritt og skjer høgst éin gong per døgn som del av ein aktiv samtale. Bilete på førespurnad er avgrensa til to per døgn.</p>
      </>}
      {unavailable(imagesAvailable, "Biletskaping er ikkje tilgjengeleg no. Eit eksisterande val blir bevart, og du kan slå det av.")}
    </>; break;
    case "weather": fields = <>
      {checkbox("Vêrdata", v.weatherEnabled, weatherEnabled => update({ weatherEnabled }))}
      {v.weatherEnabled && <>
        <p>Vel ein fast stad. Koordinatane blir lagra i agentoppsettet; vi brukar ikkje GPS-posisjonen din.</p>
        <TextField label="Stad" value={v.location} maxLength={80} onChange={event => update({ location: event.target.value })} />
        <TextField label="Breiddegrad" type="number" step="any" min={-90} max={90} value={v.latitude} onChange={event => update({ latitude: event.target.value })} />
        <TextField label="Lengdegrad" type="number" step="any" min={-180} max={180} value={v.longitude} onChange={event => update({ longitude: event.target.value })} />
        {!valid && <Status tone="error">Skriv ein stad, breiddegrad frå −90 til 90 og lengdegrad frå −180 til 180.</Status>}
      </>}
      {unavailable(catalogue.weatherAvailable, "Vêrtenesta er ikkje konfigurert. Du kan ta vare på oppsettet her; aktivering blir kontrollert separat.")}
    </>; break;
    case "sea": fields = <>
      {checkbox("Sjøtemperatur", v.seaEnabled, seaEnabled => update({ seaEnabled }), !catalogue.seaTemperatureLocations.length && !v.seaEnabled)}
      {v.seaEnabled && <>
        <p>Vel opptil åtte stader og ein standardstad for spørsmål utan stadnamn.</p>
        {catalogue.seaTemperatureLocations.map(location => <div key={location.id}>{checkbox(location.name, v.seaLocations.includes(location.id), checked => {
          const seaLocations = checked ? [...v.seaLocations, location.id] : v.seaLocations.filter(id => id !== location.id);
          update({ seaLocations, seaDefault: seaLocations.includes(v.seaDefault) ? v.seaDefault : seaLocations[0] ?? "" });
        }, !v.seaLocations.includes(location.id) && v.seaLocations.length >= 8)}</div>)}
        {v.seaLocations.filter(id => !catalogue.seaTemperatureLocations.some(location => location.id === id)).map(id =>
          <label className="sp-circle-agent-enabled" key={id}><input type="checkbox" checked onChange={() => {
            const seaLocations = v.seaLocations.filter(location => location !== id);
            update({ seaLocations, seaDefault: seaLocations.includes(v.seaDefault) ? v.seaDefault : seaLocations[0] ?? "" });
          }} />Tidlegare stad (ikkje tilgjengeleg)</label>)}
        <label htmlFor="agent-sea-default">Standardstad</label><select id="agent-sea-default" value={v.seaDefault} disabled={!v.seaLocations.length} onChange={event => update({ seaDefault: event.target.value })}>
          {!v.seaLocations.length && <option value="">Vel ein stad</option>}
          {v.seaLocations.map(id => <option value={id} key={id}>{catalogue.seaTemperatureLocations.find(location => location.id === id)?.name ?? "Tidlegare stad (ikkje tilgjengeleg)"}</option>)}
        </select>
        {!valid && <Status tone="error">Vel ein til åtte stader og ein standardstad frå utvalet.</Status>}
      </>}
      {unavailable(catalogue.seaTemperatureAvailable, "Sjøtemperatur er ikkje konfigurert i dette miljøet.")}
    </>; break;
    case "ferry": fields = <>
      {checkbox("Fergeruter for Paros", v.ferryEnabled, ferryEnabled => update({ ferryEnabled }))}
      <p>Oppslag for planlagde anløp i Parikia. Rutetider er ikkje sanntidsposisjonar og stadfestar ikkje at ei ferge har kome inn.</p>
      {unavailable(catalogue.ferryAvailable, "Fergeoppslaget er ikkje konfigurert i dette miljøet.")}
    </>; break;
  }
  return <form className="sp-circle-agent-form" onSubmit={event => { event.preventDefault(); void save(); }}>
    <h3>{baseline ? agentPartTitles[part] : "Ny agent"}</h3>
    <fieldset disabled={pending || reading} className="sp-agent-editor-fields">{fields}</fieldset>
    {draft.error && <Status tone="error">{draft.error}</Status>}
    {draft.notice && <Status>{draft.notice}</Status>}
    {draft.needsRead && <>
      <p>Lagringa er ikkje stadfesta. Utkastet er bevart. {baseline ? "Hent lagra oppsett før du prøver igjen." : "Opprettinga kan ha gått gjennom. Kontroller agentlista før du opprettar noko meir."}</p>
      <Button disabled={pending || reading} busy={reading} onClick={async () => {
        setReading(true);
        try { await onReload(); }
        catch (cause) { if (admitted()) drafts.failed(circleId, agentId, cause instanceof Error ? cause.message : "Kunne ikkje lese oppsettet."); }
        finally { setReading(false); }
      }}>Hent lagra oppsett</Button>
    </>}
    {conflict && <div role="group" aria-label="Endra av andre">
      <Status>Dette oppsettet er òg endra av andre. Utkastet ditt er bevart.</Status>
      <details><summary>Samanlikn lagra oppsett og utkast</summary><dl className="sp-agent-editor-comparison">
        {agentPartFields[part].map(field => <div key={field}><dt>{fieldLabels[field]}</dt><dd><span>Lagra: {display(field, baselineValues[field])}</span><span>Ditt utkast: {display(field, v[field])}</span></dd></div>)}
      </dl></details>
      <Button onClick={() => drafts.acknowledge(circleId, agentId, part, false)}>Behald utkastet mitt</Button>
      <Button onClick={() => drafts.acknowledge(circleId, agentId, part, true)}>Bruk lagra oppsett</Button>
    </div>}
    <div className="sp-agent-editor-footer">
      <Button variant="quiet" onClick={onBack}>Tilbake</Button>
      <Button variant="quiet" disabled={pending} onClick={() => { if (!admitted()) return; if (baseline) drafts.acknowledge(circleId, agentId, part, true); else { drafts.remove(circleId, agentId); onBack(); } }}>Forkast endringar</Button>
      <Button type="submit" busy={pending} disabled={pending || reading || draft.needsRead || conflict || !valid || !changed}>{baseline ? "Lagre endringar" : "Opprett agent"}</Button>
    </div>
  </form>;
}
