import type { ChatMessage, ThreadSummary } from "../types";

/** Channel- and account-scoped summaries. Reset when either scope changes. */
export function createThreadSummaryState() {
  const summaries = new Map<string, ThreadSummary>();
  const reads = new Map<string, { rootId: string; sequence: number; mutationVersion: number }>();
  const confirmedRead = new Map<string, number>();
  // Server watermarks are distinct from the highest live event seen: live
  // events may arrive out of order and must each be counted once.
  const watermarks = new Map<string, number>();
  const live = new Map<string, Map<string, ChatMessage>>();
  const deleted = new Set<string>();
  let readVersion = 0;
  let mutationVersion = 0;
  let list: { requestId: string; readVersion: number; mutationVersion: number; live: Map<string, ChatMessage[]> } | null = null;

  const reset = () => { summaries.clear(); reads.clear(); confirmedRead.clear(); watermarks.clear(); live.clear(); deleted.clear(); list = null; readVersion = 0; mutationVersion = 0; };
  const disconnect = () => { reads.clear(); list = null; };
  const fail = (requestId: string) => { reads.delete(requestId); if (list?.requestId === requestId) list = null; };
  const beginList = (requestId: string | null) => { list = requestId ? { requestId, readVersion, mutationVersion, live: new Map() } : null; };
  const trackRead = (requestId: string | null, rootId: string, sequence: number) => {
    if (requestId) reads.set(requestId, { rootId, sequence, mutationVersion });
  };
  const rebase = (summary: ThreadSummary, known: readonly ChatMessage[], owner: string | null): ThreadSummary => {
    const rootId = summary.root_message_id;
    const unseen = new Map<string, ChatMessage>();
    for (const message of [...known, ...(live.get(rootId)?.values() ?? [])]) {
      if (message.parent_message_id === rootId && message.sequence > summary.latest_sequence) unseen.set(message.id, message);
    }
    const result = { ...summary };
    for (const message of unseen.values()) {
      result.latest_sequence = Math.max(result.latest_sequence, message.sequence);
      if (!message.deleted_at && !deleted.has(message.id)) {
        result.reply_count++;
        if (owner && message.sender_id !== owner) result.unread_count++;
      }
    }
    watermarks.set(rootId, summary.latest_sequence);
    const events = live.get(rootId);
    if (events) for (const [id, message] of events) if (message.sequence <= summary.latest_sequence) events.delete(id);
    return result;
  };
  const receiveList = (requestId: string | undefined, values: readonly ThreadSummary[],
    known: (rootId: string) => readonly ChatMessage[], owner: string | null): "applied" | "retry" | "ignored" => {
    if (!requestId || !list || list.requestId !== requestId) return "ignored";
    const requested = list; list = null;
    if (requested.readVersion !== readVersion || requested.mutationVersion !== mutationVersion) return "retry";
    const next = new Map<string, ThreadSummary>();
    for (const summary of values) next.set(summary.root_message_id, rebase(summary, known(summary.root_message_id), owner));
    // A new root can receive its first live reply while the list is in flight.
    for (const [rootId, replies] of requested.live) {
      if (!next.has(rootId)) next.set(rootId, rebase({ root_message_id: rootId, reply_count: 0, unread_count: 0, latest_sequence: 0 }, replies, owner));
    }
    summaries.clear();
    for (const [rootId, summary] of next) summaries.set(rootId, summary);
    return "applied";
  };
  const addReply = (message: ChatMessage, owner: string | null) => {
    const rootId = message.parent_message_id;
    if (!rootId || message.sequence <= (watermarks.get(rootId) ?? 0)) return;
    let events = live.get(rootId);
    if (!events) { events = new Map(); live.set(rootId, events); }
    if (events.has(message.id)) return;
    events.set(message.id, message);
    if (list) list.live.set(rootId, [...(list.live.get(rootId) ?? []), message]);
    const previous = summaries.get(rootId);
    const visible = !message.deleted_at && !deleted.has(message.id);
    summaries.set(rootId, { root_message_id: rootId, reply_count: (previous?.reply_count ?? 0) + (visible ? 1 : 0),
      unread_count: (previous?.unread_count ?? 0) + (visible && owner && message.sender_id !== owner ? 1 : 0),
      latest_sequence: Math.max(previous?.latest_sequence ?? 0, message.sequence) });
  };
  const deleteReply = (message: ChatMessage) => {
    if (!message.parent_message_id || deleted.has(message.id)) return;
    deleted.add(message.id);
    mutationVersion++;
  };
  const receiveRead = (requestId: string | undefined, summary: ThreadSummary,
    known: readonly ChatMessage[], owner: string | null): "applied" | "stale" | "retry" | "ignored" => {
    if (!requestId) return "ignored";
    const request = reads.get(requestId); reads.delete(requestId);
    if (!request || request.rootId !== summary.root_message_id) return "ignored";
    if (request.sequence <= (confirmedRead.get(request.rootId) ?? 0)) return "stale";
    confirmedRead.set(request.rootId, request.sequence);
    readVersion++;
    // An older snapshot cannot reconstruct counts already observed from a newer
    // server list, and a pre-deletion response must not resurrect deleted replies.
    if (request.mutationVersion !== mutationVersion || summary.latest_sequence < (watermarks.get(request.rootId) ?? 0)) return "retry";
    summaries.set(request.rootId, rebase(summary, known, owner));
    return "applied";
  };
  return Object.freeze({ summaries, reset, disconnect, fail, beginList, trackRead, receiveList, addReply, deleteReply, receiveRead });
}
