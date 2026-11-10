#!/usr/bin/env node
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { once } from 'node:events';
import { resolve } from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { performance } from 'node:perf_hooks';
import { setTimeout as sleep } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';
import { quantiles, trafficPlan, parseMetrics, histogramDelta, summarizeSends, withinObjectives } from './capacity-core.mjs';
import { run, freePort, Fixture, applicationEnv, startApi, monitorApis, openChat, waitUntil } from './capacity-runtime.mjs';

export function options(args) {
  const flags = new Set(['--confirm-isolated', '--quick', '--ramp']);
  const values = new Set(['--binary', '--fixture', '--pg-port', '--rate', '--messages', '--repetitions', '--replicas', '--profiles', '--recoveries', '--output']);
  const seen = new Set();
  for (let i = 0; i < args.length; i++) {
    const name = args[i]; assert(!seen.has(name), `duplicate option: ${name}`); seen.add(name);
    assert(flags.has(name) || values.has(name), `unknown option: ${name}`);
    if (values.has(name)) { assert(args[i + 1] && !args[i + 1].startsWith('--'), `missing value: ${name}`); i++; }
  }
  const value = (name, fallback) => { const i = args.indexOf(name); return i < 0 ? fallback : args[i + 1]; };
  assert(args.includes('--confirm-isolated'), 'pass --confirm-isolated; the runner writes only to its labelled disposable fixture');
  const binary = value('--binary'); assert(binary, '--binary is required');
  const quick = args.includes('--quick');
  const config = { binary: resolve(binary), fixture: value('--fixture', 'sproyt-353-capacity-pg'), port: Number(value('--pg-port', '55453')),
    rate: Number(value('--rate', quick ? '40' : '80')), messages: Number(value('--messages', quick ? '120' : '1200')),
    repetitions: Number(value('--repetitions', quick ? '1' : '3')), replicas: value('--replicas', quick ? '1' : '1,2').split(',').map(Number),
    profiles: value('--profiles', quick ? 'hot' : 'uniform,hot,background').split(','), recoveries: Number(value('--recoveries', quick ? '3' : '100')),
    minSeconds: quick ? 3 : 15, maxInFlight: 128, clients: 32, ramp: args.includes('--ramp'), quick,
    output: resolve(value('--output', '.local/353-capacity-evidence.json')) };
  assert(config.replicas.every(n => n === 1 || n === 2));
  assert(config.profiles.every(n => ['uniform', 'hot', 'background'].includes(n)));
  assert(Number.isInteger(config.rate) && config.rate >= 20 && config.rate <= 1000);
  assert(Number.isInteger(config.messages) && config.messages >= 100 && config.messages <= 20000);
  assert(Number.isInteger(config.repetitions) && config.repetitions >= 1 && config.repetitions <= 5);
  assert(Number.isInteger(config.recoveries) && config.recoveries >= 1 && config.recoveries <= 200);
  assert(config.output.startsWith(resolve('.local') + '\\') || config.output.startsWith(resolve('.local') + '/'), 'output must stay in .local');
  return config;
}

async function modelStub() {
  let release, active = 0, calls = 0, peak = 0; const gate = new Promise(resolve => { release = resolve; });
  const server = createServer(async (req, response) => {
    if (req.method !== 'POST' || req.url !== '/chat/completions') { response.writeHead(404).end(); return; }
    let bytes = 0; for await (const chunk of req) { bytes += chunk.length; if (bytes > 32768) { response.writeHead(413).end(); return; } }
    calls++; active++; peak = Math.max(peak, active);
    try { await gate; await sleep(100); response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ choices: [{ finish_reason: 'stop', message: { content: JSON.stringify({ keywords: ['prøve'] }) } }] })); }
    finally { active--; }
  });
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  return { url: `http://127.0.0.1:${server.address().port}`, release, stats: () => ({ calls, active, peak, responseDelayMs: 100 }), close() { release(); server.closeAllConnections(); server.close(); } };
}
const sum = (values, field) => values.reduce((total, value) => total + value[field], 0);
const uuid = value => { assert(/^[a-f0-9-]{36}$/.test(value)); return value; };
const statsSql = `select json_build_object('connections',(select count(*) from pg_stat_activity where datname=current_database() and backend_type='client backend' and pid<>pg_backend_pid() and application_name<>'capacity_observer'),
 'nonClientBackends',(select json_agg(x) from (select backend_type,count(*) as count from pg_stat_activity where datname=current_database() and backend_type<>'client backend' group by backend_type) x),
 'observerConnections',(select count(*) from pg_stat_activity where datname=current_database() and application_name='capacity_observer'),
 'byApplication',(select json_agg(x) from (select application_name,count(*) as connections from pg_stat_activity where datname=current_database() and backend_type='client backend' and pid<>pg_backend_pid() and application_name<>'capacity_observer' group by application_name) x),
 'bytes',pg_database_size(current_database()),'pending',(select count(*) from channel_topic_jobs where due_at is not null),
 'completed',(select count(*) from channel_topic_jobs where due_at is null and analyzer_version=1),
 'oldestPendingMs',(select coalesce(max(extract(epoch from clock_timestamp()-m.created_at)*1000),0) from channel_topic_jobs j join messages m on m.id=j.message_id where j.due_at is not null),
 'database',(select json_build_object('commits',xact_commit,'rollbacks',xact_rollback,'reads',blks_read,'hits',blks_hit,'tempBytes',temp_bytes) from pg_stat_database where datname=current_database()))`;

async function sample(config, fixture, replicas, profile, rate, repetition) {
  const database = await fixture.create(); const tag = `capacity-${randomUUID()}`;
  const apis = [], chats = [], records = [], recovery = [], points = [], measurementErrors = [];
  const latest = [10,10,10,10,10]; let stub, monitor, reader, collecting = false, collector, stopReason = null;
  try {
    if (profile === 'background') stub = await modelStub();
    const ports = await Promise.all(Array.from({ length: replicas }, freePort));
    const envs = ports.map((port, i) => applicationEnv(database, config.port, port, i, replicas, stub?.url));
    await run(config.binary, ['migrate'], { env: envs[0], timeoutMs: 90000 });
    for (const env of envs) apis.push(await startApi(config.binary, env));
    monitor = await monitorApis(config.binary, apis);
    const owner = 'synthetic-capacity-owner';
    const control = await openChat(ports[0], owner); chats.push(control);
    const actor = uuid((await control.command('hello')).participant_id);
    const channels = [];
    for (let i = 0; i < 5; i++) {
      channels.push(uuid((await control.command('create_channel', { slug: `capacity-${i}`, name: `Synthetic ${i}`, kind: 'private', circle_id: null })).channel.id));
      for (let n = 0; n < 10; n++) await control.command('send_message', { channel_id: channels[i], body: 'Synthetic warm-up' });
    }
    if (stub) {
      const background = uuid((await control.command('create_channel', { slug: 'background', name: 'Synthetic background', kind: 'private', circle_id: null })).channel.id);
      for (let n = 0; n < 40; n++) await control.command('send_message', { channel_id: background, body: `Synthetic bounded topic source ${n}` });
      const response = await fetch(`http://127.0.0.1:${ports[0]}/api/v1/channels/${background}/topics?participant=${owner}`, {
        method: 'PUT', headers: { 'content-type': 'application/json', 'x-sproyt-account': actor }, body: JSON.stringify({ enabled: true }) });
      assert(response.ok, 'could not enable synthetic topic channel');
    }
    for (let i = 0; i < config.clients; i++) chats.push(await openChat(ports[i % replicas], owner));
    reader = await openChat(ports[0], owner); await reader.command('subscribe_channel', { channel_id: channels[0] });
    if (stub) {
      for (let i = 0; stub.stats().calls === 0 && i < 150; i++) await sleep(100);
      assert(stub.stats().calls > 0, 'configured worker did not reach the local stub');
    }
    const metrics = async () => Promise.all(ports.map(async port => parseMetrics(await (await fetch(`http://127.0.0.1:${port}/metrics`, { signal: AbortSignal.timeout(2500) })).text())));
    const before = await metrics();
    assert.equal(before.reduce((total, map) => total + [...map.values()].find(m => m.name === 'sproyt_pool_connections_max').value, 0), 4);
    const dbBefore = await fixture.json(database, statsSql); const cgBefore = await fixture.cgroup();
    const cpuBefore = process.cpuUsage(); const wallStart = Date.now(); const started = performance.now();
    const count = Math.max(config.messages, Math.ceil(rate * config.minSeconds)); const durationMs = count * 1000 / rate;
    const plan = trafficPlan(count, rate, profile); const pending = new Set();
    stub?.release(); collecting = true;
    collector = (async () => {
      while (collecting) {
        try {
          const [db, cg, maps] = await Promise.all([fixture.json(database, statsSql), fixture.cgroup(), metrics()]);
          const api = monitor.samples.at(-1);
          const point = { atMs: Date.now() - wallStart, db, cg, api, gauges: maps.map(map => [...map.values()].filter(m => /sproyt_pool_connections_|sproyt_chat_phase_in_flight/.test(m.name))) };
          points.push(point);
          if (api && (sum(api.processes, 'rss') > 1024 ** 3 || sum(api.processes, 'privateBytes') > 1024 ** 3)) stopReason = 'api_memory_budget';
          if (db.bytes > 512 * 1024 ** 2) stopReason = 'database_storage_budget';
          if (db.connections > 4 + replicas) stopReason = 'unexpected_database_connections';
          if (config.interrupted) stopReason = 'probe_interrupted';
          if (apis.some(p => p.exitCode !== null)) stopReason = 'api_exit';
          if (monitor.child.exitCode !== null || !api || Date.now() - api.at > 2500) stopReason = 'resource_sampler_failed';
        } catch (error) { measurementErrors.push(error.message); stopReason = 'observer_failed'; }
        await sleep(750);
      }
    })();
    const recovering = (async () => {
      let cursor = 10;
      for (let r = 0; r < config.recoveries; r++) {
        await waitUntil((r + .5) * durationMs / config.recoveries, started);
        if (performance.now() - started >= durationMs || stopReason) break;
        for (const event of reader.events) {
          const message = event.payload?.message;
          if (message?.channel_id === channels[0] && message.sequence === cursor + 1) cursor++;
        }
        const begin = performance.now(); const from = cursor; reader.close(); await sleep(20);
        const target = latest[0];
        try {
          reader = await openChat(ports[(r + 1) % replicas], owner);
          await reader.command('subscribe_channel', { channel_id: channels[0] });
          for (let page = 0; cursor < target && page < 100; page++) {
            const result = await reader.command('load_recent_messages', { channel_id: channels[0], after: cursor, limit: 100 });
            assert(result.messages.length > 0, 'acknowledged history missing');
            for (const message of result.messages) { assert.equal(message.sequence, cursor + 1, 'history sequence gap'); cursor++; }
          }
          assert(cursor >= target, 'bounded catch-up did not reach its target');
          recovery.push({ ms: performance.now() - begin, after: from, target, reached: cursor, replica: (r + 1) % replicas });
        } catch (error) { recovery.push({ ms: performance.now() - begin, error: error.message }); stopReason = 'recovery_failed'; break; }
      }
    })();
    const payload = event => ({ channel_id: channels[event.channel], body: (`Synthetic capacity ${tag} ${event.index} `).padEnd(512, 'x') });
    for (const event of plan) {
      if (!stopReason) await waitUntil(event.at, started);
      const replica = event.index % replicas; const begin = performance.now(); const base = { index: event.index, kind: event.kind, replica, lagMs: Math.max(0, begin - started - event.at) };
      if (stopReason || pending.size >= config.maxInFlight) { records[event.index] = { ...base, error: stopReason ?? 'client_inflight_limit' }; continue; }
      const promise = chats[1 + event.index % config.clients].command('send_message', payload(event), `${tag}-${event.index}`).then(result => {
        assert(result.message?.id, 'send acknowledgement has no message'); latest[event.channel] = Math.max(latest[event.channel], result.message.sequence);
        records[event.index] = { ...base, id: result.message.id, sequence: result.message.sequence, wireMs: performance.now() - begin, endToEndMs: performance.now() - started - event.at };
      }).catch(error => { records[event.index] = { ...base, error: error.message }; }).finally(() => pending.delete(promise));
      pending.add(promise);
    }
    await Promise.all(pending); await recovering;
    const elapsedMs = performance.now() - started; const after = await metrics();
    collecting = false; await collector;
    const dbAfter = await fixture.json(database, statsSql); const cgAfter = await fixture.cgroup();
    const rows = await fixture.json(database, `select coalesce(json_agg(json_build_object('r',r.request_id,'i',m.id,'s',m.sequence,'c',m.channel_id)),'[]'::json) from command_receipts r join messages m on m.id=r.message_id where r.request_id like '${tag}-%'`);
    const receipts = new Map(rows.map(row => [row.r, row]));
    for (const record of records.filter(r => !r.error)) {
      const durable = receipts.get(`${tag}-${record.index}`); assert(durable && durable.i === record.id && durable.s === record.sequence, 'acknowledged message is not durable');
    }
    const sequenceChecks = await fixture.json(database, `select json_agg(x) from (select channel_id,count(*)::int as count,count(distinct sequence)::int as unique_sequences,min(sequence)::int as first,max(sequence)::int as last from messages where channel_id in (${channels.map(c => `'${c}'`).join(',')}) group by channel_id) x`);
    for (const row of sequenceChecks) assert(row.first === 1 && row.count === row.unique_sequences && row.count === row.last, 'channel sequence is not unique and contiguous');
    const accepted = records.filter(r => !r.error);
    for (const record of accepted.filter((_, i) => i % Math.max(1, Math.floor(accepted.length / 20)) === 0).slice(0, 20)) {
      const replay = await chats[1 + (record.index + 1) % config.clients].command('send_message', payload(plan[record.index]), `${tag}-${record.index}`);
      assert.equal(replay.message.id, record.id, 'replay created a different logical message');
    }
    assert.equal(Number(await fixture.pg(database, `select count(*) from command_receipts where request_id like '${tag}-%'`)), rows.length);
    if (stub) { assert(dbAfter.completed > dbBefore.completed, 'background worker made no durable progress'); assert.equal(stub.stats().peak, 1, 'unexpected concurrent model consumers'); }
    const apiSamples = monitor.samples.filter(p => p.at >= wallStart); assert(apiSamples.length >= 2, 'no API resource evidence');
    const first = apiSamples[0], last = apiSamples.at(-1); const cpuMs = sum(last.processes, 'cpuMs') - sum(first.processes, 'cpuMs');
    const generatorCpu = process.cpuUsage(cpuBefore);
    const sends = summarizeSends(records, count); const recoveryStats = quantiles(recovery.filter(r => !r.error).map(r => r.ms));
    const recoverySummary = { planned: config.recoveries, attempted: recovery.length, failures: recovery.filter(r => r.error), latencyMs: recoveryStats };
    const within = withinObjectives({ sends, recovery: recoverySummary, stopReason });
    return { replicas, profile, rate, repetition, count, elapsedMs, configuredPoolTotal: 4, listenerCount: replicas, activeTopicWorkers: stub ? 1 : 0,
      cpuAffinity: monitor.ready, memoryBudget: { bytes: 1024 ** 3, enforcement: 'combined RSS/private-byte stop threshold, not an OS hard limit' },
      sends, recovery: recoverySummary,
      acceptedPerSecond: sends.accepted * 1000 / elapsedMs, unknownSendOutcomes: rows.length - accepted.length,
      allAcknowledgementsDurable: true, idempotentReplayVerified: true, uniqueContiguousSequences: true,
      api: { cpuPercentOfOneLogicalCpu: cpuMs * 100 / (last.at - first.at), peakRssBytes: Math.max(...apiSamples.map(p => sum(p.processes, 'rss'))), peakPrivateBytes: Math.max(...apiSamples.map(p => sum(p.processes, 'privateBytes'))) },
      generatorCpuMs: (generatorCpu.user + generatorCpu.system) / 1000,
      postgres: { before: dbBefore, after: dbAfter, cpuPercentOfOneLogicalCpu: (cgAfter.usage_usec - cgBefore.usage_usec) / (elapsedMs * 10), throttledUsec: cgAfter.throttled_usec - cgBefore.throttled_usec, peakMemoryBytes: Math.max(...points.map(p => p.cg.memoryBytes)), peakConnections: Math.max(...points.map(p => p.db.connections)) },
      worker: stub?.stats() ?? null, histograms: histogramDelta(before, after), resourcePoints: points, measurementErrors, stopReason,
      verdict: within ? 'within_probe_objectives' : 'saturation_or_limit', productionCapacityClaim: false };
  } finally {
    collecting = false; if (collector) await collector.catch(() => {}); stub?.release(); reader?.close(); for (const chat of chats) chat.close();
    monitor?.close(); for (const api of apis) if (api.exitCode === null) api.kill();
    await Promise.all(apis.filter(api => api.exitCode === null).map(api => once(api, 'exit').catch(() => {})));
    stub?.close(); await fixture.drop(database);
  }
}

export async function main(args) {
  assert.equal(process.platform, 'win32', 'native API resource/affinity sampling currently requires Windows PowerShell 7');
  const config = options(args); const fixture = new Fixture(config.fixture, config.port); const metadata = await fixture.inspect();
  const evidence = { schema: 'sproyt.capacity-probe.v1', at: new Date().toISOString(), source: await run('git', ['rev-parse', 'HEAD']),
    build: 'caller-supplied binary; hash identifies it, record build profile in assessment',
    binarySha256: createHash('sha256').update(await readFile(config.binary)).digest('hex'), platform: process.platform, architecture: process.arch,
    toolSha256: Object.fromEntries(await Promise.all(['capacity-probe.mjs', 'capacity-runtime.mjs', 'capacity-core.mjs', 'capacity-sample.ps1'].map(async file => [file, createHash('sha256').update(await readFile(resolve('tools', file))).digest('hex')]))),
    fixture: metadata, input: { ...config, binary: undefined, output: undefined }, samples: [], errors: [] };
  await mkdir(resolve('.local'), { recursive: true });
  const save = () => writeFile(config.output, JSON.stringify(evidence, null, 2) + '\n');
  const execute = async (replicas, profile, rate, repetition) => {
    assert(!config.interrupted, 'probe interrupted');
    const result = await sample(config, fixture, replicas, profile, rate, repetition); evidence.samples.push(result); await save();
    console.log(JSON.stringify({ replicas, profile, rate, repetition, verdict: result.verdict, accepted: result.sends.accepted, failed: result.sends.failed, p99Ms: result.sends.scheduledToAckMs.p99, recoveryP99Ms: result.recovery.latencyMs.p99 })); return result;
  };
  const interrupt = () => { config.interrupted = true; };
  process.on('SIGINT', interrupt); process.on('SIGTERM', interrupt);
  try {
    for (const replicas of config.replicas) for (const profile of config.profiles) for (let repetition = 1; repetition <= config.repetitions; repetition++) await execute(replicas, profile, config.rate, repetition);
    if (config.ramp) for (const replicas of config.replicas) {
      for (const rate of [160, 320, 640].filter(rate => rate > config.rate)) {
        const first = await execute(replicas, 'hot', rate, 1);
        if (first.verdict !== 'within_probe_objectives' || rate === 640) { for (let n = 2; n <= 3; n++) await execute(replicas, 'hot', rate, n); break; }
      }
    }
  } catch (error) { evidence.errors.push(error.message); await save(); throw error; }
  finally { process.off('SIGINT', interrupt); process.off('SIGTERM', interrupt); }
  await save(); console.log(`Evidence: ${config.output}`);
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });