import { ProfileAvatar } from "./profile-avatar";
import { Avatar, Button, Dialog, Status } from "@sproyt/ui/react";
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import type { LoadPersonCard, PersonCardData } from "../../person-card";
import { messageExcerpt } from "../../message-excerpt";
import { viewHref, type ViewTarget } from "../../view-history";

export interface PersonCardScope {
  owner: string;
  key: string;
  load: LoadPersonCard;
  openDirect(id: string): Promise<void>;
  channels: readonly { id: string; name: string }[];
  navigate(target: ViewTarget): void;
}

const OpenCard = createContext<((id: string) => void) | undefined>(undefined);

export function PersonCardButton({ id, name, children, className }: { id: string; name: string; children?: ReactNode; className?: string }) {
  const open = useContext(OpenCard);
  if (!open) return <>{children}</>;
  return <button type="button" className={className ?? "sp-person-card-action"} aria-haspopup="dialog"
    aria-label={`Vis personkort for ${name}`} onClick={() => open(id)}>{children ?? "Personkort"}</button>;
}

export function PersonCards({ scope, children }: { scope?: PersonCardScope; children: ReactNode }) {
  const [selected, setSelected] = useState<{ id: string; key: string } | null>(null);
  const key = scope?.key;
  const open = useCallback((id: string) => { if (key) setSelected({ id, key }); }, [key]);
  if (selected && selected.key !== key) setSelected(null);
  const active = selected?.key === key ? selected : null;
  return <OpenCard.Provider value={scope ? open : undefined}>
    {children}
    {scope && active && <PersonDialog key={`${scope.key}/${active.id}`} scope={scope} id={active.id} close={() => setSelected(current => current === active ? null : current)} />}
  </OpenCard.Provider>;
}

function PersonDialog({ scope, id, close }: { scope: PersonCardScope; id: string; close(): void }) {
  const [card, setCard] = useState<PersonCardData>();
  const [failed, setFailed] = useState(false);
  const [openingDirect, setOpeningDirect] = useState(false);
  const [directError, setDirectError] = useState("");

  async function openDirect() {
    if (openingDirect) return;
    setOpeningDirect(true);
    setDirectError("");
    try {
      await scope.openDirect(id);
      close();
    } catch {
      setDirectError("Kunne ikkje opne direktemeldinga. Prøv igjen.");
      setOpeningDirect(false);
    }
  }

  const [attempt, retry] = useState(0);
  const membership = scope.channels.map(channel => channel.id).sort().join(",");
  useEffect(() => {
    const controller = new AbortController();
    setCard(undefined); setFailed(false);
    void scope.load(scope.owner, id, controller.signal).then(data => {
      if (!controller.signal.aborted) setCard(data);
    }, () => { if (!controller.signal.aborted) setFailed(true); });
    const refresh = () => retry(value => value + 1);
    window.addEventListener("focus", refresh);
    return () => { controller.abort(); window.removeEventListener("focus", refresh); };
  }, [id, scope.load, scope.owner, membership, attempt]);
  const channels = new Map(scope.channels.map(c => [c.id, c.name]));
  const messages = card?.messages.filter(m => channels.has(m.channel_id)) ?? [];
  return <Dialog open title={card?.display_name ?? "Personkort"} closeLabel="Lukk personkortet" onClose={close}>
    {failed ? <><Status tone="error">Kunne ikkje hente personkortet.</Status><Button onClick={() => retry(value => value + 1)}>Prøv igjen</Button></>
      : !card ? <Status>Hentar personkort …</Status> : <section className="sp-person-card">
        <div className="sp-person-card-profile"><ProfileAvatar key={id} person={id} fallback={<Avatar name={card.display_name} />} />
          <div>{card.handle && <div>@{card.handle}</div>}
            {card.signup_ordinal !== null && <strong>Sprøyt-nummer {card.signup_ordinal}</strong>}
            <div>Med sidan {new Date(card.created_at).toLocaleDateString("nn-NO", { day: "numeric", month: "long", year: "numeric" })}</div>
          </div>
        </div>
        {(card.status_emoji || card.status_text) && <p>{[card.status_emoji, card.status_text].filter(Boolean).join(" ")}</p>}
        {id !== scope.owner && <Button busy={openingDirect} disabled={openingDirect} onClick={openDirect}>Send direktemelding</Button>}
        {directError && <Status tone="error">{directError}</Status>}
        <h3>Siste meldingar</h3>
        {messages.length === 0 && <p>Ingen meldingar i samtalane du har tilgang til.</p>}
        <ul>{messages.map(m => {
          const target = { kind: "conversation" as const, channelId: m.channel_id, messageId: m.id, sequence: m.sequence,
            threadId: m.parent_message_id ?? undefined };
          return <li key={m.id}><a href={viewHref(target, window.location.href)} onClick={event => {
            if (event.button || event.ctrlKey || event.shiftKey || event.metaKey || event.altKey) return;
            event.preventDefault(); close(); scope.navigate(target);
          }}><span>{channels.get(m.channel_id)}{m.parent_message_id && " · Tråd"}</span>
            {messageExcerpt(m.body).slice(0, 160) || "Opne meldinga"}</a></li>;
        })}</ul>
      </section>}
  </Dialog>;
}
