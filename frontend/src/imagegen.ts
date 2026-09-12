import { HttpClient } from "./api";
import { isRecord, mediaFromUpload, type MediaObject } from "./types";

export function imagePrompt(draft: string): string | null {
  if (!/^\/imagegen(?:\s|$)/i.test(draft)) return null;
  let prompt = draft.replace(/^\/imagegen\s*/i, "").trim();
  if (prompt.startsWith('"') && prompt.endsWith('"') && prompt.length >= 2) prompt = prompt.slice(1, -1).trim();
  if (!prompt || [...prompt].length > 2000) throw new Error('Bruk /imagegen "skildring av biletet" (1–2000 teikn).');
  return prompt;
}

type Job = { id: string; channel_id: string; state: string; prompt: string; error: string | null };
function decodeJobs(value: unknown): Job[] {
  if (!isRecord(value) || !Array.isArray(value.jobs)) throw new Error("Ugyldig biletkø");
  return value.jobs.map((job: unknown) => {
    if (!isRecord(job) || typeof job.id !== "string" || typeof job.channel_id !== "string" || typeof job.state !== "string" || typeof job.prompt !== "string") throw new Error("Ugyldig biletjobb");
    return { id: job.id, channel_id: job.channel_id, state: job.state, prompt: job.prompt, error: typeof job.error === "string" ? job.error : null };
  });
}

export function createImageGeneration(options: {
  http: HttpClient; before: HTMLElement; connected: () => boolean; channel: () => string;
  identity: () => string; channelName: (id: string) => string; attach: (media: MediaObject) => void;
}) {
  const panel = document.createElement("section");
  panel.className = "imagegen-inbox";
  panel.setAttribute("aria-label", "Private biletmeldingar");
  panel.hidden = true;
  const launcher = document.createElement("button"); launcher.type = "button"; launcher.textContent = "Biletverkstad"; launcher.hidden = true;
  launcher.addEventListener("click", () => { panel.hidden = false; void refresh(); });
  options.before.before(launcher, panel);
  const heading = document.createElement("strong");
  heading.textContent = "Biletverkstad · berre synleg for deg";
  const status = document.createElement("p");
  status.setAttribute("role", "status");
  const cards = document.createElement("div");
  const hide = document.createElement("button"); hide.type = "button"; hide.textContent = "Skjul";
  hide.addEventListener("click", () => { panel.hidden = true; });
  panel.append(heading, hide, status, cards);
  let signature = "";
  let polling = false;
  let submitting = false;
  let busy = false;
  let jobs: Job[] = [];
  // Reuse the admission id on a network retry, including after a page reload.
  function requestId(channel: string, prompt: string): string {
    const key = `sproyt-imagegen-admission:${options.identity()}`;
    try {
      const previous: unknown = JSON.parse(sessionStorage.getItem(key) || "null");
      if (isRecord(previous) && previous.channel === channel && previous.prompt === prompt && typeof previous.id === "string") return previous.id;
    } catch { /* Storage is optional; server-side admission still bounds jobs. */ }
    const id = crypto.randomUUID();
    try { sessionStorage.setItem(key, JSON.stringify({ channel, prompt, id })); } catch { /* optional */ }
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
    launcher.hidden = jobs.length === 0 && !status.textContent;
    if (jobs.some(job => job.state === "ready")) status.textContent = "Biletet er klart til privat gjennomgang.";
    panel.hidden = jobs.length === 0 && !status.textContent;
    cards.replaceChildren();
    for (const job of jobs) {
      const card = document.createElement("article");
      card.className = "imagegen-card";
      const caption = document.createElement("p");
      caption.textContent = `${options.channelName(job.channel_id)} · ${job.prompt}`;
      card.append(caption);
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
    async submit(draft: string, channel: string): Promise<boolean> {
      const prompt = imagePrompt(draft);
      if (prompt === null) return false;
      if (submitting) throw new Error("Biletførespurnaden blir allereie send.");
      submitting = true; panel.hidden = false; launcher.hidden = false;
      status.textContent = "Legg biletet i kø …";
      try {
        await jsonPost("/api/v1/imagegen", { channel_id: channel, request_id: requestId(channel, prompt), prompt });
        try { sessionStorage.removeItem(`sproyt-imagegen-admission:${options.identity()}`); } catch { /* optional */ }
        status.textContent = "Biletet er i kø. Førehandsvisinga kjem hit privat; ingenting er posta i kanalen.";
        await refresh();
        return true;
      } finally { submitting = false; }
    },
  };
}
