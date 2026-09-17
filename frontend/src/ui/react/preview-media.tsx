import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useState } from "react";
import type { MediaObject } from "../../types";

/** URLs are constructed from validated server IDs, never from user-authored URLs. */
function mediaUrl(id: string, preview = false) {
  const participant = new URL(window.location.href).searchParams.get("participant");
  return `/api/v1/media/${id}${preview ? "/preview" : ""}${participant ? `?participant=${encodeURIComponent(participant)}` : ""}`;
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
            <img src={mediaUrl(id, true)} alt={name} loading="lazy" />
          </button>
        : <img src={mediaUrl(id, !fullResolution)} alt={name} loading="lazy" />}
    <figcaption><span title={name}>{name}</span>{onOpen
      ? <button type="button" className="sp-media-original" onClick={onOpen}
          aria-label="Vis originalbiletet">Original</button>
      : isVideo && <a href={mediaUrl(id)} target="_blank" rel="noopener noreferrer">Original ↗</a>}</figcaption>
  </figure>;
}

export function PreviewAttachments({ media, status, busy, onRemove }: {
  media: readonly MediaObject[]; status?: string; busy: boolean; onRemove: (id: string) => void;
}) {
  const [expanded, setExpanded] = useState<string | null>(null);
  return <section aria-label="Valde vedlegg" className="sp-draft-attachments">
    {media.map(item => <div className="sp-draft-attachment" key={item.id}>
      <button className="sp-draft-thumbnail" type="button" title={`Vis ${item.original_filename}`}
        aria-label={`Vis ${item.original_filename}`} onClick={() => setExpanded(item.id)}>
        {item.content_type.startsWith("image/") ? <img src={mediaUrl(item.id, true)} alt="" /> : <span aria-hidden="true">▶</span>}
      </button>
      <span className="sp-draft-filename" title={item.original_filename}>{item.original_filename}</span>
      <Button variant="quiet" disabled={busy} aria-label={`Fjern ${item.original_filename}`} title={`Fjern ${item.original_filename}`}
        onClick={() => onRemove(item.id)}><span aria-hidden="true">×</span></Button>
    </div>)}
    {status && <Status>{status}</Status>}
    {expanded && media.filter(item => item.id === expanded).map(item => <Dialog key={item.id} open title={item.original_filename}
      closeLabel="Lukk førehandsvisinga" onClose={() => setExpanded(null)}>
      <MediaFigure id={item.id} contentType={item.content_type} name={item.original_filename} fullResolution />
    </Dialog>)}
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
    {lightbox && <Dialog open title={lightbox.name} closeLabel="Lukk bilete" onClose={() => setLightbox(null)}>
      <img src={mediaUrl(lightbox.id)} alt={lightbox.name}
        style={{ display: "block", maxWidth: "100%", maxHeight: "min(82dvh, 900px)", objectFit: "contain" }} />
    </Dialog>}</>;
}
