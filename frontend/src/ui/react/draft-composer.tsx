import { Button, type Composer } from "@sproyt/ui/react";
import { useId, useLayoutEffect, useRef, useState, type ComponentProps } from "react";

/** Application-owned composition keeps attachments and text in one draft.
 * The library's visual vocabulary is retained; tools open only on request. */
export function DraftComposer({ value, onChange, onSend, label, leading, tools,
  toolsLabel = "Skriveverktøy", placeholder = "Skriv ei melding …", hint,
  disabled = false, busy = false, error, sendOnEnter = false,
  hasAttachments = false }: ComponentProps<typeof Composer> & { hasAttachments?: boolean }) {
  const id = useId();
  const field = useRef<HTMLTextAreaElement>(null);
  const [toolsOpen, setToolsOpen] = useState(false);
  const resize = () => {
    const element = field.current;
    if (!element) return;
    const available = window.visualViewport?.height ?? window.innerHeight;
    element.style.height = "0px";
    element.style.height = `${Math.min(Math.max(44, element.scrollHeight), Math.max(76, Math.min(240, available * .32)))}px`;
  };
  useLayoutEffect(resize, [value]);
  useLayoutEffect(() => {
    const element = field.current;
    if (!element) return;
    let width = -1;
    const observer = new ResizeObserver(entries => {
      const next = entries[0]?.contentRect.width ?? 0;
      if (next !== width) { width = next; resize(); }
    });
    observer.observe(element);
    window.visualViewport?.addEventListener("resize", resize);
    return () => { observer.disconnect(); window.visualViewport?.removeEventListener("resize", resize); };
  }, []);
  const submit = () => {
    if (!disabled && !busy && (value.trim() || hasAttachments)) onSend(value.trim());
  };
  return <form className="sp-composer sp-draft-composer" onSubmit={event => { event.preventDefault(); submit(); }}>
    <div className="sp-draft-context">{leading}</div>
    <label className="sp-label" htmlFor={id}>{label}</label>
    <textarea ref={field} id={id} className="sp-textarea" rows={1} value={value}
      onChange={event => onChange(event.currentTarget.value)} placeholder={placeholder}
      disabled={disabled || busy} aria-describedby={`${id}-help`} aria-invalid={Boolean(error) || undefined}
      onKeyDown={event => {
        if (event.key === "Enter" && !event.nativeEvent.isComposing && event.keyCode !== 229
          && (event.ctrlKey || event.metaKey || (sendOnEnter && !event.shiftKey))) {
          event.preventDefault(); submit();
        }
      }} />
    <span id={`${id}-help`} className={`sp-help sp-draft-help${error ? " sp-error" : ""}`} role={error ? "alert" : undefined}>{error || hint}</span>
    <div className="sp-composer-footer">
      {tools && <Button variant="quiet" aria-label={toolsLabel} title={toolsLabel} aria-controls={`${id}-tools`}
        aria-expanded={toolsOpen} onClick={() => setToolsOpen(open => !open)}>＋</Button>}
      <Button type="submit" variant="primary" disabled={disabled || (!value.trim() && !hasAttachments)} busy={busy}
        aria-label={hasAttachments && !value.trim() ? "Send vedlegg" : "Send ↑"} title="Send melding">
        <span aria-hidden="true">{busy ? "…" : "↑"}</span>
      </Button>
    </div>
    {tools && <div id={`${id}-tools`} className="sp-composer-tools" hidden={!toolsOpen}>{tools}</div>}
  </form>;
}
