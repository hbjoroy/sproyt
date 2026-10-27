import { useEffect, useRef, useSyncExternalStore } from "react";
import { Button } from "@sproyt/ui/react";
import type { ConversationSnapshot } from "../../application/conversation-snapshot";
import { ViewHistory, viewHref, type ViewTarget } from "../../view-history";

export function viewTargetLabel(target: ViewTarget, snapshot: ConversationSnapshot): string | null {
  if (target.kind === "directory") return target.scope === "direct" ? "Direktemeldingar" : "Kretsar og kanalar";
  if (target.kind === "inbox") return { unread: "Uleste meldingar", mentions: "Omtalar", tasks: "Innboks" }[target.inbox];
  if (target.kind === "conversation") {
    const group = snapshot.groups.find(group => group.conversations.some(c => c.id === target.channelId));
    const channel = snapshot.channels.find(c => c.id === target.channelId);
    if (!channel) return null;
    const name = group?.conversations.find(c => c.id === channel.id)?.name ?? (channel.is_direct ? "Direktemelding" : `# ${channel.name}`);
    const circle = channel.circle_id ? snapshot.circles.find(c => c.id === channel.circle_id)?.name : null;
    return [circle ?? (channel.is_direct ? "Direkte" : "Felles"), name, target.threadId ? "tråd" : target.messageId ? "melding" : null].filter(Boolean).join(" · ");
  }
  const page = target.page;
  if ("circleId" in page && !snapshot.circles.some(c => c.id === page.circleId)) return null;
  if (page.kind === "overview") return "Innstillingar";
  if (page.kind === "welcome") return "Kom i gang";
  if (page.kind === "appearance") return "Utsjånad og bruk";
  if (page.kind === "help") return "Hjelp og om Sprøyt";
  if (page.kind === "profile") return "Profil og status";
  if (page.kind === "notifications") return "Varslingsinnstillingar";
  if (page.kind === "circle" || page.kind === "chat-agents" || page.kind === "memory") {
    const circle = snapshot.circles.find(c => c.id === page.circleId)!;
    if (page.kind === "chat-agents" && !["owner", "moderator"].includes(circle.role)) return null;
    if (page.kind === "chat-agents") {
      if (page.channelId && !snapshot.channels.some(c => c.id === page.channelId && c.circle_id === page.circleId)) return null;
      const panel = page.panel ? { overview: "Oversikt", behaviour: "Oppførsel", skills: "Ferdigheiter", memory: "Minne",
        vision: "Bilettolking", images: "Biletskaping", weather: "Vêr", sea: "Sjøtemperatur", ferry: "Ferger",
        handoff: "Samarbeid", channels: "Kanalar og tilgang" }[page.panel] : null;
      return [circle.name, "Agentar", page.agentId === "new" ? "Ny agent" : page.agentId ? "Agent" : null, panel].filter(Boolean).join(" · ");
    }
    return `${circle.name} · ${page.kind === "circle" ? "Kretsinnstillingar" : page.kind === "memory" ? "Mitt minne" : "Agentar"}`;
  }
  if (page.kind === "advanced") return "Integrasjonar";
  if (page.kind === "community") {
    const d = page.destination;
    if ("channelId" in d && !snapshot.channels.some(c => c.id === d.channelId)) return null;
    if ("circleId" in d && d.circleId && !snapshot.circles.some(c => c.id === d.circleId)) return null;
    return "Personar og fellesskap";
  }
  return null;
}

export function ViewHistoryControls({ history, snapshot, onCapture, onOpen, inMenu = false }: {
  history: ViewHistory; snapshot: ConversationSnapshot; onCapture(): void;
  onOpen(target: ViewTarget): void;
  inMenu?: boolean;
}) {
  const state = useSyncExternalStore(history.subscribe, history.getSnapshot);
  const menu = useRef<HTMLDetailsElement>(null);
  useEffect(() => { menu.current?.removeAttribute("open"); }, [state.entryId]);
  useEffect(() => {
    const dismiss = (event: PointerEvent | KeyboardEvent) => {
      if (!menu.current?.open) return;
      if (event instanceof KeyboardEvent) {
        if (event.key !== "Escape") return;
        event.preventDefault(); menu.current.querySelector("summary")?.focus();
      } else if (menu.current.contains(event.target as Node)) return;
      menu.current.removeAttribute("open");
    };
    document.addEventListener("pointerdown", dismiss); document.addEventListener("keydown", dismiss);
    return () => { document.removeEventListener("pointerdown", dismiss); document.removeEventListener("keydown", dismiss); };
  }, []);
  const currentKey = state.target ? viewHref(state.target, "https://navigation.invalid/") : "";
  const recent = state.recent.filter(item => viewHref(item.target, "https://navigation.invalid/") !== currentKey)
    .map(item => ({ ...item, label: viewTargetLabel(item.target, snapshot) })).filter(item => item.label).slice(0, 12);
  return <div className={`sp-view-history${inMenu ? " sp-view-history-in-menu" : ""}`} aria-label="Navigasjonshistorikk">
    {!inMenu && <Button variant="symbol" aria-label="Tilbake" title="Tilbake" disabled={!state.canBack} onClick={() => { onCapture(); history.back(); }}><span aria-hidden="true">←</span></Button>}
    {!inMenu && <Button variant="symbol" aria-label="Fram" title="Fram" disabled={!state.canForward} onClick={() => { onCapture(); history.forward(); }}><span aria-hidden="true">→</span></Button>}
    <details ref={menu} className="sp-recent-views" onToggle={() => {
      const panel = menu.current?.querySelector<HTMLElement>(".sp-recent-views-menu");
      if (!menu.current?.open || !panel) return;
      const anchor = menu.current.getBoundingClientRect(), bounds = panel.getBoundingClientRect();
      panel.style.left = `${Math.max(8, Math.min(anchor.left, window.innerWidth - bounds.width - 8))}px`;
      panel.style.top = `${Math.max(8, Math.min(anchor.bottom + 4, window.innerHeight - bounds.height - 8))}px`;
    }}><summary>{inMenu ? "Nyleg opna" : "Nyleg"}</summary><div className="sp-recent-views-menu">
      {recent.length ? recent.map(item => <button type="button" key={viewHref(item.target, "https://navigation.invalid/")} onClick={() => {
        menu.current?.removeAttribute("open"); onOpen(item.target);
      }}>{item.label}</button>) : <p>Stader du opnar, blir viste her. Historikken er berre din på denne eininga.</p>}
    </div></details>
  </div>;
}
