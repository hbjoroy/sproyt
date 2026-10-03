/** A read watermark means read up to the furthest visible sequence, including
 * an explicit jump. Loading messages never calls this policy. Server responses
 * confirm progress; failed/lost requests remain eligible for a later retry. */
export function createVisibleReadPolicy() {
  const confirmed = new Map<string, number>();
  const pending = new Map<string, { key: string; sequence: number }>();
  const confirm = (key: string, sequence: number) => {
    confirmed.set(key, Math.max(confirmed.get(key) ?? 0, sequence));
    for (const [id, request] of pending) {
      if (request.key === key && request.sequence <= sequence) pending.delete(id);
    }
  };
  const acknowledge = (key: string, sequence: number, send: () => string | null | undefined) => {
    if (sequence <= (confirmed.get(key) ?? 0)
      || [...pending.values()].some(request => request.key === key && request.sequence >= sequence)) return;
    const id = send();
    if (id) pending.set(id, { key, sequence });
  };
  const complete = (id: string | undefined) => {
    if (!id) return;
    const request = pending.get(id);
    if (request) confirm(request.key, request.sequence);
  };
  return Object.freeze({ acknowledge, confirm, complete,
    fail: (id: string) => pending.delete(id), disconnect: () => pending.clear() });
}
