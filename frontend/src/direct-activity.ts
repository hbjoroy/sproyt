import type { DirectActivity } from "./types";

const hour = 60 * 60 * 1000;
export const DIRECT_ACTIVE_WINDOW = 36 * hour;
const halfLife = 30 * 24 * hour;

type RankedConversation = { readonly id: string; readonly directActivity?: DirectActivity | null };

/** Server history, not local visits. Use one clock for the whole comparison. */
export function rankDirectConversations<T extends RankedConversation>(items: readonly T[], now: number): T[] {
  const ranked = items.map(item => {
    const activity = item.directActivity;
    const parsed = activity?.last_message_at ? Date.parse(activity.last_message_at) : NaN;
    const last = Number.isFinite(parsed) ? Math.min(parsed, now) : -Infinity;
    const calculated = activity ? Date.parse(activity.calculated_at) : NaN;
    const score = activity && Number.isFinite(calculated) && Number.isFinite(activity.score)
      ? Math.max(0, activity.score) * 2 ** (-Math.max(0, now - calculated) / halfLife) : 0;
    return { item, last, score, active: last >= now - DIRECT_ACTIVE_WINDOW };
  });
  return ranked.sort((a, b) => Number(b.active) - Number(a.active)
    || (a.active && b.active ? b.last - a.last : b.score - a.score)
    || (a.last === b.last ? 0 : b.last - a.last)
    || (a.item.id < b.item.id ? -1 : a.item.id > b.item.id ? 1 : 0)).map(({ item }) => item);
}

/** Keep existing targets still while interacting; insert genuinely new rows. */
export function retainDirectOrder<T extends { readonly id: string }>(ranked: readonly T[], retained: readonly string[]): T[] {
  const items = new Map(ranked.map(item => [item.id, item]));
  const result: T[] = [];
  for (const id of retained) {
    const item = items.get(id);
    if (item) { result.push(item); items.delete(id); }
  }
  return [...result, ...items.values()];
}
