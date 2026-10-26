import { Button } from "@sproyt/ui/react";
import { useRef, useState } from "react";
import type { ConversationSnapshot } from "../../application/conversation-snapshot";
import { PreviewCommunity, type CommunityHost } from "./preview-community";

/** Available before the first direct conversation exists, including in search. */
export function NewDirectMessage({ snapshot, host, onStarted, onOpen }: {
  snapshot: ConversationSnapshot; host: CommunityHost; onStarted(): void;
  onOpen?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const started = useRef(false);
  const close = () => {
    setOpen(false);
    if (!started.current) requestAnimationFrame(() => trigger.current?.focus({ preventScroll: true }));
  };
  return <>
    <Button ref={trigger} variant="quiet" onClick={event => { event.currentTarget.focus({ preventScroll: true }); if (onOpen) { onOpen(); return; } started.current = false; setOpen(true); }}>
      Ny direktemelding
    </Button>
    {open && <PreviewCommunity destination={{ kind: "people" }} snapshot={snapshot}
      host={{ ...host, async openDirect(userId) {
        await host.openDirect(userId);
        started.current = true;
        onStarted();
      } }} onClose={close} onNavigate={() => {}} />}
  </>;
}
