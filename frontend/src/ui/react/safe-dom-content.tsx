import { useLayoutEffect, useRef } from "react";

export interface SafeDomContentProps {
  /** Stable callback; include changed source data in its identity. */
  readonly render: (target: HTMLDivElement) => void | (() => void);
  readonly className?: string;
}

/** React owns this wrapper only. The existing safe Markdown/media renderer owns
 * its descendants and returns cleanup for asynchronous work and event handlers.
 * Do not relocate nodes owned by the host UI into this island. */
export function SafeDomContent({ render, className }: SafeDomContentProps) {
  const target = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = target.current;
    if (!element) return;
    const dispose = render(element);
    return () => {
      dispose?.();
      element.replaceChildren();
    };
  }, [render]);
  return <div ref={target} className={className} />;
}
