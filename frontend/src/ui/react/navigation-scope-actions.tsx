import { Button } from "@sproyt/ui/react";
import { useRef, useState, type ReactNode } from "react";
import type { ConversationSnapshot, ConversationSnapshotGroup } from "../../application/conversation-snapshot";
import { PreviewCommunity, type CommunityDestination, type CommunityHost } from "./preview-community";

/** Navigation opens existing host-owned flows; it never mutates on mount. */
export function NavigationScopeActions({ group, snapshot, host, onSelect }: {
  group: ConversationSnapshotGroup; snapshot: ConversationSnapshot; host: CommunityHost;
  onSelect(channelId: string): void;
}) {
  const [destination, setDestination] = useState<CommunityDestination>();
  const trigger = useRef<HTMLButtonElement | null>(null);
  if (group.id === "scope:direct") return null;
  const circle = group.circle;
  const close = () => {
    setDestination(undefined);
    requestAnimationFrame(() => { if (trigger.current?.isConnected) trigger.current.focus({ preventScroll: true }); });
  };
  const action = (label: string, target: CommunityDestination, symbol: ReactNode) => <Button
    className="sp-scope-action" variant="quiet" aria-label={label} title={label}
    onClick={event => {
      event.currentTarget.focus({ preventScroll: true });
      trigger.current = event.currentTarget;
      setDestination(target);
    }}><span aria-hidden="true">{symbol}</span></Button>;
  return <>
    <div className="sp-scope-actions">
      {action(`Finn kanalar i ${group.name}`, circle ? { kind: "channels", circleId: circle.id } : { kind: "global-channels" }, "#")}
      {action(`Ny kanal i ${group.name}`, { kind: "create-channel", circleId: circle?.id ?? null }, "+")}
      {(!circle || circle.role === "owner") && action(circle ? `Inviter til ${group.name}` : "Inviter ny brukar til Sprøyt",
        circle ? { kind: "invite", circleId: circle.id } : { kind: "global-invite" }, <svg className="sp-channel-members-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false"><circle cx="8" cy="7" r="3" /><path d="M2 21v-3a6 6 0 0 1 12 0v3M19 6v8M15 10h8" /></svg>)}
    </div>
    {destination && <PreviewCommunity key={JSON.stringify(destination)} destination={destination} snapshot={snapshot} host={host}
      onNavigate={setDestination} onClose={close} onSelectChannel={channelId => { close(); onSelect(channelId); }} />}
  </>;
}
