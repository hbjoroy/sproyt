import type { ReactNode } from "react";

export interface KretsromShellProps {
  view: "list" | "detail";
  masthead: ReactNode;
  directory: ReactNode;
  roomHeader?: ReactNode;
  children: ReactNode;
}

export function KretsromShell({ view, masthead, directory, roomHeader, children }: KretsromShellProps) {
  return <div className="kr-shell" data-view={view}>
    <header className="kr-mast">{masthead}</header>
    <div className="kr-layout">
      <nav className="kr-directory" aria-label="Samtalar">{directory}</nav>
      <main className="kr-room">{roomHeader}{children}</main>
    </div>
  </div>;
}

export interface KretsromMessageProps {
  author: string;
  authorContent?: ReactNode;
  time: string;
  dateTime?: string;
  avatar: ReactNode;
  status?: string;
  actions?: ReactNode;
  reactions?: ReactNode;
  children: ReactNode;
}

export function KretsromMessage(props: KretsromMessageProps) {
  return <article className="kr-message">
    <span aria-hidden="true" className="kr-avatar">{props.avatar}</span>
    <div className="kr-copy">
      <header className="kr-byline">{props.authorContent ?? <strong>{props.author}</strong>}
        <time className="sp-time" dateTime={props.dateTime}>{props.time}</time>{props.status && <small>{props.status}</small>}
      </header>
      <div className="kr-body">{props.children}</div>
      <div className="kr-reactions">{props.reactions}</div>
      <div className="kr-actions">{props.actions}</div>
    </div>
  </article>;
}

export function KretsromComposerRow({ tools, children, send }: {
  tools?: ReactNode; children: ReactNode; send: ReactNode;
}) {
  return <div className="kr-compose">{tools}{children}{send}</div>;
}
