import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createServer as netServer } from 'node:net';
import { createInterface } from 'node:readline';
import { resolve } from 'node:path';
import { setTimeout as sleep } from 'node:timers/promises';
import { performance } from 'node:perf_hooks';
import { randomUUID } from 'node:crypto';

export async function run(command, args, options = {}) {
  const child = spawn(command, args, { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], ...options });
  let stdout = '', stderr = '';
  child.stdout.on('data', chunk => { stdout += chunk; if (stdout.length > 4 * 1024 ** 2) child.kill(); });
  child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-4000); });
  const timer = setTimeout(() => child.kill(), options.timeoutMs ?? 30000);
  try {
    const [code] = await once(child, 'close');
    assert.equal(code, 0, `${command} failed: ${stderr.slice(-1200)}`);
    return stdout.trim();
  } finally { clearTimeout(timer); }
}

export async function freePort() {
  const server = netServer(); server.listen(0, '127.0.0.1'); await once(server, 'listening');
  const port = server.address().port; server.close(); await once(server, 'close'); return port;
}

export function fixtureMetadata(value, port) {
  assert(value.Config?.Labels?.['sproyt.capacity-fixture'] === 'true', 'container is not a labelled capacity fixture');
  assert(value.State?.Running, 'fixture is not running');
  const ports = (value.Ports ?? value.NetworkSettings?.Ports)?.['5432/tcp'];
  assert(ports?.length === 1 && ports[0].HostIp === '127.0.0.1' && Number(ports[0].HostPort) === port, 'fixture port must be loopback-only and match the runner');
  assert(!value.Mounts?.some(m => m.Type === 'bind'), 'fixture must not have host bind mounts');
  assert(value.HostConfig?.NanoCpus > 0 && value.HostConfig?.Memory > 0, 'fixture requires CPU and memory limits');
  return { image: value.Config.Image, imageId: value.Image, cpuLimit: value.HostConfig.NanoCpus / 1e9,
    memoryLimitBytes: value.HostConfig.Memory, hostPort: port };
}

export class Fixture {
  constructor(name, port) {
    assert(/^sproyt-353-capacity-[a-z0-9-]+$/.test(name));
    assert(Number.isInteger(port) && port >= 1024 && port < 65536);
    this.name = name; this.port = port;
  }
  async inspect() { return fixtureMetadata(JSON.parse(await run('wslc', ['inspect', this.name]))[0], this.port); }
  async pg(database, sql) {
    assert(database === 'postgres' || /^sproyt_capacity_[a-f0-9]{32}$/.test(database), 'only disposable capacity databases are accepted');
    return run('wslc', ['exec', this.name, 'env', 'PGAPPNAME=capacity_observer', 'psql', '-X', '-U', 'sproyt', '-d', database, '-v', 'ON_ERROR_STOP=1', '-Atq', '-c', sql]);
  }
  async json(database, sql) { return JSON.parse(await this.pg(database, sql)); }
  async create() {
    const name = `sproyt_capacity_${randomUUID().replaceAll('-', '')}`;
    await this.pg('postgres', `create database ${name}`); return name;
  }
  async drop(database) {
    assert(/^sproyt_capacity_[a-f0-9]{32}$/.test(database));
    await this.pg('postgres', `drop database ${database} with (force)`);
  }
  async cgroup() {
    const value = await run('wslc', ['exec', this.name, 'cat', '/sys/fs/cgroup/cpu.stat', '/sys/fs/cgroup/memory.current']);
    const rows = value.split('\n');
    const counters = Object.fromEntries(rows.map(l => /^(\w+) (\d+)$/.exec(l)).filter(Boolean).map(m => [m[1], Number(m[2])]));
    return { ...counters, memoryBytes: Number(rows.findLast(l => /^\d+$/.test(l.trim()))) };
  }
}

export function applicationEnv(database, pgPort, port, index, replicas, topicUrl) {
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/^(SPROYT_|HEART_|PG|DATABASE_URL$|RUST_LOG$)/i.test(key)));
  return { ...env, DATABASE_URL: `postgres://sproyt:capacity-fixture@127.0.0.1:${pgPort}/${database}?application_name=capacity_api_${index + 1}`,
    SPROYT_ADDR: `127.0.0.1:${port}`, SPROYT_ENV: 'test', SPROYT_AUTH_MODE: 'development', SPROYT_LOG_FORMAT: 'pretty', RUST_LOG: 'warn',
    SPROYT_DB_MAX_CONNECTIONS: String(4 / replicas), SPROYT_CHAT_AGENTS_ENABLED: 'false', SPROYT_PROCESS_PILOT_ENABLED: 'false',
    SPROYT_PROCESS_OUTBOX_ENABLED: 'false', SPROYT_MEDIA_CHUNK_UPLOADS_ENABLED: 'false',
    SPROYT_CHANNEL_TOPICS_WORKER_ENABLED: topicUrl && index === 0 ? 'true' : 'false',
    ...(topicUrl && index === 0 ? { SPROYT_CHANNEL_TOPICS_MODEL_URL: topicUrl, SPROYT_CHANNEL_TOPICS_MODEL: 'synthetic-capacity-stub' } : {}) };
}

export async function startApi(binary, env) {
  const child = spawn(binary, [], { env, windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] });
  let stderr = ''; child.stderr.on('data', c => { stderr = (stderr + c).slice(-4000); });
  for (let attempt = 0; attempt < 100; attempt++) {
    assert(child.exitCode === null, `fixture API exited before readiness: ${stderr}`);
    try { if ((await fetch(`http://${env.SPROYT_ADDR}/readyz`, { signal: AbortSignal.timeout(1000) })).ok) return child; } catch {}
    await sleep(100);
  }
  child.kill(); throw new Error('fixture API readiness timed out');
}

export async function monitorApis(binary, processes) {
  const child = spawn('pwsh.exe', ['-NoProfile', '-File', resolve('tools/capacity-sample.ps1'), '-ProcessIds', processes.map(p => p.pid).join(','), '-Binary', binary],
    { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
  const samples = []; let error = ''; child.stderr.on('data', c => { error = (error + c).slice(-2000); });
  const lines = createInterface({ input: child.stdout });
  const ready = new Promise((resolveReady, reject) => {
    const timer = setTimeout(() => reject(new Error('CPU affinity/sampler setup timed out')), 10000);
    child.once('exit', () => { clearTimeout(timer); reject(new Error(`sampler exited: ${error}`)); });
    lines.on('line', line => {
      try { const row = JSON.parse(line); if (row.type === 'ready') { clearTimeout(timer); resolveReady(row); } else samples.push(row); }
      catch { error = 'invalid sampler output'; }
    });
  });
  try { return { child, samples, ready: await ready, close() { lines.close(); child.kill(); } }; }
  catch (error) { lines.close(); child.kill(); throw error; }
}

export async function openChat(port, owner) {
  const socket = new WebSocket(`ws://127.0.0.1:${port}/ws?participant=${encodeURIComponent(owner)}`);
  const pending = new Map(); const events = [];
  socket.addEventListener('message', ({ data }) => {
    const event = JSON.parse(data); const request = pending.get(event.request_id);
    if (request) { pending.delete(event.request_id); clearTimeout(request.timer);
      if (event.type === 'error') request.reject(new Error(`protocol_${event.payload?.code ?? 'error'}`)); else request.resolve(event.payload); }
    else { events.push(event); if (events.length > 2000) events.shift(); }
  });
  socket.addEventListener('close', () => { for (const request of pending.values()) { clearTimeout(request.timer); request.reject(new Error('socket_closed')); } pending.clear(); });
  await new Promise((resolveOpen, reject) => {
    const timer = setTimeout(() => { socket.close(); reject(new Error('socket_open_timeout')); }, 5000);
    socket.addEventListener('open', () => { clearTimeout(timer); resolveOpen(); }, { once: true });
    socket.addEventListener('error', () => { clearTimeout(timer); reject(new Error('socket_open_failed')); }, { once: true });
  });
  const command = (type, payload, id = randomUUID()) => new Promise((resolveCommand, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error('command_timeout')); }, 10000);
    pending.set(id, { resolve: resolveCommand, reject, timer });
    socket.send(JSON.stringify({ protocol: 'sproyt.chat.v1', type, request_id: id, ...(payload === undefined ? {} : { payload }) }));
  });
  await command('hello');
  return { command, events, close() { socket.close(); } };
}

export async function waitUntil(at, origin) {
  const remaining = at - (performance.now() - origin);
  if (remaining > 0) await sleep(remaining);
}