import settingsWorkspaceCss from "../settings-workspace.css";
import { Button, Dialog } from "@sproyt/ui/react";
import { createContext, useContext, useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

type WorkspacePageContext = {
  pageKey?: string;
  outlet: HTMLDivElement | null;
  attach: (element: HTMLDivElement | null) => void;
};
const NavigationContext = createContext<{ content: ReactNode; overview: boolean } | null>(null);
export function DestinationNavigation({ children, navigation, overview }: { children: ReactNode; navigation: ReactNode; overview: boolean }) {
  return <NavigationContext.Provider value={navigation ? { content: navigation, overview } : null}>{children}</NavigationContext.Provider>;
}

const PageContext = createContext<WorkspacePageContext | null>(null);

/** Presentation only: the host still owns routing, authorization and operation tickets. */
export function WorkspacePageProvider({ pageKey, children }: { pageKey?: string; children: ReactNode }) {
  const [outlet, attach] = useState<HTMLDivElement | null>(null);
  return <PageContext.Provider value={{ pageKey, outlet, attach }}><style>{settingsWorkspaceCss}</style>{children}</PageContext.Provider>;
}

/** Keep this sibling outside the preserved conversation's hidden/inert subtree. */
export function WorkspacePageOutlet() {
  const context = useContext(PageContext);
  return <div ref={context?.attach} className="sp-workspace-outlet" hidden={!context?.pageKey} />;
}

export type DestinationPageProps = {
  open: boolean; title: string; closeLabel?: string; onClose(): void; children: ReactNode; focusHeading?: boolean;
};

/** Ordinary destinations render in main. Inline and standalone callers retain
 * the native modal contract when no host-owned page destination is active. */
export function DestinationPage({ open, title, closeLabel = "Tilbake", onClose, children, focusHeading = true }: DestinationPageProps) {
  const context = useContext(PageContext);
  const navigation = useContext(NavigationContext);
  const heading = useRef<HTMLHeadingElement>(null);
  const titleId = useId();
  const close = useRef(onClose); close.current = onClose;
  const pageKey = context?.pageKey, outlet = context?.outlet;
  useLayoutEffect(() => {
    if (!open || !pageKey || !outlet || !focusHeading) return;
    const page = heading.current?.closest("[data-workspace-page]");
    let interacted = false;
    const interaction = () => { interacted = true; };
    const removeListeners = () => {
      document.removeEventListener("pointerdown", interaction, true);
      document.removeEventListener("keydown", interaction, true);
      document.removeEventListener("focusin", interaction, true);
    };
    document.addEventListener("pointerdown", interaction, true);
    document.addEventListener("keydown", interaction, true);
    document.addEventListener("focusin", interaction, true);
    const frame = requestAnimationFrame(() => {
      removeListeners();
      // Navigation announces its heading only until the user starts working.
      // In WebKit, moving focus during input insertion can discard the input.
      if (!interacted && !page?.contains(document.activeElement)) heading.current?.focus({ preventScroll: true });
    });
    return () => { removeListeners(); cancelAnimationFrame(frame); };
  }, [open, pageKey, outlet, focusHeading]);
  useEffect(() => {
    if (!open || !pageKey || !outlet) return;
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || event.isComposing
        || event.target instanceof HTMLSelectElement || document.querySelector("dialog[open]")) return;
      event.preventDefault(); close.current();
    };
    // Page shortcuts run after local/header document handlers have had a chance
    // to dismiss their own popup and preventDefault().
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, [open, pageKey, outlet]);
  if (!pageKey) return <Dialog open={open} title={title} closeLabel={closeLabel} onClose={onClose}>{navigation?.overview ? navigation.content : children}</Dialog>;
  if (!open || !outlet) return null;
  return createPortal(<section className={`sp-workspace-page${navigation ? ` sp-settings-workspace${navigation.overview ? " sp-settings-workspace-overview" : ""}` : ""}`} role="region" aria-labelledby={titleId} data-workspace-page={pageKey}>
    <header className="sp-workspace-page-header">
      <Button variant="quiet" aria-label={closeLabel} onClick={onClose}><span aria-hidden="true">←</span> Tilbake</Button>
      <h1 ref={heading} id={titleId} tabIndex={-1}>{title}</h1>
    </header>
    <div className="sp-settings-workspace-layout">
      {navigation && <nav className="sp-settings-workspace-navigation" aria-label="Innstillingskategoriar">{navigation.content}</nav>}
      <div className="sp-workspace-page-body">{children}</div>
    </div>
  </section>, outlet);
}
