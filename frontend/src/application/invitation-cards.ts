export type Invitation = Readonly<{
  response?: "accepted" | "declined" | null;
  invited_by: string;
  invited_by_name: string;
  channel_name?: string | null;
  circle_name?: string | null;
  accepted_count: number;
  declined_count: number;
}>;

export type InvitationCardState = Readonly<{
  invitation?: Invitation;
  accepted?: boolean;
  loading?: boolean;
  pending?: "accept_invitation" | "decline_invitation";
  error?: string;
  missing?: boolean;
}>;

const initialState: InvitationCardState = { loading: true };

/** Shared by every occurrence of a token, including the thread's parent card.
 * Protocol events update state; React alone owns the rendered card. */
export function createInvitationCards(actions: {
  participantId: () => string | null;
  inspect: (token: string, force?: boolean) => void;
  respond: (token: string, command: "accept_invitation" | "decline_invitation") => void;
}) {
  const states = new Map<string, InvitationCardState>();
  const listeners = new Map<string, Set<() => void>>();
  return {
    ...actions,
    get: (token: string): InvitationCardState => states.get(token) ?? initialState,
    update(token: string, patch: Partial<InvitationCardState>) {
      states.set(token, { ...states.get(token), ...patch });
      listeners.get(token)?.forEach(listener => listener());
    },
    subscribe(token: string, listener: () => void) {
      const subscribers = listeners.get(token) ?? new Set<() => void>();
      subscribers.add(listener);
      listeners.set(token, subscribers);
      return () => {
        subscribers.delete(listener);
        if (subscribers.size === 0) listeners.delete(token);
      };
    },
    visibleTokens: () => [...listeners.keys()]
  };
}

export type InvitationCards = ReturnType<typeof createInvitationCards>;

export function invitationTokensFromMessage(body: string): string[] {
  return [...body.matchAll(/\[\[invite:([A-Za-z0-9_-]{32,128})\]\]/gu)].map(match => match[1]!);
}
