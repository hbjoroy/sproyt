import { Button } from "@sproyt/ui/react";
import { useId } from "react";

export type GettingStartedProps = {
  mode: "welcome" | "help";
  onFindChannels: () => void;
  onNewDirect: () => void;
  onInvitation: () => void;
  onProfile: () => void;
};

/** The host decides when this guide is appropriate and owns each action. */
export function GettingStarted({ mode, onFindChannels, onNewDirect, onInvitation, onProfile }: GettingStartedProps) {
  const headingId = useId();
  return <section className="sp-getting-started" aria-labelledby={headingId}>
    <header className="sp-getting-started-heading">
      <h1 id={headingId}>{mode === "welcome" ? "Velkomen til Sprøyt" : "Kom i gang med Sprøyt"}</h1>
      <p>{mode === "welcome" ? "Her kan du finne folk og samtalar som betyr noko for deg." : "Finn fram til folk og samtalar på den måten som passar deg."}</p>
    </header>
    <ul className="sp-getting-started-explanations">
      <li>Krets samlar folk.</li>
      <li>Kanalar samlar samtalar om eit tema.</li>
      <li>Direktemeldingar går til valde personar.</li>
    </ul>
    <div className="sp-getting-started-actions">
      <Button variant="primary" onClick={onFindChannels}>Finn kretsar og kanalar</Button>
      <Button onClick={onNewDirect}>Ny direktemelding</Button>
      <Button variant="quiet" onClick={onInvitation}>Bruk invitasjonslenkje eller kode</Button>
    </div>
    <Button className="sp-getting-started-profile" variant="quiet" onClick={onProfile}>Sjå profilen din</Button>
  </section>;
}

export type DirectoryEmptyStateProps = {
  scope: "circles" | "direct";
  searching: boolean;
  onFindChannels?: () => void;
  onNewDirect?: () => void;
  onInvitation?: () => void;
};

/** Search emptiness does not imply that the directory itself is empty. */
export function DirectoryEmptyState({ scope, searching, onFindChannels, onNewDirect, onInvitation }: DirectoryEmptyStateProps) {
  if (searching) return <div className="sp-directory-empty" role="status"><p>{scope === "circles" ? "Ingen kretsar eller kanalar passar søket." : "Ingen direktemeldingar passar søket."}</p><p>Prøv eit anna søk.</p></div>;
  return <div className="sp-directory-empty">
    <p>{scope === "circles" ? "Ingen kretsar eller kanalar enno." : "Ingen direktemeldingar enno. Start ei ny samtale."}</p>
    {scope === "circles" && <p>Ein krets samlar folk. Inne i kretsen finn du kanalar for ulike samtalar. Direktemeldingar går til valde personar.</p>}
    <div className="sp-directory-empty-actions">
      {scope === "circles" && onFindChannels && <Button variant="quiet" onClick={onFindChannels}>Finn kretsar og kanalar</Button>}
      {scope === "circles" && onInvitation && <Button variant="quiet" onClick={onInvitation}>Bruk invitasjonslenkje eller kode</Button>}
      {scope === "direct" && onNewDirect && <Button variant="quiet" onClick={onNewDirect}>Ny direktemelding</Button>}
    </div>
  </div>;
}
