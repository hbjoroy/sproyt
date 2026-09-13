import { HttpClient } from "./api";
import { isRecord, mediaFromUpload, type MediaObject } from "./types";

export function imagePrompt(draft: string): string | null {
  if (!/^\/imagegen(?:\s|$)/i.test(draft)) return null;
  let prompt = draft.replace(/^\/imagegen\s*/i, "").trim();
  if (prompt.startsWith('"') && prompt.endsWith('"') && prompt.length >= 2) prompt = prompt.slice(1, -1).trim();
  if (!prompt || [...prompt].length > 2000) throw new Error('Bruk /imagegen "skildring av biletet" (1–2000 teikn).');
  return prompt;
}

type Expansion = { prompt: string; model: string | null; style: string | null; sources: string[]; warning: string | null };
type VisualReference = { title: string; url: string; credit: string };
type Job = { expansion: Expansion | null; visualReferences: VisualReference[]; id: string; channel_id: string; state: string; prompt: string; error: string | null };
function decodeJobs(value: unknown): Job[] {
  if (!isRecord(value) || !Array.isArray(value.jobs)) throw new Error("Ugyldig biletkø");
  return value.jobs.map((job: unknown) => {
    if (!isRecord(job) || typeof job.id !== "string" || typeof job.channel_id !== "string" || typeof job.state !== "string" || typeof job.prompt !== "string") throw new Error("Ugyldig biletjobb");
    const e = job.expansion;
    const expansion: Expansion | null = isRecord(e) && typeof e.prompt === "string" ? {
      prompt: e.prompt, model: typeof e.model === "string" ? e.model : null,
      style: typeof e.style === "string" ? e.style : null,
      sources: Array.isArray(e.sources) ? e.sources.filter((source): source is string => typeof source === "string" && source.startsWith("https://en.wikipedia.org/wiki/")) : [],
      warning: typeof e.warning === "string" ? e.warning : null
    } : null;
    const visualReferences: VisualReference[] = Array.isArray(job.visual_references) ? job.visual_references.filter((r): r is VisualReference => isRecord(r) && typeof r.title === "string" && typeof r.credit === "string" && typeof r.url === "string" && r.url.startsWith("https://commons.wikimedia.org/wiki/File:")) : [];
    return { expansion, visualReferences, id: job.id, channel_id: job.channel_id, state: job.state, prompt: job.prompt, error: typeof job.error === "string" ? job.error : null };
  });
}

export function createImageGeneration(options: {
  http: HttpClient; before: HTMLElement; toolbar: HTMLElement; connected: () => boolean; channel: () => string;
  identity: () => string; channelName: (id: string) => string; attach: (media: MediaObject) => void;
}) {
  const panel = document.createElement("section");
  panel.className = "imagegen-inbox";
  panel.setAttribute("aria-label", "Private biletmeldingar");
  panel.hidden = true;
  const launcher = document.createElement("button"); launcher.type = "button"; launcher.textContent = "🖼️";
  launcher.className = "composer-icon"; launcher.title = "Biletverkstad"; launcher.setAttribute("aria-label", "Biletverkstad");
  launcher.addEventListener("click", () => {
    dismissed = false;
    if (!jobs.length) status.textContent = 'Skriv /imagegen "skildring av biletet" i meldinga. Opplasta bilete i utkastet blir brukte som referansar (opptil tre).';
    panel.hidden = false;
    void refresh();
  });
  options.toolbar.append(launcher);
  options.before.before(panel);
  const heading = document.createElement("strong");
  heading.textContent = "Biletverkstad · berre synleg for deg";
  const status = document.createElement("p");
  status.setAttribute("role", "status");
  const cards = document.createElement("div");
  const hide = document.createElement("button"); hide.type = "button"; hide.textContent = "Skjul";
  hide.addEventListener("click", () => { dismissed = true; panel.hidden = true; });
  panel.append(heading, hide, status, cards);
  let signature = "";
  let polling = false;
  let submitting = false;
  let busy = false;
  let jobs: Job[] = [];
  let dismissed = false;
  // Reuse the admission id on a network retry, including after a page reload.
  function requestId(channel: string, prompt: string, referenceIds: string[]): string {
    const key = `sproyt-imagegen-admission:${options.identity()}`;
    try {
      const previous: unknown = JSON.parse(sessionStorage.getItem(key) || "null");
      if (isRecord(previous) && previous.channel === channel && previous.prompt === prompt && JSON.stringify(previous.referenceIds || []) === JSON.stringify(referenceIds) && typeof previous.id === "string") return previous.id;
    } catch { /* Storage is optional; server-side admission still bounds jobs. */ }
    const id = crypto.randomUUID();
    try { sessionStorage.setItem(key, JSON.stringify({ channel, prompt, referenceIds, id })); } catch { /* optional */ }
    return id;
  }
  async function jsonPost(path: string, body: unknown): Promise<unknown> {
    const response = await options.http.request(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
    if (!response.ok) throw await HttpClient.error(response);
    return response.json();
  }
  function previewUrl(id: string): string {
    const participant = new URLSearchParams(location.search).get("participant");
    return `/api/v1/imagegen/${encodeURIComponent(id)}/preview${participant ? `?participant=${encodeURIComponent(participant)}` : ""}`;
  }
  function render() {
    const next = JSON.stringify([jobs, options.channel(), options.identity()]);
    if (next === signature || busy) return;
    signature = next;
    if (jobs.some(job => job.state === "ready")) status.textContent = "Biletet er klart til privat gjennomgang.";
    panel.hidden = dismissed || (jobs.length === 0 && !status.textContent);
    cards.replaceChildren();
    for (const job of jobs) {
      const card = document.createElement("article");
      card.className = "imagegen-card";
      const caption = document.createElement("p");
      caption.textContent = `${options.channelName(job.channel_id)} · ${job.prompt}`;
      card.append(caption);
      if (job.expansion) {
        const details = document.createElement("details");
        const summary = document.createElement("summary"); summary.textContent = "Sjå utvida biletprompt";
        const text = document.createElement("p"); text.textContent = job.expansion.prompt;
        const attribution = document.createElement("p"); attribution.textContent = [job.expansion.model, job.expansion.style, job.expansion.warning].filter(Boolean).join(" · ");
        details.append(summary, text, attribution);
        for (const source of job.expansion.sources) {
          const link = document.createElement("a"); link.href = source; link.textContent = "Kjelde: " + decodeURIComponent(new URL(source).pathname.slice(6)).replaceAll("_", " ");
          link.target = "_blank"; link.rel = "noopener noreferrer"; details.append(link, document.createElement("br"));
        }
        card.append(details);
      }
      if (job.visualReferences.length) {
        const details = document.createElement("details");
        const summary = document.createElement("summary"); summary.textContent = "Sjå referansefoto";
        details.append(summary);
        for (const reference of job.visualReferences) {
          const line = document.createElement("p");
          const link = document.createElement("a"); link.href = reference.url; link.textContent = reference.title;
          link.target = "_blank"; link.rel = "noopener noreferrer";
          line.append(link, document.createTextNode(" · " + reference.credit)); details.append(line);
        }
        card.append(details);
      }
      if (["ready", "accepted"].includes(job.state)) {
        const preview = document.createElement("img");
        preview.src = previewUrl(job.id);
        preview.alt = "Privat førehandsvising av det genererte biletet";
        preview.loading = "lazy";
        card.append(preview);
      }
      const label = document.createElement("p");
      label.textContent = job.error || ({ queued: "Biletet er i kø.", submitting: "Sender til Santorini …", running: "Lagar biletet …", ready: "Biletet er klart. Vil du bruke det?", accepting: "Gjer klart vedlegget …", accepted: "Godteke. Legg det i utkastet når du er klar." }[job.state] || job.state);
      card.append(label);
      const action = (text: string, decision: string) => {
        const button = document.createElement("button");
        button.type = "button"; button.textContent = text;
        if (decision === "accept" && options.channel() !== job.channel_id) {
          button.disabled = true; label.textContent += " Opne den opphavlege kanalen for å leggje biletet i utkastet.";
        }
        button.addEventListener("click", async () => {
          if (busy) return;
          const identity = options.identity();
          busy = true; card.querySelectorAll("button").forEach(b => { b.disabled = true; });
          try {
            const result = await jsonPost(`/api/v1/imagegen/${encodeURIComponent(job.id)}/review`, { decision });
            if (identity !== options.identity()) return;
            if (decision === "accept") {
              const media = mediaFromUpload(result);
              if (!media) throw new Error("Ugyldig biletvedlegg");
              // A channel switch while awaiting the response must not attach
              // the picture to whichever channel happens to be open now.
              if (options.channel() === job.channel_id) {
                options.attach(media);
                status.textContent = "Biletet er lagt i utkastet. Skriv ei melding og trykk Send når du vil dele det.";
              } else status.textContent = "Biletet er godteke. Opne den opphavlege kanalen for å leggje det i utkastet.";
            } else status.textContent = decision === "decline" ? "Biletet er avslått og blir ikkje delt." : "Biletmeldinga er lukka.";
          } catch (error) { status.textContent = error instanceof Error ? error.message : "Kunne ikkje oppdatere biletet."; }
          finally { busy = false; signature = ""; await refresh(); }
        });
        card.append(button);
      };
      if (job.state === "ready") { action("Godta", "accept"); action("Avslå", "decline"); }
      if (job.state === "accepted") { action("Legg i utkast", "accept"); action("Lukk", "dismiss"); }
      if (job.state === "failed") action("Lukk", "dismiss");
      cards.append(card);
    }
  }
  async function refresh() {
    if (polling || busy || !options.connected()) return;
    polling = true;
    try {
      const identity = options.identity();
      const response = await options.http.request("/api/v1/imagegen");
      if (identity !== options.identity()) return;
      if (response.ok) { jobs = decodeJobs(await response.json()); render(); }
    } catch { /* Keep the private inbox intact while offline; retry on next tick. */ }
    finally { polling = false; }
  }
  window.setInterval(() => { void refresh(); }, 5_000);
  return {
    async submit(draft: string, channel: string, media: MediaObject[] = []): Promise<boolean> {
      const prompt = imagePrompt(draft);
      if (prompt === null) return false;
      if (submitting) throw new Error("Biletførespurnaden blir allereie send.");
      const referenceIds = media.filter(item => item.channel_id === channel && item.content_type.startsWith("image/")).map(item => item.id);
      if (referenceIds.length > 3) throw new Error("Bruk høgst tre referansebilete i utkastet når du lagar eit bilete.");
      submitting = true; dismissed = false; panel.hidden = false;
      status.textContent = "Legg biletet i kø …";
      try {
        await jsonPost("/api/v1/imagegen", { channel_id: channel, request_id: requestId(channel, prompt, referenceIds), prompt, reference_ids: referenceIds });
        try { sessionStorage.removeItem(`sproyt-imagegen-admission:${options.identity()}`); } catch { /* optional */ }
        status.textContent = "Biletet er i kø. Førehandsvisinga kjem hit privat; ingenting er posta i kanalen.";
        await refresh();
        return true;
      } finally { submitting = false; }
    },
  };
}
