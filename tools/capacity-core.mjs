import assert from 'node:assert/strict';

export function quantiles(values) {
  const sorted = values.filter(Number.isFinite).toSorted((a, b) => a - b);
  const at = p => sorted.length ? sorted[Math.max(0, Math.ceil(p * sorted.length) - 1)] : null;
  return { count: sorted.length, min: sorted[0] ?? null, p50: at(.5), p95: at(.95), p99: at(.99), max: sorted.at(-1) ?? null };
}

export function trafficPlan(count, rate, profile, quietRate = 2) {
  assert(Number.isInteger(count) && count > 0 && count <= 100_000);
  assert(Number.isFinite(rate) && rate > quietRate * 4 && rate <= 4000);
  const events = [];
  if (profile === 'uniform') {
    for (let i = 0; i < count; i++) events.push({ at: i * 1000 / rate, channel: i % 5, kind: 'uniform' });
  } else {
    for (let channel = 1; channel < 5; channel++) {
      for (let at = 0; at < count * 1000 / rate; at += 1000 / quietRate) events.push({ at, channel, kind: 'quiet' });
    }
    for (let at = 0; at < count * 1000 / rate; at += 1000 / (rate - 4 * quietRate)) events.push({ at, channel: 0, kind: 'hot' });
    events.sort((a, b) => a.at - b.at || b.channel - a.channel);
  }
  return events.slice(0, count).map((event, index) => ({ ...event, index }));
}

export function parseMetrics(text) {
  const rows = new Map();
  for (const line of text.split('\n')) {
    const match = /^(\w+)(?:\{(.*)\})?\s+([-+.\deE]+)$/.exec(line);
    if (!match || !match[1].startsWith('sproyt_')) continue;
    const labels = Object.fromEntries([...((match[2] ?? '').matchAll(/(\w+)="((?:\\.|[^"\\])*)"/g))].map(m => [m[1], JSON.parse(`"${m[2]}"`)]));
    const key = match[1] + JSON.stringify(Object.entries(labels).toSorted());
    rows.set(key, { name: match[1], labels, value: Number(match[3]) });
  }
  return rows;
}

export function histogramDelta(before, after) {
  const groups = new Map();
  for (let replica = 0; replica < after.length; replica++) {
    for (const [key, row] of after[replica]) {
      if (!/^(sproyt_pool_selected_acquire|sproyt_chat_phase)_duration_seconds_(bucket|count|sum)$/.test(row.name)) continue;
      const delta = row.value - (before[replica]?.get(key)?.value ?? 0);
      assert(delta >= -1e-8, 'metrics reset during sample');
      const base = row.name.replace(/_(bucket|count|sum)$/, '');
      const labels = Object.fromEntries(Object.entries(row.labels).filter(([key]) => key !== 'le').toSorted());
      const groupKey = base + JSON.stringify(labels);
      const group = groups.get(groupKey) ?? { name: base, labels, count: 0, sumSeconds: 0, buckets: new Map() };
      if (row.name.endsWith('_bucket')) {
        const bound = row.labels.le === '+Inf' ? Infinity : Number(row.labels.le);
        group.buckets.set(bound, (group.buckets.get(bound) ?? 0) + delta);
      } else if (row.name.endsWith('_count')) group.count += delta;
      else group.sumSeconds += delta;
      groups.set(groupKey, group);
    }
  }
  return [...groups.values()].filter(g => g.count > 0).map(group => {
    const buckets = [...group.buckets].sort((a, b) => a[0] - b[0]);
    const estimate = p => {
      let lower = 0, previous = 0;
      for (const [upper, cumulative] of buckets) {
        if (cumulative >= p * group.count) {
          if (!Number.isFinite(upper)) return null; // The overflow bucket provides no upper-bound quantile.
          return (lower + (upper - lower) * (p * group.count - previous) / Math.max(1, cumulative - previous)) * 1000;
        }
        lower = upper; previous = cumulative;
      }
      return null;
    };
    return { name: group.name, labels: group.labels, count: group.count, meanMs: group.sumSeconds * 1000 / group.count,
      estimatedP50Ms: estimate(.5), estimatedP95Ms: estimate(.95), estimatedP99Ms: estimate(.99) };
  });
}

export function summarizeSends(records, planned) {
  const successful = records.filter(r => !r.error);
  return { planned, accepted: successful.length, failed: planned - successful.length,
    wireMs: quantiles(successful.map(r => r.wireMs)), scheduledToAckMs: quantiles(successful.map(r => r.endToEndMs)),
    schedulerLagMs: quantiles(records.filter(r => Number.isFinite(r.lagMs)).map(r => r.lagMs)),
    byKind: Object.fromEntries([...new Set(records.map(r => r.kind))].map(kind => [kind, summarizeGroup(records.filter(r => r.kind === kind))])),
    byReplica: Object.fromEntries([...new Set(records.map(r => r.replica))].map(replica => [replica, summarizeGroup(records.filter(r => r.replica === replica))])),
    errors: Object.fromEntries([...new Set(records.filter(r => r.error).map(r => r.error))].map(error => [error, records.filter(r => r.error === error).length])) };
}
function summarizeGroup(records) {
  return { planned: records.length, accepted: records.filter(r => !r.error).length, wireMs: quantiles(records.filter(r => !r.error).map(r => r.wireMs)),
    scheduledToAckMs: quantiles(records.filter(r => !r.error).map(r => r.endToEndMs)) };
}
// Client-admission drops and unattempted recoveries are not server rejections.
export function withinObjectives({ sends, recovery, stopReason }) {
  const latencyOk = stats => stats.count > 0 && stats.p99 !== null && stats.p99 <= 750;
  return !stopReason && sends.failed === 0 && latencyOk(sends.scheduledToAckMs)
    && Object.values(sends.byKind).every(group => group.accepted === group.planned && latencyOk(group.scheduledToAckMs))
    && sends.schedulerLagMs.count > 0 && sends.schedulerLagMs.p99 <= 100
    && recovery.attempted === recovery.planned && recovery.failures.length === 0
    && recovery.latencyMs.count === recovery.planned && recovery.latencyMs.p99 <= 5000;
}
