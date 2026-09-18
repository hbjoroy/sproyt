import { Button } from "@sproyt/ui/react";
import { useCallback, useEffect, useSyncExternalStore } from "react";
import type { InvitationCards } from "../../application/invitation-cards";

export function InvitationCard({ token, host }: { readonly token: string; readonly host: InvitationCards }) {
  const subscribe = useCallback((listener: () => void) => host.subscribe(token, listener), [host, token]);
  const getSnapshot = useCallback(() => host.get(token), [host, token]);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  useEffect(() => { host.inspect(token); }, [host, token]);
  const invitation = state.invitation;
  const accepted = state.accepted || invitation?.response === "accepted";
  const declined = !accepted && invitation?.response === "declined";
  const authoredByMe = invitation?.invited_by === host.participantId();
  let detail = "Lastar invitasjonen …";
  if (invitation) {
    const responses = [invitation.accepted_count > 0 && `${invitation.accepted_count} har godteke`,
      invitation.declined_count > 0 && `${invitation.declined_count} har avvist`].filter(Boolean);
    detail = authoredByMe ? (responses.length ? `Du sende invitasjonen. ${responses.join(", ")}.` : "Du sende invitasjonen. Ventar på svar.")
      : accepted ? "Du har godteke invitasjonen." : declined ? "Du har avvist invitasjonen."
        : `${invitation.invited_by_name} har invitert deg.`;
  }
  if (state.pending) detail = state.pending === "accept_invitation" ? "Godtek invitasjonen …" : "Avviser invitasjonen …";
  return <section className={`invitation-card${declined ? " declined" : ""}`} data-react-invitation="true"
    aria-busy={Boolean(state.pending || state.loading)} aria-label="Invitasjon">
    {invitation && <h4>Invitasjon til {invitation.channel_name ? `kanalen ${invitation.channel_name}` : `vennekretsen ${invitation.circle_name}`}</h4>}
    <p role={state.error ? "alert" : "status"}>{state.error || detail}</p>
    {invitation && !accepted && !authoredByMe && !state.missing && <div className="invitation-actions">
      <Button disabled={Boolean(state.pending || state.loading)} onClick={() => host.respond(token, "accept_invitation")}>{declined ? "Godta likevel" : "Godta"}</Button>
      <Button disabled={Boolean(state.pending || state.loading || declined)} onClick={() => host.respond(token, "decline_invitation")}>Avvis</Button>
    </div>}
    {state.error && !state.missing && !invitation && <Button onClick={() => host.inspect(token, true)}>Prøv igjen</Button>}
  </section>;
}
