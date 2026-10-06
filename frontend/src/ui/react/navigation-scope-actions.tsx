import { Button } from "@sproyt/ui/react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import type { ConversationSnapshot, ConversationSnapshotGroup } from "../../application/conversation-snapshot";
import { CircleChatAgentsDialog } from "./circle-chat-agents";
import { AgentMemoryDialog } from "./agent-memory";
import { PreviewCommunity, type CommunityDestination, type CommunityHost } from "./preview-community";

/** Navigation opens existing host-owned flows; it never mutates on mount. */
export function NavigationScopeActions({ group, snapshot, host, onSelect }: {
  group: ConversationSnapshotGroup; snapshot: ConversationSnapshot; host: CommunityHost;
  onSelect(channelId: string): void;
}) {
  const [agentsOpen, setAgentsOpen] = useState(false);
  const [memoryOwner, setMemoryOwner] = useState<string>();
  const [destination, setDestination] = useState<CommunityDestination>();
  const trigger = useRef<HTMLButtonElement | null>(null);
  const panel = useRef<HTMLDivElement | null>(null);
  const [open, setOpen] = useState(false);
  const id = useId();
  useEffect(() => {
    if (!open) return;
    const dismiss = (event: Event) => {
      if (event.type === "scroll" && panel.current?.contains(event.target as Node)) return;
      panel.current?.hidePopover();
    };
    document.addEventListener("scroll", dismiss, true);
    window.addEventListener("resize", dismiss);
    return () => {
      document.removeEventListener("scroll", dismiss, true);
      window.removeEventListener("resize", dismiss);
    };
  }, [open]);
  const toggle = () => {
    const button = trigger.current;
    const menu = panel.current;
    if (!button || !menu) return;
    button.focus({ preventScroll: true });
    if (menu.matches(":popover-open")) { menu.hidePopover(); return; }
    const anchor = button.getBoundingClientRect();
    const width = Math.min(280, window.innerWidth - 16);
    menu.style.width = `${width}px`;
    menu.style.left = `${Math.max(8, Math.min(anchor.right - width, window.innerWidth - width - 8))}px`;
    menu.showPopover();
    const height = menu.getBoundingClientRect().height;
    menu.style.top = `${Math.max(8, Math.min(anchor.bottom + 4, window.innerHeight - height - 8))}px`;
    menu.querySelector<HTMLButtonElement>("button")?.focus({ preventScroll: true });
  };
  const circle = group.circle;
  useEffect(()=>{if (!circle || !["owner","moderator"].includes(circle.role)) setAgentsOpen(false);},[circle?.id,circle?.role]);
  if (group.id === "scope:direct") return null;
  const close = () => {
    setDestination(undefined);
    requestAnimationFrame(() => { if (trigger.current?.isConnected) trigger.current.focus({ preventScroll: true }); });
  };
  const action = (label: string, target: CommunityDestination, symbol: ReactNode, visibleLabel = label) => <Button
    className="sp-scope-menu-action" variant="quiet" aria-label={label}
    onClick={event => {
      event.currentTarget.focus({ preventScroll: true });
      panel.current?.hidePopover();
      trigger.current?.focus({ preventScroll: true });
      setDestination(target);
    }}><span className="sp-scope-menu-symbol" aria-hidden="true">{symbol}</span><span>{visibleLabel}</span></Button>;
  return <>
    <div className="sp-scope-actions">
      <Button ref={trigger} className="sp-scope-action" variant="quiet" aria-label={`Val for ${group.name}`}
        title={`Val for ${group.name}`} aria-expanded={open} aria-controls={id} onClick={toggle}><span aria-hidden="true">⋯</span></Button>
      <div ref={panel} id={id} popover="auto" className="sp-scope-menu" role="group" aria-label={`Val for ${group.name}`}
        onToggle={event => setOpen(event.newState === "open")}>
      {action(`Finn kanalar i ${group.name}`, circle ? { kind: "channels", circleId: circle.id } : { kind: "global-channels" }, "#", "Finn kanalar")}
      {action(`Ny kanal i ${group.name}`, { kind: "create-channel", circleId: circle?.id ?? null }, "+", "Ny kanal")}
      {circle && action(`Medlemmer og roller i ${group.name}`, {kind:"circle-members",circleId:circle.id}, "♙", "Medlemmer og roller")}
      {circle && ["owner","moderator"].includes(circle.role) && host.chatAgents && <Button className="sp-scope-menu-action" variant="quiet" aria-label={`Agentar i ${group.name}`}
        onClick={()=>{panel.current?.hidePopover();trigger.current?.focus({preventScroll:true});setAgentsOpen(true);}}><span className="sp-scope-menu-symbol" aria-hidden="true">⚙</span><span>Agentar</span></Button>}
      {circle?.role === "owner" && action(`Endre namn på ${group.name}`, { kind: "rename-circle", circleId: circle.id }, <svg className="sp-channel-members-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="m4 16 11-11 4 4-11 11-5 1 1-5ZM13 7l4 4" /></svg>, "Endre namn")}
      {(!circle || circle.role === "owner") && action(circle ? `Inviter til ${group.name}` : "Inviter ny brukar til Sprøyt",
        circle ? { kind: "invite", circleId: circle.id } : { kind: "global-invite" }, <svg className="sp-channel-members-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false"><circle cx="8" cy="7" r="3" /><path d="M2 21v-3a6 6 0 0 1 12 0v3M19 6v8M15 10h8" /></svg>, circle ? "Inviter personar" : "Inviter ny brukar")}
      {circle && host.agentMemory && host.selfId() && <Button className="sp-scope-menu-action" variant="quiet" aria-label={`Mitt agentminne i ${group.name}`}
        onClick={() => { panel.current?.hidePopover(); trigger.current?.focus({ preventScroll: true }); setMemoryOwner(host.selfId() ?? undefined); }}>
        <span className="sp-scope-menu-symbol" aria-hidden="true">◇</span><span>Mitt agentminne</span></Button>}
      </div>
    </div>
    {agentsOpen && circle && ["owner","moderator"].includes(circle.role) && host.chatAgents && <CircleChatAgentsDialog key={circle.id} api={host.chatAgents} circleId={circle.id} circleName={circle.name}
      onClose={()=>{setAgentsOpen(false);close();}} />}
    {memoryOwner && memoryOwner === host.selfId() && circle && host.agentMemory && <AgentMemoryDialog key={`${memoryOwner}:${circle.id}`} api={host.agentMemory}
      circleId={circle.id} circleName={circle.name} channels={snapshot.groups.flatMap(group => group.conversations.map(item => item.channel))}
      onClose={() => { setMemoryOwner(undefined); close(); }} />}
    {destination && <PreviewCommunity key={JSON.stringify(destination)} destination={destination} snapshot={snapshot} host={host}
      onNavigate={setDestination} onClose={close} onSelectChannel={channelId => { close(); onSelect(channelId); }} />}
  </>;
}
