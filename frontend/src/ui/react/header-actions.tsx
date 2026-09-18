import { Button } from "@sproyt/ui/react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";

/** Keep the same controls and open dialogs mounted across compact/list layouts. */
export function HeaderActions({ children, primary, badge }: { readonly children: ReactNode; readonly primary?: ReactNode; readonly badge?: ReactNode }) {
  const [open, setOpen] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  useEffect(() => {
    if (!open) return;
    const dismiss = (event: PointerEvent | KeyboardEvent) => {
      // A dialog opened from the toolbar owns dismissal until it closes.
      if (document.querySelector("dialog:modal")) return;
      if (event instanceof KeyboardEvent) {
        if (event.key !== "Escape") return;
        event.preventDefault();
        setOpen(false);
        trigger.current?.focus({ preventScroll: true });
      } else if (!container.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", dismiss);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("keydown", dismiss);
    };
  }, [open]);
  return <div className="sp-header-actions" ref={container}>
    <span className="sp-brand sp-sproyt-brand">
      <img src="/assets/sproyt-wave-icon-512.png" alt="" aria-hidden="true" />
      <span>Sprøyt</span>
    </span>
    {badge}
    <div className="sp-header-primary">{primary}
    <Button className="sp-header-toggle" variant="quiet" ref={trigger} aria-expanded={open} aria-controls={id}
      onClick={() => setOpen(value => !value)}>Meny <span aria-hidden="true">☰</span></Button></div>
    <div id={id} className="sp-header-panel" data-open={open} role="group" aria-label="Handlingar">
      {children}
    </div>
  </div>;
}
