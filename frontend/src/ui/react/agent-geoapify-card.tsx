import { Button } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { AgentGeoapifyRollApi, GeoRollPreview, GeoRollView } from "../../agent-geoapify-roll";

type Screen = "closed" | "loading" | "error" | "open" | "sending";
const progressing = new Set(["geocoding", "pending", "leased", "ready"]);

/** Consent resumes the agent job; the card never chooses or draws a venue itself. */
export function AgentGeoapifyCard({ api, owner, channelId, messageId, admitted }: {
  readonly api: AgentGeoapifyRollApi; readonly owner: string; readonly channelId: string; readonly messageId: string; readonly admitted: () => boolean;
}) {
  const [screen, setScreen] = useState<Screen>("closed");
  const [preview, setPreview] = useState<GeoRollPreview | null>(null);
  const [origin, setOrigin] = useState("");
  const live = useRef(true), generation = useRef(0), request = useRef<AbortController | null>(null);
  const currentAdmission = useRef(admitted); currentAdmission.current = admitted;
  useEffect(() => { live.current = true; return () => { live.current = false; generation.current++; request.current?.abort(); }; }, []);
  const allowed = (epoch: number, controller: AbortController) => live.current && generation.current === epoch && !controller.signal.aborted && currentAdmission.current();
  function close() { generation.current++; request.current?.abort(); request.current = null; setPreview(null); setOrigin(""); setScreen("closed"); }
  function load() {
    if (!live.current || !currentAdmission.current()) return;
    const epoch = ++generation.current; request.current?.abort();
    const controller = new AbortController(); request.current = controller; setScreen("loading");
    void api.preview(owner, channelId, messageId, () => allowed(epoch, controller), controller.signal).then(value => {
      if (!allowed(epoch, controller)) return;
      setPreview(value); setOrigin(value.suggestedOrigin ?? ""); setScreen("open");
    }).catch(() => { if (allowed(epoch, controller)) setScreen("error"); });
  }
  function updateState(job: GeoRollView) { setPreview(previous => previous ? { ...previous, state: job } : previous); setScreen("open"); }
  function start() {
    if (!preview || preview.state || !currentAdmission.current()) return;
    const epoch = ++generation.current; request.current?.abort();
    const controller = new AbortController(); request.current = controller; setScreen("sending");
    void api.start(owner, channelId, messageId, preview, origin, () => allowed(epoch, controller), controller.signal)
      .then(job => { if (allowed(epoch, controller)) updateState(job); })
      .catch(() => { if (allowed(epoch, controller)) setScreen("error"); });
  }
  function choose(index: number) {
    if (!preview?.state || preview.state.status !== "choose_origin" || !currentAdmission.current()) return;
    const epoch = ++generation.current; request.current?.abort();
    const controller = new AbortController(); request.current = controller; setScreen("sending");
    void api.choose(owner, channelId, messageId, preview.state.id, index, () => allowed(epoch, controller), controller.signal)
      .then(job => { if (allowed(epoch, controller)) updateState(job); })
      .catch(() => { if (allowed(epoch, controller)) setScreen("error"); });
  }
  function retryGeocoding() {
    if (!preview?.state?.canRetryGeocoding || !preview.state.originText || preview.originMode === "shared" || !currentAdmission.current()) return;
    const epoch = ++generation.current; request.current?.abort();
    const controller = new AbortController(); request.current = controller; setScreen("sending");
    void api.start(owner, channelId, messageId, preview, preview.state.originText, () => allowed(epoch, controller), controller.signal)
      .then(job => { if (allowed(epoch, controller)) updateState(job); })
      .catch(() => { if (allowed(epoch, controller)) setScreen("error"); });
  }
  useEffect(() => {
    if (screen !== "open" || !preview?.state || !progressing.has(preview.state.status)) return;
    const epoch = generation.current;
    const controller = new AbortController();
    const timer = setTimeout(() => {
      if (!allowed(epoch, controller)) return;
      void api.preview(owner, channelId, messageId, () => allowed(epoch, controller), controller.signal).then(value => {
        if (allowed(epoch, controller)) setPreview(value);
      }).catch(() => { if (allowed(epoch, controller)) setScreen("error"); });
    }, 3000);
    return () => { clearTimeout(timer); controller.abort(); };
  }, [api, owner, channelId, messageId, screen, preview]);
  const job = preview?.state;
  return <div className="sp-agent-geoapify-card">
    {screen === "closed" && <Button variant="quiet" onClick={load}>La agenten trille via Geoapify</Button>}
    {screen === "loading" && <div><p>Hentar førespurnaden …</p><Button variant="quiet" onClick={close}>Avbryt</Button></div>}
    {screen === "error" && <div><p>Kunne ikkje hente eller oppdatere førespurnaden. Berre den som bad om søket kan godkjenne det.</p><Button variant="quiet" onClick={load}>Prøv igjen</Button> <Button variant="quiet" onClick={close}>Lukk</Button></div>}
    {screen === "sending" && <div><p>Sender førespurnaden …</p><Button variant="quiet" onClick={close}>Lukk</Button></div>}
    {screen === "open" && preview && <div>
      {!job && <>
        <p>Agenten kan finne barar og pubar innan {preview.radius} meter og trille eit tilfeldig val blant treffa frå Geoapify og OpenStreetMap.</p>
        {preview.originMode === "shared" ? <p>Den delte posisjonen din blir send til Geoapify for dette søket.</p> :
          <label>Utgangspunkt <input value={origin} maxLength={240} onChange={event => setOrigin(event.currentTarget.value)} placeholder="By, hotell eller annan stad" /></label>}
        <p>Utgangspunktet blir sendt til Geoapify. Trefflista blir gitt til agenten, som legg svaret i kanalen.</p>
        <Button disabled={preview.originMode !== "shared" && (!origin.trim() || /[\u0000-\u001f\u007f]/u.test(origin))} onClick={start}>Tillat oppslag og la agenten trille</Button>
      </>}
      {job && progressing.has(job.status) && <p>Agenten arbeider med baroppslaget …</p>}
      {job?.status === "choose_origin" && <><p>Vel utgangspunkt før agenten held fram:</p>{job.choices.map(choice =>
        <div key={choice.index}><Button variant="quiet" onClick={() => choose(choice.index)}>{choice.label}</Button></div>)}</>}
      {job?.status === "published" && <p>Agenten har lagt svaret i samtalen.</p>}
      {job?.status === "failed" && !job.canRetryGeocoding && <p>Agenten kunne ikkje fullføre oppslaget. Prøv ei ny førespurnad i samtalen.</p>}
      {job?.canRetryGeocoding && job.originText && <div><p>Stadoppslaget for «{job.originText}» kan prøvast på nytt med same førespurnad.</p><Button onClick={retryGeocoding}>Prøv stadoppslaget igjen</Button></div>}
      {job?.status === "cancelled" && <p>Førespurnaden er avslutta.</p>}
      <Button variant="quiet" onClick={close}>Lukk</Button>
    </div>}
  </div>;
}
