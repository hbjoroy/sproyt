import { Button } from "@sproyt/ui/react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { ConnectionLogoMark, type ConnectionHealthState } from "./connection-health";

export interface CompactConversationHeader {
  readonly context?: string;
  readonly title: string;
  readonly onBack: () => void;
}

function shortenMiddle(value: string, maximum = 14): { readonly head: string; readonly tail?: string } {
  if (value.length <= maximum) return { head: value };
  const tail = 3;
  return { head: value.slice(0, maximum - tail - 1).trimEnd(), tail: value.slice(-tail) };
}

/** Keep the same controls and open dialogs mounted across compact/list layouts. */
export function HeaderActions({ children, primary, compactConversation, navigationKey, onSettings, onConversations, onLatest, connection }: {
  readonly children: ReactNode;
  readonly primary?: ReactNode;
  readonly compactConversation?: CompactConversationHeader;
  readonly navigationKey?: string;
  readonly onSettings: () => void;
  readonly onConversations: () => void;
  readonly onLatest: () => void;
  readonly connection: ConnectionHealthState;
}) {
  const [conversationOpen, setConversationOpen] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const conversationTrigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  useEffect(() => {
    setConversationOpen(false);
    const frame = requestAnimationFrame(() => {
      if (document.querySelector("dialog:modal, [data-workspace-page]")) return;
      const active = document.activeElement;
      if (active === document.body || (active instanceof HTMLElement && !active.getClientRects().length))
        trigger.current?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [navigationKey]);
  useEffect(() => {
    if (!conversationOpen) return;
    const dismiss = (event: PointerEvent | KeyboardEvent) => {
      // A dialog opened from the toolbar owns dismissal until it closes.
      if (document.querySelector("dialog:modal")) return;
      if (event instanceof KeyboardEvent) {
        if (event.key !== "Escape") return;
        event.preventDefault();
        setConversationOpen(false);
        conversationTrigger.current?.focus({ preventScroll: true });
      } else if (!container.current?.contains(event.target as Node)) {
        setConversationOpen(false);
      }
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", dismiss);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("keydown", dismiss);
    };
  }, [conversationOpen]);
  return <div className="sp-header-actions" ref={container}>
    <span title={connection.label} className="sp-brand sp-sproyt-brand sp-sproyt-brand-static">
      <img src="/assets/sproyt-wave-icon-512.png" alt="" aria-hidden="true" />
      <ConnectionLogoMark phase={connection.phase} />
      <span className="sp-sproyt-brand-label">Sprøyt</span>
    </span>
    <Button className="sp-sproyt-brand sp-sproyt-brand-action" variant="symbol"
      aria-label="Samtalar" title={`Samtalar. ${connection.label}`} aria-describedby={`${id}-connection`}
      onClick={() => { setConversationOpen(false); onConversations(); }}>
      <img src="/assets/sproyt-wave-icon-512.png" alt="" aria-hidden="true" />
      <ConnectionLogoMark phase={connection.phase} />
    </Button>
    {compactConversation && <div className="sp-mobile-conversation-context">
      <button ref={conversationTrigger} type="button" className="sp-mobile-conversation-title"
        aria-expanded={conversationOpen} aria-controls={`${id}-conversation-name`}
        aria-label={`${compactConversation.context ? `${compactConversation.context}, ` : ""}${compactConversation.title}. Vis fullt namn.`}
        title={[compactConversation.context, compactConversation.title].filter(Boolean).join(" · ")}
        onClick={() => setConversationOpen(value => !value)}>
        {compactConversation.context && <span className="sp-mobile-context-initial" aria-hidden="true">{compactConversation.context.trim().slice(0, 1).toLocaleUpperCase()}</span>}
        {(() => {
          const shortened = shortenMiddle(compactConversation.title);
          return <span className="sp-mobile-conversation-short-name" aria-hidden="true">
            <span>{shortened.head}</span>{shortened.tail && <><span>…</span><span>{shortened.tail}</span></>}
          </span>;
        })()}
      </button>
      <div id={`${id}-conversation-name`} className="sp-mobile-conversation-name" role="group" aria-label="Samtaleinformasjon" hidden={!conversationOpen}>
        {compactConversation.context && <span>{compactConversation.context}</span>}
        <strong>{compactConversation.title}</strong>
        <Button variant="quiet" onClick={() => { setConversationOpen(false); onLatest(); }}>Gå til siste</Button>
      </div>
    </div>}
    <span id={`${id}-connection`} className="sp-sr" role="status">{connection.label}</span>
    <div className="sp-header-primary">{primary}
    <Button className="sp-header-toggle" variant="quiet" ref={trigger}
      aria-label="Innstillingar" title="Innstillingar" onClick={() => { setConversationOpen(false); onSettings(); }}>
      <svg className="sp-settings-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="m9 3-.6 2.5-2 .9L4.2 5 2.5 8l1.8 1.7-.2 2.2L2 13l1.7 3 2.5-.6 1.8 1.3.2 2.6h3.5l.8-2.4 2.1-.8 2.1 1.3 1.8-3-1.8-1.8.2-2.1L21 9.3l-1.8-3-2.5.6-1.8-1.3L14.5 3Z" /><circle cx="11.5" cy="11.2" r="3.2" /></svg>
      <span className="sp-header-toggle-label">Innstillingar</span></Button></div>
    <div hidden className="sp-header-destination-hosts">
      {children}
    </div>
  </div>;
}
