import { Button, Dialog, Status, TextField } from "@sproyt/ui/react";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { AdvancedHost } from "../../application/advanced-host";
import type { ConversationSnapshot } from "../../application/conversation-snapshot";
import type { CreatedGrafanaIntegration, ProcessView } from "../../api";

function useOperation() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const pending = useRef(false);
  const mounted = useRef(true);
  useEffect(() => () => { mounted.current = false; }, []);
  const run = async (action: () => Promise<void>, message = "") => {
    if (pending.current) return;
    pending.current = true; setBusy(true); setError(""); setNotice("");
    try { await action(); if (mounted.current) setNotice(message); }
    catch (error) { if (mounted.current) setError(error instanceof Error ? error.message : "Handlinga feila. Prøv igjen."); }
    finally { pending.current = false; if (mounted.current) setBusy(false); }
  };
  return { busy, mounted, run, feedback: <>{error && <Status tone="error">{error}</Status>}{notice && <Status>{notice}</Status>}</> };
}

function Secret({ label, value }: { label: string; value: string }) {
  const input = useRef<HTMLInputElement>(null);
  const op = useOperation();
  return <div>
    <TextField ref={input} label={label} value={value} readOnly autoComplete="off" spellCheck={false} />
    <Button onClick={() => void op.run(async () => {
      try { await navigator.clipboard.writeText(value); }
      catch { input.current?.focus(); input.current?.select(); throw new Error("Merk og kopier verdien manuelt."); }
    }, `${label} er kopiert.`)}>Kopier {label.toLocaleLowerCase()}</Button>{op.feedback}
  </div>;
}

function Agent({ host, snapshot }: { host: AdvancedHost; snapshot: ConversationSnapshot }) {
  useSyncExternalStore(host.subscribe, host.revision, host.revision);
  const [credential, setCredential] = useState("");
  const op = useOperation();
  const access = host.agent();
  const channel = snapshot.activeChannel;
  const canCreate = channel && ["owner", "moderator"].includes(channel.role);
  return <>
    <p>Tilgangen varer i 30 minutt og gir agenten rett til å lese historikk og sende meldingar i {channel?.name ?? "vald samtale"}.</p>
    {!canCreate && <Status>Berre eigarar og moderatorar kan gi agenttilgang til denne samtalen.</Status>}
    <Button busy={op.busy} disabled={!canCreate || Boolean(access) || host.agentBusy()} onClick={() => void op.run(async () => {
      const secret = await host.createAgent(channel!.id);
      if (op.mounted.current) setCredential(secret);
    }, "Tilgangen er klar. Kopier credentialen no; han blir ikkje vist igjen etter lukking.")}>Lag kortliva tilgang</Button>
    {access && <>
      <p>Tilgang til {snapshot.channels.find(item => item.id === access.channelId)?.name ?? access.channelId}, gyldig til {new Date(access.expiresAt).toLocaleString("nn-NO")}.</p>
      {credential && <Secret label="Agentcredential" value={credential} />}
      <Button variant="danger" busy={op.busy} onClick={() => void op.run(async () => { await host.revokeAgent(); setCredential(""); }, "Agenttilgangen er trekt tilbake.")}>Trekk tilbake</Button>
    </>}{op.feedback}
  </>;
}

export function PreviewGrafana({ host, channelId }: { host: AdvancedHost; channelId: string }) {
  const [secret, setSecret] = useState<CreatedGrafanaIntegration | null>(null);
  const op = useOperation();
  return <section aria-label="Grafana-integrasjon">
    <h3>Grafana-varsel</h3>
    <p>Lag ein nøkkel for å sende Grafana-varsel til denne kanalen. Tokenet blir berre vist her fram til du lukkar kanaldetaljane.</p>
    <Button busy={op.busy} disabled={Boolean(secret)} onClick={() => void op.run(async () => {
      const created = await host.createGrafana(channelId);
      if (op.mounted.current) setSecret(created);
    })}>Lag Grafana-nøkkel</Button>
    {secret && <>
      <p>Nøkkelen gjeld til {new Date(secret.credentialExpiresAt).toLocaleString("nn-NO")}.</p>
      <Secret label="Webhook-adresse" value={`${window.location.origin}/api/v1/integrations/grafana/alerts`} />
      <Secret label="Grafana-token" value={secret.credential} />
    </>}{op.feedback}
  </section>;
}

function Heart({ host, snapshot }: { host: AdvancedHost; snapshot: ConversationSnapshot }) {
  const [circleId, setCircleId] = useState(snapshot.activeChannel?.circle_id ?? snapshot.circles[0]?.id ?? "");
  const [title, setTitle] = useState("");
  const [id, setId] = useState(host.processId());
  const [view, setView] = useState<ProcessView | null>(null);
  const op = useOperation();
  const circle = snapshot.circles.find(item => item.id === circleId);
  const channel = snapshot.activeChannel;
  const load = async (value: string) => { const result = await host.getProcess(value); if (op.mounted.current) setView(result); };
  return <div style={{ display: "grid", gap: 12 }}>
    <label htmlFor="heart-circle">Vennekrets</label>
    <select id="heart-circle" value={circleId} onChange={event => setCircleId(event.target.value)}><option value="">Vel vennekrets</option>{snapshot.circles.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select>
    {circle?.role === "owner" && <div>
      <Button disabled={op.busy} onClick={() => void op.run(() => host.setHeart(circleId, true), "Event-planlegging er slått på for kretsen.")}>Slå på event-planlegging</Button>
      <Button disabled={op.busy} onClick={() => void op.run(() => host.setHeart(circleId, false), "Event-planlegging er slått av for kretsen.")}>Slå av event-planlegging</Button>
    </div>}
    <form onSubmit={event => { event.preventDefault(); void op.run(async () => {
      const created = await host.startProcess(channel!.id, title); if (op.mounted.current) setId(created); await load(created);
    }); }}>
      <p>Ny planlegging blir starta i {channel?.name ?? "vald kanal"}.</p>
      <TextField label="Tittel på planlegging" value={title} onChange={event => setTitle(event.target.value)} />
      <Button type="submit" busy={op.busy} disabled={!channel?.circle_id || channel.is_direct}>Start planlegging</Button>
    </form>
    <TextField label="Prosess-ID" value={id} onChange={event => { setId(event.target.value); setView(null); }} />
    <div>
      <Button disabled={!id.trim() || op.busy} onClick={() => void op.run(() => load(id.trim()))}>Oppdater status</Button>
      <Button disabled={!id.trim() || op.busy} onClick={() => void op.run(() => host.inspectProcess(id.trim()), "Heart-status er lagd i den varige køen. Oppdater status om litt.")}>Inspiser Heart</Button>
      <Button disabled={!id.trim() || op.busy} onClick={() => void op.run(() => host.answerProcess(id.trim(), "yes"), "Svaret «ja» er lagd i den varige køen.")}>Svar ja</Button>
      <Button disabled={!id.trim() || op.busy} onClick={() => void op.run(() => host.answerProcess(id.trim(), "no"), "Svaret «nei» er lagd i den varige køen.")}>Svar nei</Button>
    </div>
    {view && <section aria-label="Prosessstatus"><h3>{view.process.definitionName}: {view.process.status}</h3>{view.events.map((event, index) => <article key={index}><p>{event.eventType} · {event.actorId}</p><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{JSON.stringify(event.payload, null, 2)}</pre></article>)}</section>}
    {op.feedback}
  </div>;
}

export function PreviewAdvanced({ kind, host, snapshot, onClose }: { kind: "agent" | "heart"; host: AdvancedHost; snapshot: ConversationSnapshot; onClose(): void }) {
  return <Dialog open title={kind === "agent" ? "Agenttilgang" : "Heart og planlegging"} closeLabel="Lukk" onClose={onClose}>
    {kind === "agent" ? <Agent host={host} snapshot={snapshot} /> : <Heart host={host} snapshot={snapshot} />}
  </Dialog>;
}
