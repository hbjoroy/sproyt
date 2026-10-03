import type { ChatMessage } from "../types";

/** Paging covers the channel sequence, including replies hidden from the root
 * timeline. Never derive this cursor from the rendered root messages. */
export function historyPage(messages: readonly ChatMessage[], limit: number) {
  return {
    before: messages.length ? Math.min(...messages.map(message => message.sequence)) : null,
    hasMore: messages.length === limit,
    hasRoots: messages.some(message => message.parent_message_id === null && !message.deleted_at)
  };
}
