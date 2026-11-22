import { createContext, useEffect, useState, type ReactNode } from "react";
import { KretsromShell } from "@sproyt/ui/react";

export const KretsromContext = createContext<{ tabs: HTMLDivElement | null; workshop: HTMLDivElement | null } | null>(null);

interface Props {
  view: "list" | "detail";
  masthead: ReactNode;
  directory: ReactNode;
  title: string;
  context?: string;
  showRoom: boolean;
  roomId?: string | null;
  backLabel: string;
  onBack(): void;
  actions: ReactNode;
  children: ReactNode;
}

export function KretsromLayout(props: Props) {
  const [tabs, setTabs] = useState<HTMLDivElement | null>(null);
  const [workshop, setWorkshop] = useState<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!tabs) return;
    const closeAfterSelection = (event: MouseEvent) => {
      if (!(event.target instanceof Element) || !event.target.closest('[role="tab"]')) return;
      const details = tabs.closest("details");
      if (details) {
        details.open = false;
        details.querySelector("summary")?.focus();
      }
    };
    tabs.addEventListener("click", closeAfterSelection);
    return () => tabs.removeEventListener("click", closeAfterSelection);
  }, [tabs]);
  return <KretsromContext.Provider value={{ tabs, workshop }}>
    <KretsromShell view={props.view} masthead={props.masthead} directory={props.directory}
      roomHeader={props.showRoom && <header className="kr-room-header">
        <button type="button" className="kr-back" onClick={props.onBack}>‹ {props.backLabel}</button>
        <details className="kr-room-options" key={props.roomId} onKeyDown={event => {
          if (event.key === "Escape" && event.currentTarget.open && !document.querySelector("dialog:modal")) {
            event.currentTarget.open = false;
            event.currentTarget.querySelector("summary")?.focus();
          }
        }}>
          <summary aria-label={`Samtaleval for ${props.title}`}>
            {props.context && <strong>{props.context}</strong>}<span className="kr-room-title">{props.title}</span>
          </summary>
          <div className="kr-room-menu">
            <div ref={setTabs} />
            {props.actions}
          </div>
        </details>
      </header>}><div className="kr-reading-area"><div className="kr-reading-column">{props.children}</div>
        <div className="kr-workshop-host" hidden={!props.showRoom} ref={setWorkshop} /></div></KretsromShell>
  </KretsromContext.Provider>;
}
