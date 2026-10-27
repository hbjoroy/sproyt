/** Counts the independently confirmed inbox categories without combining them. */
export function inboxCounts(mentions: readonly { read: boolean }[], tasks: readonly { status: string }[]) {
  return {
    mentionCount: mentions.filter(mention => !mention.read).length,
    taskCount: tasks.filter(task => task.status !== "done").length,
  };
}

export const approximateCount = (count: number) => count < 25 ? String(count) : count < 50 ? "25+" : count < 100 ? "50+" : "100+";
