export type PendingInvitationResponse = Readonly<{
  token: string;
  command: "accept_invitation" | "decline_invitation";
}>;

export type PendingCircleInvitationRecipient = Readonly<{ circleId: string; userId: string }>;
export type PendingMessage = Readonly<{ channelId: string; body: string; draft: string; mediaIds: string[] }>;
export type PendingThreadReply = PendingMessage & Readonly<{ rootId: string }>;

/** One request registry per application, shared by presentation adapters.
 * Durable/uncertain sends deliberately survive response correlation: only the
 * existing acknowledgement and recovery policy may settle those entries.
 */
export function createPendingRequests() {
  const maps = {
    pendingCommands: new Map<string, string>(),
    pendingInvitationResponses: new Map<string, PendingInvitationResponse>(),
    pendingInvitationInspections: new Map<string, string>(),
    pendingChannelInvitationRecipients: new Map<string, string>(),
    pendingCircleInvitationRecipients: new Map<string, PendingCircleInvitationRecipient>(),
    pendingCircleShareInvitations: new Map<string, string>(),
    pendingCircleDirectInvitations: new Map<string, PendingCircleInvitationRecipient>(),
    pendingDirectInvitationMessages: new Map<string, string>(),
    pendingDirectChannelUsers: new Map<string, string>(),
    pendingPeopleDirectRequests: new Map<string, string>(),
    pendingMessages: new Map<string, PendingMessage>(),
    uncertainMessages: new Map<string, PendingMessage>(),
    pendingThreadReplies: new Map<string, PendingThreadReply>(),
    uncertainThreadReplies: new Map<string, PendingThreadReply>(),
    retriedUncertainRequests: new Set<string>(),
    historyRequestIds: new Set<string | undefined>()
  };

  return Object.freeze({
    ...maps,
    /** Capture and consume a transient response exactly once. */
    correlate(requestId: string | null | undefined) {
      const response = {
        requestedCommand: requestId ? maps.pendingCommands.get(requestId) : undefined,
        pendingInvitation: requestId ? maps.pendingInvitationResponses.get(requestId) : undefined,
        inspectedInvitationToken: requestId ? maps.pendingInvitationInspections.get(requestId) : undefined,
        invitationRecipient: requestId ? maps.pendingChannelInvitationRecipients.get(requestId) : undefined,
        circleInvitationRecipient: requestId ? maps.pendingCircleInvitationRecipients.get(requestId) : undefined,
        circleShareInvitation: requestId ? maps.pendingCircleShareInvitations.get(requestId) : undefined,
        directInvitationMessage: requestId ? maps.pendingDirectInvitationMessages.get(requestId) : undefined,
        directCircleInvitation: requestId ? maps.pendingCircleDirectInvitations.get(requestId) : undefined,
        directPeerUserId: requestId ? maps.pendingDirectChannelUsers.get(requestId) : undefined,
        directPersonUserId: requestId ? maps.pendingPeopleDirectRequests.get(requestId) : undefined
      };
      if (requestId) {
        maps.pendingCommands.delete(requestId);
        maps.pendingInvitationResponses.delete(requestId);
        maps.pendingInvitationInspections.delete(requestId);
        maps.pendingChannelInvitationRecipients.delete(requestId);
        maps.pendingCircleInvitationRecipients.delete(requestId);
        maps.pendingCircleShareInvitations.delete(requestId);
        maps.pendingDirectInvitationMessages.delete(requestId);
        maps.pendingCircleDirectInvitations.delete(requestId);
        maps.pendingDirectChannelUsers.delete(requestId);
        maps.pendingPeopleDirectRequests.delete(requestId);
      }
      return response;
    }
  });
}

export type PendingRequests = ReturnType<typeof createPendingRequests>;
