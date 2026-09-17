import { Button, Status } from "@sproyt/ui/react";
import { useSyncExternalStore } from "react";
import type { ImageGenerationOwner } from "../../imagegen";

const labels: Record<string, string> = {
  queued: "Biletet er i kø.", submitting: "Sender til Santorini …", running: "Lagar biletet …",
  ready: "Biletet er klart. Vil du bruke det?", accepting: "Gjer klart vedlegget …",
  accepted: "Godteke. Legg det i utkastet når du er klar."
};

/** A view of the existing private inbox owner. Subscribing never starts polling. */
export function PreviewImageGeneration({ owner, channelId }: { owner: ImageGenerationOwner; channelId: string | null }) {
  const state = useSyncExternalStore(owner.subscribe, owner.getSnapshot);
  if (!state.visible) return null;
  return <section aria-label="Private biletmeldingar"
    onPointerDown={event => {
      // Blurring an empty composer collapses its tools and moves this panel.
      // Keep a mouse action under the pointer until its click is delivered.
      if (event.pointerType === "mouse" && event.button === 0 && event.target instanceof Element && event.target.closest("button, summary")) event.preventDefault();
    }}
    style={{ padding: "var(--sp-space-3, 12px)", borderBlockEnd: "1px solid var(--sp-line)", maxHeight: "45dvh", overflowY: "auto" }}>
    <strong>Biletverkstad · berre synleg for deg</strong>
    <Button variant="quiet" onClick={owner.hide}>Skjul</Button>
    {state.status && <Status>{state.status}</Status>}
    {state.jobs.map(job => <article key={job.id} style={{ paddingBlock: 12, borderBlockEnd: "1px solid var(--sp-line)" }}>
      <p>{owner.channelName(job.channel_id)}{owner.threadRoot(job.id) ? " · Trådutkast" : ""} · {job.prompt}</p>
      {job.expansion && <details>
        <summary>Sjå utvida biletprompt</summary>
        <p>{job.expansion.prompt}</p>
        <p>{[job.expansion.model, job.expansion.style, job.expansion.warning].filter(Boolean).join(" · ")}</p>
        {job.expansion.sources.map(source => <p key={source}><a href={source} target="_blank" rel="noopener noreferrer">Kjelde: {new URL(source).pathname.slice(6).replaceAll("_", " ")}</a></p>)}
      </details>}
      {job.visualReferences.length > 0 && <details>
        <summary>Sjå referansefoto</summary>
        {job.visualReferences.map(reference => <p key={reference.url}><a href={reference.url} target="_blank" rel="noopener noreferrer">{reference.title}</a> · {reference.credit}</p>)}
      </details>}
      {["ready", "accepted"].includes(job.state) && <img src={owner.previewUrl(job.id)}
        alt="Privat førehandsvising av det genererte biletet" loading="lazy"
        style={{ display: "block", maxWidth: "100%", maxHeight: "30dvh", objectFit: "contain" }} />}
      <p>{job.error || labels[job.state] || job.state}</p>
      {["ready", "accepted"].includes(job.state) && <>
        {channelId !== job.channel_id && <p>Opne den opphavlege kanalen for å leggje biletet i utkastet.</p>}
        <Button disabled={state.busy || channelId !== job.channel_id} onClick={() => { void owner.review(job.id, "accept"); }}>
          {job.state === "ready" ? "Godta" : "Legg i utkast"}
        </Button>
      </>}
      {job.state === "ready" && <Button variant="danger" disabled={state.busy} onClick={() => { void owner.review(job.id, "decline"); }}>Avslå</Button>}
      {["accepted", "failed"].includes(job.state) && <Button disabled={state.busy} onClick={() => { void owner.review(job.id, "dismiss"); }}>Lukk</Button>}
    </article>)}
  </section>;
}
