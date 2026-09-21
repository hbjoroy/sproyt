import { Button } from "@sproyt/ui/react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";

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
export function HeaderActions({ children, primary, compactConversation }: {
  readonly children: ReactNode;
  readonly primary?: ReactNode;
  readonly compactConversation?: CompactConversationHeader;
}) {
  const [open, setOpen] = useState(false);
  const [conversationOpen, setConversationOpen] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const conversationTrigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  useEffect(() => {
    if (!open && !conversationOpen) return;
    const dismiss = (event: PointerEvent | KeyboardEvent) => {
      // A dialog opened from the toolbar owns dismissal until it closes.
      if (document.querySelector("dialog:modal")) return;
      if (event instanceof KeyboardEvent) {
        if (event.key !== "Escape") return;
        event.preventDefault();
        setOpen(false);
        setConversationOpen(false);
        (conversationOpen ? conversationTrigger : trigger).current?.focus({ preventScroll: true });
      } else if (!container.current?.contains(event.target as Node)) {
        setOpen(false);
        setConversationOpen(false);
      }
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", dismiss);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("keydown", dismiss);
    };
  }, [conversationOpen, open]);
  return <div className="sp-header-actions" ref={container}>
    <span className="sp-brand sp-sproyt-brand">
      <img src="/assets/sproyt-wave-icon-512.png" alt="" aria-hidden="true" />
      <span className="sp-sproyt-brand-label">Sprøyt</span>
    </span>
    {compactConversation && <div className="sp-mobile-conversation-context">
      <Button className="sp-mobile-conversation-back" variant="quiet" aria-label="Samtalar" title="Samtalar"
        onClick={compactConversation.onBack}><span aria-hidden="true">←</span></Button>
      <button ref={conversationTrigger} type="button" className="sp-mobile-conversation-title"
        aria-expanded={conversationOpen} aria-controls={`${id}-conversation-name`}
        aria-label={`${compactConversation.context ? `${compactConversation.context}, ` : ""}${compactConversation.title}. Vis fullt namn.`}
        title={[compactConversation.context, compactConversation.title].filter(Boolean).join(" · ")}
        onClick={() => { setOpen(false); setConversationOpen(value => !value); }}>
        {compactConversation.context && <span className="sp-mobile-context-initial" aria-hidden="true">{compactConversation.context.trim().slice(0, 1).toLocaleUpperCase()}</span>}
        {(() => {
          const shortened = shortenMiddle(compactConversation.title);
          return <span className="sp-mobile-conversation-short-name" aria-hidden="true">
            <span>{shortened.head}</span>{shortened.tail && <><span>…</span><span>{shortened.tail}</span></>}
          </span>;
        })()}
      </button>
      <span id={`${id}-conversation-name`} className="sp-mobile-conversation-name" role="tooltip" hidden={!conversationOpen}>
        {compactConversation.context && <span>{compactConversation.context}</span>}
        <strong>{compactConversation.title}</strong>
      </span>
    </div>}
    <div className="sp-header-primary">{primary}
    <Button className="sp-header-toggle" variant="quiet" ref={trigger} aria-expanded={open} aria-controls={id}
      aria-label="Meny" title="Meny" onClick={() => { setConversationOpen(false); setOpen(value => !value); }}><span className="sp-header-toggle-label">Meny</span> <span aria-hidden="true">☰</span></Button></div>
    <div id={id} className="sp-header-panel" data-open={open} role="group" aria-label="Handlingar">
      {children}
    </div>
  </div>;
}
