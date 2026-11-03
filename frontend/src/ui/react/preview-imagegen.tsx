import { Button, Status } from "@sproyt/ui/react";
import { useLayoutEffect, useRef, useSyncExternalStore } from "react";
import type { ImageGenerationOwner } from "../../imagegen";

const labels: Record<string, string> = {
  queued: "Biletet er i kø.", submitting: "Sender til Santorini …", running: "Lagar biletet …",
  ready: "Biletet er klart. Vil du bruke det?", accepting: "Gjer klart vedlegget …",
  accepted: "Godteke. Legg det i utkastet når du er klar."
};

/** A view of the existing private inbox owner. Subscribing never starts polling. */
export function PreviewImageGeneration({ owner, channelId }: { owner: ImageGenerationOwner; channelId: string | null }) {
  const state = useSyncExternalStore(owner.subscribe, owner.getSnapshot);
  const restoreFocus = useRef<(() => void) | null>(null);
  useLayoutEffect(() => {
    if (!state.visible) {
      restoreFocus.current?.();
      restoreFocus.current = null;
    }
  }, [state.visible]);
  if (!state.visible) return null;
  const returnToComposer = (trigger: HTMLButtonElement) => {
    const field = trigger.closest(".sp-composer-dock")?.querySelector<HTMLTextAreaElement>(".sp-draft-composer textarea");
    return () => {
      if (field?.isConnected && !field.disabled && (document.activeElement === trigger || document.activeElement === document.body)) {
        field.focus({ preventScroll: true });
      }
    };
  };
  return <section className="sp-imagegen" aria-label="Private biletmeldingar"
    onPointerDown={event => {
      // Blurring an empty composer collapses its tools and moves this panel.
      // Keep a mouse action under the pointer until its click is delivered.
      if (event.pointerType === "mouse" && event.button === 0 && event.target instanceof Element && event.target.closest("button, summary")) event.preventDefault();
    }}>
    <header className="sp-imagegen-head">
      <div><h2>Biletverkstad</h2><p>Berre synleg for deg · ikkje sendt</p></div>
      <Button variant="secondary" onClick={event => {
        restoreFocus.current = returnToComposer(event.currentTarget);
        owner.hide();
      }}>Skjul</Button>
    </header>
    <div className="sp-imagegen-body">
      {state.status && <Status>{state.status}</Status>}
      {state.rejectedDraft !== null && <label>Prompt til nytt forsøk
        <textarea className="sp-textarea" value={state.rejectedDraft} onChange={event => owner.editRejectedDraft(event.target.value)} />
      </label>}
      {state.jobs.map(job => <article key={job.id} className="sp-imagegen-job">
        <p>{owner.channelName(job.channel_id)}{owner.threadRoot(job.id) ? " · Trådutkast" : ""} · {job.prompt}</p>
        {job.expansion && <details>
          <summary>Sjå innsend biletprompt ({job.mode})</summary>
          <p>{job.expansion.prompt}</p>
          <p>{[job.expansion.model, job.expansion.style, job.expansion.warning].filter(Boolean).join(" · ")}</p>
          {job.expansion.sources.map(source => <p key={source}><a href={source} target="_blank" rel="noopener noreferrer">Kjelde: {new URL(source).pathname.slice(6).replaceAll("_", " ")}</a></p>)}
        </details>}
        {job.visualReferences.length > 0 && <details>
          <summary>Sjå referansefoto</summary>
          {job.visualReferences.map(reference => <p key={reference.url}><a href={reference.url} target="_blank" rel="noopener noreferrer">{reference.title}</a> · {reference.credit}</p>)}
        </details>}
        {["ready", "accepted"].includes(job.state) && <img src={owner.previewUrl(job.id)}
          alt="Privat førehandsvising av det genererte biletet" loading="lazy" />}
        <p>{job.error || labels[job.state] || job.state}</p>
        <div className="sp-imagegen-actions">
        {["ready", "accepted"].includes(job.state) && <>
          {channelId !== job.channel_id && <p>Opne den opphavlege kanalen for å leggje biletet i utkastet.</p>}
          <Button variant="primary" disabled={state.busy || channelId !== job.channel_id} onClick={event => {
            restoreFocus.current = returnToComposer(event.currentTarget);
            void owner.review(job.id, "accept");
          }}>
            {job.state === "ready" ? "Godta" : "Legg i utkast"}
          </Button>
        </>}
        {job.state === "ready" && <Button variant="danger" disabled={state.busy} onClick={() => { void owner.review(job.id, "decline"); }}>Avslå</Button>}
        {["accepted", "failed"].includes(job.state) && <Button disabled={state.busy} onClick={() => { void owner.review(job.id, "dismiss"); }}>Lukk</Button>}
        </div>
      </article>)}
    </div>
  </section>;
}
