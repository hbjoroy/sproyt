import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { AgentLocation, AgentLocationApi, LocationAgent } from "../../agent-locations";

type Pending = "" | "locating" | "sharing" | "removing";

function locationErrorMessage(error: unknown, fallback: string): string {
  return error instanceof Error && error.name !== "AbortError" ? error.message : fallback;
}

function geolocationError(error: GeolocationPositionError): string {
  if (error.code === error.PERMISSION_DENIED) {
    return "Posisjonstilgangen vart avvist. Tillat posisjon for Sprøyt i nettlesarinnstillingane, og prøv igjen.";
  }
  if (error.code === error.TIMEOUT) return "Nettlesaren rakk ikkje å finne posisjonen. Prøv igjen når du har betre dekning.";
  return "Nettlesaren fann ikkje posisjonen din. Kontroller posisjonstenestene, og prøv igjen.";
}

function formatTime(value: string): string {
  return new Date(value).toLocaleString(["nn-NO", "nb-NO"], { dateStyle: "medium", timeStyle: "short" });
}

function LocationDetails({ location }: { location: AgentLocation }) {
  return <dl className="sp-agent-location-details">
    <div><dt>Observert</dt><dd><time dateTime={location.observedAt}>{formatTime(location.observedAt)}</time></dd></div>
    <div><dt>Nøyaktigheit</dt><dd>om lag {Math.round(location.accuracyM)} meter</dd></div>
    <div><dt>Gjeld til</dt><dd><time dateTime={location.expiresAt}>{formatTime(location.expiresAt)}</time></dd></div>
  </dl>;
}

export function AgentLocationSharingDialog({ api, channelId, channelName, onClose }: {
  api: AgentLocationApi;
  channelId: string;
  channelName: string;
  onClose: () => void;
}) {
  const [agents, setAgents] = useState<readonly LocationAgent[]>();
  const [selected, setSelected] = useState("");
  const [pending, setPending] = useState<Pending>("");
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");
  const [reload, setReload] = useState(0);
  const generation = useRef(0);
  const mutation = useRef<AbortController | undefined>(undefined);

  useEffect(() => {
    const current = ++generation.current;
    const controller = new AbortController();
    setAgents(undefined); setError(""); setSuccess(""); setPending("");
    void api.list(channelId, controller.signal).then(list => {
      if (current !== generation.current) return;
      setAgents(list);
      setSelected(value => list.some(agent => agent.id === value) ? value : list[0]?.id ?? "");
    }).catch(cause => {
      if (current === generation.current && !(cause instanceof DOMException && cause.name === "AbortError")) {
        setError(locationErrorMessage(cause, "Kunne ikkje hente agentane i kanalen."));
      }
    });
    return () => { generation.current++; controller.abort(); mutation.current?.abort(); };
  }, [api, channelId, reload]);

  const agent = agents?.find(item => item.id === selected);
  const replaceLocation = (agentId: string, location: AgentLocation | null) => {
    setAgents(items => items?.map(item => item.id === agentId ? { ...item, location } : item));
  };
  const share = () => {
    if (!agent || pending) return;
    setError(""); setSuccess("");
    const geolocation = navigator.geolocation;
    if (!geolocation) {
      setError("Denne nettlesaren støttar ikkje posisjonsdeling. Du kan framleis bruke resten av Sprøyt.");
      return;
    }
    const current = ++generation.current;
    setPending("locating");
    geolocation.getCurrentPosition(position => {
      if (current !== generation.current) return;
      const observed = new Date(position.timestamp);
      if (!Number.isFinite(position.coords.latitude) || !Number.isFinite(position.coords.longitude)
        || !Number.isFinite(position.coords.accuracy) || position.coords.accuracy < 0 || position.coords.accuracy > 100_000
        || !Number.isFinite(observed.getTime()) || Date.now() - observed.getTime() > 5 * 60_000
        || observed.getTime() - Date.now() > 60_000) {
        setPending(""); setError("Nettlesaren gav ein ugyldig posisjon. Prøv igjen."); return;
      }
      const controller = new AbortController();
      mutation.current = controller;
      setPending("sharing");
      void api.share(channelId, agent.id, {
        latitude: position.coords.latitude,
        longitude: position.coords.longitude,
        accuracyM: position.coords.accuracy,
        observedAt: observed.toISOString()
      }, controller.signal).then(location => {
        if (current !== generation.current) return;
        replaceLocation(agent.id, location);
        setSuccess(`Posisjonen er delt med ${agent.name} i ${channelName}.`);
      }).catch(cause => {
        if (current === generation.current && !(cause instanceof DOMException && cause.name === "AbortError")) {
          setError(locationErrorMessage(cause, "Kunne ikkje dele posisjonen. Prøv igjen."));
        }
      }).finally(() => { if (current === generation.current) setPending(""); });
    }, cause => {
      if (current !== generation.current) return;
      setPending(""); setError(geolocationError(cause));
    }, { enableHighAccuracy: true, maximumAge: 0, timeout: 15_000 });
  };
  const remove = () => {
    if (!agent || pending) return;
    const current = ++generation.current;
    const controller = new AbortController();
    mutation.current?.abort(); mutation.current = controller;
    setPending("removing"); setError(""); setSuccess("");
    void api.remove(channelId, agent.id, controller.signal).then(() => {
      if (current !== generation.current) return;
      replaceLocation(agent.id, null);
      setSuccess(`Posisjonen er ikkje lenger delt med ${agent.name}.`);
    }).catch(cause => {
      if (current === generation.current && !(cause instanceof DOMException && cause.name === "AbortError")) {
        setError(locationErrorMessage(cause, "Kunne ikkje fjerne posisjonen. Prøv igjen."));
      }
    }).finally(() => { if (current === generation.current) setPending(""); });
  };

  return <Dialog open title="Del posisjon med agent" closeLabel="Lukk posisjonsdeling" onClose={onClose}>
    <div className="sp-agent-location-sharing">
      <p>Agenten kan bruke posisjonen i svar her. Andre i kanalen kan då forstå kvar du er.</p>
      <p className="sp-help">Sprøyt hentar posisjonen éin gong når du trykkjer på knappen. Delinga gjeld berre deg, agenten og denne kanalen, går ut etter 30 minutt og sporar deg ikkje vidare.</p>
      {agents ? agents.length > 0 ? <>
        <label htmlFor="agent-location-agent">Agent</label>
        <select id="agent-location-agent" value={selected} disabled={Boolean(pending)} onChange={event => {
          generation.current++; mutation.current?.abort(); setPending(""); setError(""); setSuccess(""); setSelected(event.currentTarget.value);
        }}>{agents.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select>
        {agent?.location ? <section aria-label={`Delt posisjon for ${agent.name}`}>
          <h3>Posisjonen er delt med {agent.name}</h3>
          <LocationDetails location={agent.location} />
        </section> : <Status>Ingen posisjon er delt med denne agenten.</Status>}
        {pending === "locating" && <Status>Finn posisjonen din …</Status>}
        {pending === "sharing" && <Status>Deler posisjonen …</Status>}
        {pending === "removing" && <Status>Fjernar posisjonen …</Status>}
        {error && <Status tone="error">{error}</Status>}
        {success && <Status>{success}</Status>}
        <div className="sp-row">
          <Button variant="primary" busy={pending === "locating" || pending === "sharing"} disabled={Boolean(pending)} onClick={share}>
            {agent?.location ? "Del ny posisjon" : "Del posisjonen min"}
          </Button>
          {agent?.location && <Button variant="danger" busy={pending === "removing"} disabled={Boolean(pending)} onClick={remove}>Fjern delinga</Button>}
        </div>
      </> : <Status>Ingen tilgjengelege agentar i denne kanalen.</Status> : !error && <Status>Hentar agentar …</Status>}
      {!agents && error && <><Status tone="error">{error}</Status><Button onClick={() => setReload(value => value + 1)}>Prøv igjen</Button></>}
    </div>
  </Dialog>;
}
