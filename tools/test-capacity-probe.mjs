import assert from 'node:assert/strict';
import test from 'node:test';
import { quantiles, trafficPlan, parseMetrics, histogramDelta, summarizeSends, withinObjectives } from './capacity-core.mjs';
import { fixtureMetadata, Fixture, applicationEnv } from './capacity-runtime.mjs';
import { options } from './capacity-probe.mjs';

const fixture = () => ({ Config: { Image: 'postgres:18.6-alpine', Labels: { 'sproyt.capacity-fixture': 'true' } },
  State: { Running: true }, Ports: { '5432/tcp': [{ HostIp: '127.0.0.1', HostPort: '55453' }] },
  HostConfig: { NanoCpus: 500000000, Memory: 1610612736 }, Mounts: [{ Type: 'volume' }], Image: 'sha256:synthetic' });

test('database writes require a labelled, loopback-only, bounded disposable fixture', () => {
  assert.equal(fixtureMetadata(fixture(), 55453).cpuLimit, .5);
  for (const mutate of [v => { v.Config.Labels = {}; }, v => { v.State.Running = false; },
    v => { v.Ports['5432/tcp'][0].HostIp = '0.0.0.0'; }, v => { v.Ports['5432/tcp'][0].HostPort = '5432'; },
    v => { v.Mounts.push({ Type: 'bind' }); }, v => { v.HostConfig.Memory = 0; }, v => { v.HostConfig.NanoCpus = 0; }]) {
    const value = fixture(); mutate(value); assert.throws(() => fixtureMetadata(value, 55453));
  }
  assert.throws(() => new Fixture('production', 55453));
});

test('cleanup rejects existing application database names before executing anything', async () => {
  const target = new Fixture('sproyt-353-capacity-test', 55453);
  await assert.rejects(target.drop('sproyt'));
  await assert.rejects(target.drop('postgres'));
  await assert.rejects(target.pg('sproyt', 'select 1'));
});

test('quiet traffic stays at two messages/second/channel when hot load increases', () => {
  for (const rate of [80, 160, 320, 640]) {
    const plan = trafficPlan(rate * 15, rate, 'hot');
    assert.equal(plan.length, rate * 15);
    for (let channel = 1; channel <= 4; channel++) {
      const quiet = plan.filter(e => e.channel === channel);
      assert.equal(quiet.length, 30);
      assert(quiet.every((e, i) => e.at === i * 500));
    }
    assert(plan.every((e, i) => e.index === i && (!i || e.at >= plan[i - 1].at)));
  }
});

test('uniform traffic is balanced and finite at the planned rate', () => {
  const plan = trafficPlan(1200, 80, 'uniform');
  assert.equal(plan.at(-1).at, 1199 * 12.5);
  for (let channel = 0; channel < 5; channel++) assert.equal(plan.filter(e => e.channel === channel).length, 240);
  assert.throws(() => trafficPlan(1200, Infinity, 'hot'));
});

test('scheduled latency includes generator delay and failed admissions stay in the denominator', () => {
  const result = summarizeSends([{ kind: 'hot', replica: 0, lagMs: 300, wireMs: 5, endToEndMs: 305 },
    { kind: 'quiet', replica: 1, lagMs: 350, error: 'client_inflight_limit' }], 3);
  assert.equal(result.accepted, 1); assert.equal(result.failed, 2);
  assert.equal(result.wireMs.p99, 5); assert.equal(result.scheduledToAckMs.p99, 305);
  assert.equal(result.schedulerLagMs.p99, 350);
  assert.equal(result.byKind.quiet.accepted, 0); assert.equal(result.errors.client_inflight_limit, 1);
});

const histogram = (count, first, sum, order = false) => parseMetrics([
  `sproyt_chat_phase_duration_seconds_bucket{${order ? 'le="0.001",phase="queued"' : 'phase="queued",le="0.001"'}} ${first}`,
  `sproyt_chat_phase_duration_seconds_bucket{phase="queued",le="0.01"} ${count}`,
  `sproyt_chat_phase_duration_seconds_bucket{phase="queued",le="+Inf"} ${count}`,
  `sproyt_chat_phase_duration_seconds_count{phase="queued"} ${count}`,
  `sproyt_chat_phase_duration_seconds_sum{phase="queued"} ${sum}`,
  'unrelated_metric 999', '# HELP ignored comment'
].join('\n'));

test('histograms subtract each replica baseline before aggregation, independent of label order', () => {
  const [result] = histogramDelta([histogram(10, 10, .01), histogram(20, 20, .02)],
    [histogram(12, 11, .015, true), histogram(22, 21, .025)]);
  assert.equal(result.count, 4); assert(Math.abs(result.meanMs - 2.5) < 1e-10);
  assert(result.estimatedP99Ms > 9 && result.estimatedP99Ms <= 10);
  assert.throws(() => histogramDelta([histogram(10, 10, .01)], [histogram(1, 1, .001)]), /reset/);
});

test('overflow histogram quantiles are unknown instead of understated', () => {
  const map = parseMetrics('sproyt_chat_phase_duration_seconds_bucket{phase="queued",le="0.001"} 1\nsproyt_chat_phase_duration_seconds_bucket{phase="queued",le="+Inf"} 100\nsproyt_chat_phase_duration_seconds_count{phase="queued"} 100\nsproyt_chat_phase_duration_seconds_sum{phase="queued"} 100');
  assert.equal(histogramDelta([new Map()], [map])[0].estimatedP99Ms, null);
  assert.equal(quantiles([]).p99, null);
  assert.equal(quantiles(Array.from({ length: 1000 }, (_, i) => i + 1)).p99, 990);
});

test('API topology fixes total pool capacity and enables exactly one local topic consumer', () => {
  const saved = process.env.SPROYT_HEART_URL; process.env.SPROYT_HEART_URL = 'https://must-not-inherit.invalid';
  try {
    const one = applicationEnv('synthetic', 55453, 12345, 0, 1);
    const two = [0, 1].map(i => applicationEnv('synthetic', 55453, 12345 + i, i, 2, 'http://127.0.0.1:45678'));
    assert.equal(one.SPROYT_DB_MAX_CONNECTIONS, '4');
    assert.equal(two.reduce((n, e) => n + Number(e.SPROYT_DB_MAX_CONNECTIONS), 0), 4);
    assert.deepEqual(two.map(e => e.SPROYT_CHANNEL_TOPICS_WORKER_ENABLED), ['true', 'false']);
    for (const env of [one, ...two]) {
      assert.equal(env.SPROYT_HEART_URL, undefined); assert.equal(env.SPROYT_CHAT_AGENTS_ENABLED, 'false');
      assert.equal(env.SPROYT_AUTH_MODE, 'development');
      assert(env.DATABASE_URL.startsWith('postgres://sproyt:capacity-fixture@127.0.0.1:55453/'));
    }
  } finally { if (saved === undefined) delete process.env.SPROYT_HEART_URL; else process.env.SPROYT_HEART_URL = saved; }
});

test('runner requires explicit isolation acknowledgement and rejects ambiguous or unbounded arguments', () => {
  const base = ['--confirm-isolated', '--binary', 'synthetic.exe'];
  assert.equal(options(base).maxInFlight, 128);
  for (const args of [['--binary', 'synthetic.exe'], [...base, '--rate', 'Infinity'], [...base, '--replicas', '3'],
    [...base, '--messages', '1000000'], [...base, '--output', '../outside.json'], [...base, '--rate'],
    [...base, '--unknown'], [...base, '--quick', '--quick']]) assert.throws(() => options(args));
});

test('passing aggregate latency cannot conceal slow quiet traffic or skipped recoveries', () => {
  const sends = summarizeSends([{kind:'hot',replica:0,lagMs:0,wireMs:5,endToEndMs:5},
    {kind:'quiet',replica:0,lagMs:0,wireMs:6,endToEndMs:6}],2);
  const recovery = {planned:100,attempted:100,failures:[],latencyMs:{count:100,p99:30}};
  assert.equal(withinObjectives({sends,recovery}),true);
  sends.byKind.quiet.scheduledToAckMs.p99=751;
  assert.equal(withinObjectives({sends,recovery}),false);
  sends.byKind.quiet.scheduledToAckMs.p99=6;
  assert.equal(withinObjectives({sends,recovery:{...recovery,attempted:99}}),false);
  assert.equal(withinObjectives({sends,recovery,stopReason:'resource_sampler_failed'}),false);
});
