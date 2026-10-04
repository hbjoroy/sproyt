import { Button } from "@sproyt/ui/react";
import { useEffect, useRef, useState } from "react";

const maxImageBytes = 35 * 1024 * 1024; // Same bound as the media upload endpoint.
const imageTypes = new Set(["image/jpeg", "image/png", "image/gif", "image/webp", "image/heic", "image/avif"]);
function iosStandalone(): boolean {
  if (typeof navigator === "undefined") return false;
  const ios = /iPad|iPhone|iPod/u.test(navigator.userAgent) || (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);
  return ios && ((navigator as Navigator & { standalone?: boolean }).standalone === true || matchMedia("(display-mode: standalone)").matches);
}

async function prepareImage(href: string, name: string, signal: AbortSignal): Promise<File> {
  const url = new URL(href, location.href);
  if (url.origin !== location.origin) throw new Error("Biletet må hentast frå Sprøyt.");
  const response = await fetch(url, { credentials: "same-origin", redirect: "error", signal });
  const type = response.headers.get("content-type")?.split(";", 1)[0]?.trim().toLowerCase() ?? "";
  if (!response.ok || !imageTypes.has(type)) { void response.body?.cancel().catch(() => {}); throw new Error("Kunne ikkje hente biletet. Prøv igjen."); }
  if (Number(response.headers.get("content-length")) > maxImageBytes) { void response.body?.cancel().catch(() => {}); throw new Error("Biletet er for stort til deling."); }
  const reader = response.body?.getReader();
  if (!reader) throw new Error("Kunne ikkje hente biletet. Prøv igjen.");
  const chunks: Uint8Array<ArrayBuffer>[] = [];
  let size = 0;
  try {
    for (;;) {
      const chunk = await reader.read();
      if (chunk.done) break;
      size += chunk.value.byteLength;
      if (size > maxImageBytes) { void reader.cancel().catch(() => {}); throw new Error("Biletet er for stort til deling."); }
      chunks.push(new Uint8Array(chunk.value));
    }
  } finally { reader.releaseLock(); }
  if (!size) throw new Error("Biletet er tomt.");
  return new File(chunks, name, { type });
}

/** Installed iOS apps prepare a file for an explicit share gesture; other browsers retain native downloads. */
export function ImageDownloadLink({ href, name, className }: { href: string; name: string; className: string }) {
  const [attempt, setAttempt] = useState(0);
  const [standalone] = useState(iosStandalone);
  const [prepared, setPrepared] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const controller = useRef<AbortController | null>(null);
  const generation = useRef(0);
  useEffect(() => {
    generation.current++;
    setPrepared(null); setNotice(""); setBusy(false);
    return () => { generation.current++; controller.current?.abort(); };
  }, [href, name]);

  useEffect(() => {
    if (!attempt) return;
    const timer = window.setTimeout(() => setAttempt(0), 4000);
    return () => window.clearTimeout(timer);
  }, [attempt]);

  const prepare = async () => {
    if (busy) return;
    setPrepared(null);
    if (!navigator.share || !navigator.canShare) { setNotice("Denne appen støttar ikkje deling av filer. Opne Sprøyt i Safari for å laste ned biletet."); return; }
    setBusy(true); setNotice("Hentar bilete for deling …");
    controller.current?.abort();
    const request = new AbortController(); controller.current = request;
    const current = generation.current;
    try {
      const file = await prepareImage(href, name, request.signal);
      if (request.signal.aborted || current !== generation.current) return;
      if (!navigator.canShare({ files: [file] })) throw new Error("Denne fila kan ikkje delast her. Opne Sprøyt i Safari for å laste ned biletet.");
      setPrepared(file); setNotice("Biletet er klart. Vel Lagre eller del.");
    } catch (cause) { if (!request.signal.aborted && current === generation.current) setNotice(cause instanceof Error && cause.name !== "TypeError" ? cause.message : "Kunne ikkje hente biletet. Prøv igjen."); }
    finally { request.abort(); if (current === generation.current) setBusy(false); }
  };
  const share = async () => {
    if (!prepared || busy) return;
    const current = generation.current;
    try {
      if (!navigator.canShare?.({ files: [prepared] })) throw new Error("Denne fila kan ikkje delast her.");
      // Invoke synchronously from this second click, before any await consumes activation.
      const sharing = navigator.share({ files: [prepared] });
      setBusy(true); await sharing;
      if (current !== generation.current) return;
      setPrepared(null); setNotice("Biletet er overlevert til deling.");
    } catch (cause) {
      if (current !== generation.current) return;
      setNotice(cause instanceof Error && cause.name === "AbortError" ? "Delinga vart avbroten." : "Kunne ikkje opne deling. Prøv igjen.");
    } finally { if (current === generation.current) setBusy(false); }
  };
  if (standalone) return <>
    <Button variant="quiet" className={className} disabled={busy} onClick={() => void prepare()} aria-label={`Last ned ${name}`} title={`Last ned ${name}`}>↓</Button>
    {notice && <div className="sp-media-download-notice" style={{ pointerEvents: "auto" }}><output role="status" aria-live="polite">{notice}</output>
      {prepared && <Button variant="quiet" disabled={busy} onClick={() => void share()}>Lagre eller del</Button>}</div>}
  </>;
  return <>
    <a className={className} href={href} download={name} target="_blank" rel="noopener noreferrer" onClick={() => setAttempt(value => value + 1)}
      aria-label={`Last ned ${name}`} title={`Last ned ${name}`}>↓</a>
    {attempt > 0 && <output role="status" aria-live="polite" className="sp-media-download-notice">Nedlasting starta</output>}
  </>;
}
