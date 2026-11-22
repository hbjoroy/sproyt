import { reactionEmoji } from "@sproyt/ui/react";

type Usage = { emoji: string; days: number[]; last: number };
const dayMs = 86_400_000;
const catalog: ReadonlyArray<readonly [string, string]> = [
  ["🍻", "Skål"], ...reactionEmoji, ["😀", "Stort smil, glad"],
];
const names = new Map(catalog);

function history(owner: string, now: number): Usage[] {
  try {
    const entries: unknown = JSON.parse(localStorage.getItem("sproyt.reaction-use.v1:" + owner) ?? "[]");
    if (!Array.isArray(entries)) return [];
    const cutoff = Math.floor(now / dayMs) - 89;
    return entries.slice(0, 64).flatMap(entry => {
      if (!entry || typeof entry.emoji !== "string" || !Number.isFinite(entry.last) || !Array.isArray(entry.days)) return [];
      const days = entry.days.filter((day: unknown): day is number => typeof day === "number" && Number.isInteger(day) && day >= cutoff);
      return days.length ? [{emoji: entry.emoji, days, last: entry.last}] : [];
    });
  } catch { return []; }
}

export function rememberReaction(owner: string, emoji: string, now = Date.now()): void {
  const entries = history(owner, now);
  const day = Math.floor(now / dayMs);
  const days = new Set(entries.find(entry => entry.emoji === emoji)?.days);
  days.add(day);
  try {
    const updated = [{ emoji, days: [...days], last: now }, ...entries.filter(entry => entry.emoji !== emoji)];
    localStorage.setItem("sproyt.reaction-use.v1:" + owner, JSON.stringify(updated.slice(0, 64)));
  } catch { /* Pinned choices still come from the account. */ }
}

export function reactionChoices(owner: string, pinned: readonly string[], now = Date.now()): ReadonlyArray<readonly [string, string]> {
  const entries = history(owner, now);
  const regular = entries.filter(entry => entry.days.length >= 2)
    .sort((a, b) => b.days.length - a.days.length || a.days[0]! - b.days[0]!).map(entry => entry.emoji);
  const recent = entries.find(entry => !pinned.includes(entry.emoji) && !regular.includes(entry.emoji));
  const stable = [...new Set([...pinned, ...regular, "😂", "❤️", "👍", "🍻", "🎉", "😊"])];
  const first = recent && pinned.length < 6 ? [...stable.filter(emoji => emoji !== recent.emoji).slice(0, 5), recent.emoji] : stable.slice(0, 6);
  return [...new Set([...first, ...stable, ...entries.map(entry => entry.emoji), ...names.keys()])]
    .map(emoji => [emoji, names.get(emoji) ?? emoji] as const);
}
