import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useState } from "react";
import type { MediaObject } from "../../types";

/** URLs are constructed from validated server IDs, never from user-authored URLs. */
function mediaUrl(id: string, preview = false) {
  const participant = new URL(window.location.href).searchParams.get("participant");
  return `/api/v1/media/${id}${preview ? "/preview" : ""}${participant ? `?participant=${encodeURIComponent(participant)}` : ""}`;
}

function MediaFigure({ id, contentType, name, onOpen }: { id: string; contentType: string; name: string; onOpen?: () => void }) {
  const size = { display: "block", maxWidth: "100%", maxHeight: "min(45vh, 360px)", objectFit: "contain" as const };
  return <figure style={{ margin: "8px 0", minWidth: 0 }}>
    {contentType.startsWith("video/")
      ? <video src={mediaUrl(id)} controls preload="metadata" style={size} aria-label={name} />
      : onOpen
        ? <button type="button" aria-label={`Vis ${name} i full storleik`} onClick={onOpen}
            style={{ display: "block", padding: 0, border: 0, background: "transparent", maxWidth: "100%" }}>
            <img src={mediaUrl(id, true)} alt={name} loading="lazy" style={size} />
          </button>
        : <img src={mediaUrl(id, true)} alt={name} loading="lazy" style={size} />}
    <figcaption>{name} · <a href={mediaUrl(id)} target="_blank" rel="noopener noreferrer">Vis i full storleik</a></figcaption>
  </figure>;
}

export function PreviewAttachments({ media, status, busy, onRemove }: {
  media: readonly MediaObject[]; status?: string; busy: boolean; onRemove: (id: string) => void;
}) {
  return <section aria-label="Valde vedlegg" style={{ maxHeight: "min(38vh, 360px)", overflowY: "auto" }}>
    {media.map(item => <div key={item.id}>
      <MediaFigure id={item.id} contentType={item.content_type} name={item.original_filename} />
      <Button disabled={busy} onClick={() => onRemove(item.id)}>Fjern {item.original_filename}</Button>
    </div>)}
    {status && <Status>{status}</Status>}
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
