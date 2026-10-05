import { Button, Dialog, Status } from "@sproyt/ui/react";
import { useEffect, useState, useSyncExternalStore } from "react";
import type { ShareReceipt, ShareTarget } from "../../share-target";
import type { ConversationSnapshot } from "../../application/conversation-snapshot";

function ShareDraft({ item, target, snapshot, busy }: { item: ShareReceipt; target: ShareTarget; snapshot: ConversationSnapshot; busy: boolean }) {
  const destinations = snapshot.groups.flatMap(group => group.conversations.filter(entry => entry.channel.role !== "observer").map(entry => ({ id: entry.id, name: `${group.name} · ${entry.name}` })));
  const [text, setText] = useState(item.text);
  const [channel, setChannel] = useState(item.admission?.channelId ?? item.channelId ?? "");
  const [preview, setPreview] = useState("");
  useEffect(() => {
    setPreview("");
    if (!item.file) return;
    const url = URL.createObjectURL(item.file); setPreview(url);
    return () => URL.revokeObjectURL(url);
  }, [item.file]);
  if (!item.owner) return <section><p>Ei deling ventar på innlogging. Ta henne berre i bruk dersom du sjølv delte innhaldet.</p>
    <Button disabled={busy} onClick={() => void target.claim(item.id)}>Bruk delinga med denne kontoen</Button></section>;
  return <section style={{ display: "grid", gap: 12, paddingBlock: 12 }}>
    {preview && <img src={preview} alt={item.file?.name || "Delt bilete"} style={{ maxWidth: "100%", maxHeight: 180, objectFit: "contain" }} />}
    {item.file && <p>{item.file.name} · {(item.file.size / 1024 / 1024).toFixed(1)} MiB</p>}
    <label>Delt tekst eller lenkje<textarea aria-label="Delt tekst eller lenkje" value={item.admission?.draft ?? text} disabled={busy || Boolean(item.admission)} onChange={event => {
      setText(event.currentTarget.value); void target.edit(item.id, channel, event.currentTarget.value);
    }} style={{ minHeight: 88, width: "100%", color: "var(--sp-text)", background: "var(--sp-surface)", font: "inherit" }} /></label>
    <label>Krets og kanal<select aria-label="Krets og kanal" value={channel} disabled={busy || Boolean(item.admission)} onChange={event => {
      setChannel(event.currentTarget.value); void target.edit(item.id, event.currentTarget.value, text);
    }} style={{ minHeight: 44, width: "100%", color: "var(--sp-text)", background: "var(--sp-surface)" }}>
      <option value="">Vel kanal</option>{destinations.map(entry => <option key={entry.id} value={entry.id}>{entry.name}</option>)}
    </select></label>
    {!destinations.length && <Status>Du har ingen kanal du kan skrive i. Delinga er teken vare på lokalt.</Status>}
    <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
      <Button disabled={busy || !channel || (!text.trim() && !item.file && !item.admission)} onClick={() => void target.send(item.id, channel, text)}>{item.admission ? "Prøv den opphavlege sendinga igjen" : "Send delinga"}</Button>
      {(!item.admission || item.rejected) && <Button variant="quiet" disabled={busy} onClick={() => void target.discard(item.id)}>Forkast delinga</Button>}
    </div>
  </section>;
}

export function ReceivedShares({ target, snapshot }: { target: ShareTarget; snapshot: ConversationSnapshot }) {
  const state = useSyncExternalStore(target.subscribe, target.getSnapshot);
  const [open, setOpen] = useState(location.pathname === "/share-target");
  useEffect(() => { void target.refresh().catch(() => {}); }, [target]);
  useEffect(() => {
    const refresh = () => { void target.refresh().catch(() => {}); };
    window.addEventListener("focus", refresh); document.addEventListener("visibilitychange", refresh);
    navigator.serviceWorker?.addEventListener("message", refresh);
    return () => { window.removeEventListener("focus", refresh); document.removeEventListener("visibilitychange", refresh); navigator.serviceWorker?.removeEventListener("message", refresh); };
  }, [target]);
  return <>
    {(state.receipts.length > 0 || state.notice || state.error) && <Button variant="quiet" onClick={() => setOpen(true)}>Motteke deling{state.receipts.length ? ` (${state.receipts.length})` : ""}</Button>}
    <Dialog open={open} title="Motteke deling" closeLabel="Lukk delinga" onClose={() => setOpen(false)}>
      <p>Vel kvar du vil sende. Vanlege samtaleutkast blir tekne vare på. Ingenting blir sendt før du trykkjer Send.</p>
      {state.error && <Status tone="error">{state.error}</Status>}
      {state.notice && <Status>{state.notice}</Status>}
      {!state.receipts.length && !state.notice && <Status>Ingen delingar ventar. Ved nettfeil kan du opne Sprøyt igjen; lokalt lagra delingar blir tekne vare på.</Status>}
      {state.receipts.map(item => <ShareDraft key={`${item.id}:${item.owner}`} item={item} target={target} snapshot={snapshot} busy={state.busy} />)}
    </Dialog>
  </>;
}
