import { Button, Status } from "@sproyt/ui/react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { ThreadSummary } from "../../types";
import type { ThreadPreviewApi, ThreadRootPreview } from "../../thread-previews";
import { DestinationPage } from "./workspace-page";
import directoryCss from "../thread-directory.css";

/** The server summaries cover the whole channel; previews are fetched in bounded batches. */
export function ThreadDirectory({ owner, channelId, channelName, summaries, status, previewGeneration, api, admitted, onRetrySummaries, onOpen, onClose }: {
  owner: string; channelId: string; channelName: string; summaries: readonly ThreadSummary[];
  status: { ready: boolean; loading: boolean; error: string }; previewGeneration: number; api: ThreadPreviewApi;
  admitted(): boolean; onRetrySummaries(): void; onOpen(rootId: string): void; onClose(): void;
}) {
  const [limit, setLimit] = useState(30);
  const [attempt, setAttempt] = useState(0);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [version, setVersion] = useState(0);
  const cache = useRef({ generation: previewGeneration, rows: new Map<string, ThreadRootPreview | null>() });
  if (cache.current.generation !== previewGeneration) cache.current = { generation: previewGeneration, rows: new Map() };
  const guard = useRef(admitted); guard.current = admitted;
  const ordered = useMemo(() => [...summaries].filter(summary => summary.reply_count > 0)
    .sort((a, b) => Number(b.unread_count > 0) - Number(a.unread_count > 0)
      || b.latest_sequence - a.latest_sequence || a.root_message_id.localeCompare(b.root_message_id)), [summaries]);
  const visible = ordered.slice(0, limit);
  const idKey = visible.map(item => item.root_message_id).join(",");
  useEffect(() => {
    if (!status.ready || !guard.current()) return;
    const ownCache = cache.current;
    const ids = idKey ? idKey.split(",").filter(id => !ownCache.rows.has(id)) : [];
    if (!ids.length) { setLoading(false); return; }
    const controller = new AbortController();
    let live = true;
    const allowed = () => live && !controller.signal.aborted && cache.current === ownCache && guard.current();
    setLoading(true); setError("");
    void (async () => {
      for (let offset = 0; offset < ids.length; offset += 30) {
        const batch = ids.slice(offset, offset + 30);
        const previews = await api.list(owner, channelId, batch, allowed, controller.signal);
        if (!allowed()) return;
        const byId = new Map(previews.map(row => [row.id, row]));
        for (const id of batch) ownCache.rows.set(id, byId.get(id) ?? null);
        setVersion(value => value + 1);
      }
    })().catch(() => { if (allowed()) setError("Kunne ikkje hente trådane no."); })
      .finally(() => { if (allowed()) setLoading(false); });
    return () => { live = false; controller.abort(); };
  }, [owner, channelId, api, idKey, attempt, status.ready, previewGeneration]);
  void version;
  return <DestinationPage open title="Trådar i samtalen" closeLabel="Tilbake til samtalen" onClose={onClose}>
    <style>{directoryCss}</style>
    <div className="sp-thread-directory">
      <p className="sp-thread-directory-context">{channelName}</p>
      {!status.ready && (status.error ? <Status tone="error">{status.error} <Button onClick={onRetrySummaries}>Prøv igjen</Button></Status>
        : <Status>Hentar trådar …</Status>)}
      {status.ready && !ordered.length && <p>Ingen trådar med svar i denne samtalen enno.</p>}
      {status.ready && <ol className="sp-thread-directory-list">{visible.flatMap(summary => {
        const root = cache.current.rows.get(summary.root_message_id);
        if (!root) return [];
        const answers = summary.reply_count === 1 ? "1 svar" : `${summary.reply_count} svar`;
        const unread = summary.unread_count > 0 ? ` · ${summary.unread_count} ulest${summary.unread_count === 1 ? "" : "e"}` : "";
        return <li key={root.id}><button type="button" className="sp-thread-directory-item" onClick={() => { if (guard.current()) onOpen(root.id); }}>
          <span className="sp-thread-directory-author">{root.author || (root.deleted ? "Sletta melding" : "Ukjend")}</span>
          <span className="sp-thread-directory-preview">{root.preview || "Melding"}</span>
          <span className="sp-thread-directory-meta">{answers}{unread}</span>
        </button></li>;
      })}</ol>}
      {loading && <p role="status">Hentar meldingar …</p>}
      {error && <Status tone="error">{error} <Button onClick={() => setAttempt(value => value + 1)}>Prøv igjen</Button></Status>}
      {status.ready && limit < ordered.length && !loading && !error && <Button variant="quiet" onClick={() => setLimit(value => value + 30)}>Last fleire trådar</Button>}
    </div>
  </DestinationPage>;
}
