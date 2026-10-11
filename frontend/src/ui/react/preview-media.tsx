import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";
import type { MediaObject } from "../../types";
import { ImageDownloadLink } from "./image-download-link";
import { ImageViewer } from "./image-viewer";

/** URLs are constructed from validated server IDs, never from user-authored URLs. */
function mediaUrl(id: string, suffix: "" | "/preview" | "/download" = "") {
  const participant = new URL(window.location.href).searchParams.get("participant");
  return `/api/v1/media/${id}${suffix}${participant ? `?participant=${encodeURIComponent(participant)}` : ""}`;
}

function PreviewImage({ id, name }: { id: string; name: string }) {
  const image = useRef<HTMLImageElement>(null);
  const [state, setState] = useState("pending");
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let active = true;
    let started = false;
    const controller = new AbortController();
    async function poll() {
      try {
        const response = await fetch(mediaUrl(id, "/preview").replace("/preview", "/status"), { credentials: "same-origin", cache: "no-store", signal: AbortSignal.any([controller.signal,AbortSignal.timeout(10000)]) });
        if ([401,403,404].includes(response.status)) return;
        if (response.ok) {
          const status: { state: string } = await response.json();
          if (!active) return;
          setState(status.state);
          if (["ready", "legacy", "failed"].includes(status.state)) return;
        }
      } catch { /* Keep the bounded placeholder and retry; never use the original. */ }
      if (active) timer = setTimeout(() => void poll(), 5000);
    }
    const observer = new IntersectionObserver(entries => {
      if (!started && entries.some(entry => entry.isIntersecting)) { started = true; void poll(); }
    }, { rootMargin: "100px" });
    if (image.current) observer.observe(image.current);
    return () => { active = false; observer.disconnect(); controller.abort(); if (timer) clearTimeout(timer); };
  }, [id]);
  const url = mediaUrl(id, "/preview");
  return <><img ref={image} src={state === "ready" ? `${url}${url.includes("?") ? "&" : "?"}ready=1` : url} alt={name} loading="lazy" />
    {state === "failed" && <span role="status">Førehandsvisinga kunne ikkje lagast. Originalen er tilgjengeleg.</span>}</>;
}

function MediaFigure({ id, contentType, name, onOpen, fullResolution = false }: {
  id: string; contentType: string; name: string; onOpen?: () => void; fullResolution?: boolean;
}) {
  const isVideo = contentType.startsWith("video/");
  return <figure className="sp-media-figure">
    {isVideo
      ? <video src={mediaUrl(id)} controls preload="metadata" aria-label={name} />
      : onOpen
        ? <button type="button" aria-label={`Vis ${name} i full storleik`} onClick={onOpen}
            className="sp-media-open">
            <PreviewImage id={id} name={name} />
          </button>
        : fullResolution ? <img src={mediaUrl(id)} alt={name} /> : <PreviewImage id={id} name={name} />}
    <figcaption><span title={name}>{name}</span>{onOpen
      ? <><button type="button" className="sp-media-original" onClick={onOpen}
          aria-label="Vis originalbiletet">Original</button>
        <ImageDownloadLink className="sp-media-download" href={mediaUrl(id, "/download")} name={name} /></>
      : isVideo && <a href={mediaUrl(id)} target="_blank" rel="noopener noreferrer">Original ↗</a>}</figcaption>
  </figure>;
}

export function PreviewAttachments({ media, status, busy, onRemove }: {
  media: readonly MediaObject[]; status?: string; busy: boolean; onRemove: (id: string) => void;
}) {
  const [expanded, setExpanded] = useState<string | null>(null);
  const expandedItem = media.find(item => item.id === expanded);
  return <section aria-label="Valde vedlegg" className="sp-draft-attachments">
    {media.map(item => <div className="sp-draft-attachment" key={item.id}>
      <button className="sp-draft-thumbnail" type="button" title={`Vis ${item.original_filename}`}
        aria-label={`Vis ${item.original_filename}`} onClick={() => setExpanded(item.id)}>
        {item.content_type.startsWith("image/") ? <PreviewImage id={item.id} name="" /> : <span aria-hidden="true">▶</span>}
      </button>
      <span className="sp-draft-filename" title={item.original_filename}>{item.original_filename}</span>
      <Button variant="quiet" disabled={busy} aria-label={`Fjern ${item.original_filename}`} title={`Fjern ${item.original_filename}`}
        onClick={() => onRemove(item.id)}><span aria-hidden="true">×</span></Button>
    </div>)}
    {status && <Status>{status}</Status>}
    {expandedItem?.content_type.startsWith("image/") && <ImageViewer src={mediaUrl(expandedItem.id)} downloadSrc={mediaUrl(expandedItem.id, "/download")} name={expandedItem.original_filename} onClose={() => setExpanded(null)} />}
    {expandedItem && !expandedItem.content_type.startsWith("image/") && <Dialog open title={expandedItem.original_filename}
      closeLabel="Lukk førehandsvisinga" onClose={() => setExpanded(null)}>
      <MediaFigure id={expandedItem.id} contentType={expandedItem.content_type} name={expandedItem.original_filename} fullResolution />
    </Dialog>}
  </section>;
}

export function PreviewMediaContent({ body, mediaOnly = false }: { body: string; mediaOnly?: boolean }) {
  const [lightbox, setLightbox] = useState<{ id: string; name: string } | null>(null);
  const attachments: { id: string; contentType: string; name: string }[] = [];
  const text = body.replace(/\[\[media:([0-9a-f-]{36})\|([^|\]]+)\|([^\]]*)\]\]/gi, (_, id: string, contentType: string, encodedName: string) => {
    let name = "media";
    try { name = decodeURIComponent(encodedName || "media"); } catch { /* Keep the safe fallback. */ }
    attachments.push({ id, contentType, name });
    return "";
  }).trim();
  return <>{!mediaOnly && text && <div style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{text}</div>}
    {attachments.map((media, index) => <MediaFigure key={`${media.id}:${index}`} {...media}
      onOpen={media.contentType.startsWith("image/") ? () => setLightbox({ id: media.id, name: media.name }) : undefined} />)}
    {lightbox && <ImageViewer src={mediaUrl(lightbox.id)} downloadSrc={mediaUrl(lightbox.id, "/download")} name={lightbox.name} onClose={() => setLightbox(null)} />}</>;
}
