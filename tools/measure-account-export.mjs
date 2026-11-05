// Run from the repository root with Node and PowerShell 7 on Windows.
// Use a disposable wslc PostgreSQL 17 fixture listening on 127.0.0.1:55431,
// initialized with the synthetic sproyt/export-fixture credentials. Each
// sample drops its own temporary database and removes its local JSON sink.
// Usage: node tools/measure-account-export.mjs path/to/sproyt.exe [container]
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createWriteStream } from "node:fs";
import { mkdir, readFile, unlink } from "node:fs/promises";
import { createServer } from "node:net";
import { resolve } from "node:path";
import { createInterface } from "node:readline";
import { setTimeout as sleep } from "node:timers/promises";

const [binaryPath, fixture = "sproyt-351-export-pg"] = process.argv.slice(2);
assert(binaryPath, "Usage: node tools/measure-account-export.mjs path/to/sproyt.exe [container]");
assert.match(fixture, /^[a-zA-Z0-9][a-zA-Z0-9_.-]*$/);
const binary = resolve(binaryPath);
const messageBytes = 2048;
const sizes = [4000, 40000];

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", windowsHide: true, ...options });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${command} failed: ${result.stderr}`);
  return result.stdout.trim();
}

function pg(args) {
  return run("wslc", ["exec", fixture, ...args]);
}

async function freePort() {
  const listener = createServer();
  listener.listen(0, "127.0.0.1");
  await once(listener, "listening");
  const { port } = listener.address();
  listener.close();
  await once(listener, "close");
  return port;
}

async function startChat(url) {
  const socket = new WebSocket(url);
  const requests = new Map();
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("WebSocket open timed out")), 5000);
    socket.addEventListener("open", () => { clearTimeout(timeout); resolve(); }, { once: true });
    socket.addEventListener("error", () => { clearTimeout(timeout); reject(new Error("WebSocket failed")); }, { once: true });
  });
  socket.addEventListener("message", ({ data }) => {
    const event = JSON.parse(data);
    requests.get(event.request_id)?.(event);
  });
  let next = 0;
  const command = (type, payload) => new Promise((resolve, reject) => {
    const id = `bench-${++next}`;
    const timeout = setTimeout(() => {
      requests.delete(id);
      reject(new Error(`WebSocket ${type} timed out`));
    }, 5000);
    requests.set(id, event => {
      clearTimeout(timeout);
      requests.delete(id);
      if (event.type === "error") reject(new Error(JSON.stringify(event.payload)));
      else resolve(event.payload);
    });
    socket.send(JSON.stringify({ protocol: "sproyt.chat.v1", request_id: id, type, payload }));
  });
  return { command, close: () => socket.close() };
}

function startSampling(pid) {
  const samples = [];
  const process = spawn("pwsh.exe", ["-NoProfile", "-Command", `while ($p = Get-Process -Id ${pid} -ErrorAction SilentlyContinue) { Write-Output "$($p.WorkingSet64),$($p.PrivateMemorySize64)"; Start-Sleep -Milliseconds 25 }`],
    { windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
  const reader = createInterface({ input: process.stdout });
  reader.on("line", line => {
    const [workingSet, privateBytes] = line.split(",").map(Number);
    if (workingSet > 0 && privateBytes > 0) samples.push({ workingSet, privateBytes });
  });
  return { samples, close: () => { reader.close(); process.kill(); } };
}

const percentile = (values, p) => [...values].sort((a, b) => a - b)[Math.ceil(values.length * p) - 1];
const round = value => Number(value.toFixed(2));
const mib = value => round(value / 1024 ** 2);

async function measure(rows) {
  const database = `sproyt_export_bench_${rows}_${Date.now()}`;
  const target = resolve(`.local/${database}.json`);
  pg(["createdb", "-U", "sproyt", database]);
  let server, chat, sampler, writing;
  try {
    const port = await freePort();
    const origin = `http://127.0.0.1:${port}`;
    const env = { ...process.env, DATABASE_URL: `postgres://sproyt:export-fixture@127.0.0.1:55431/${database}`,
      SPROYT_ADDR: `127.0.0.1:${port}`, SPROYT_ENV: "test", SPROYT_AUTH_MODE: "development",
      SPROYT_DB_MAX_CONNECTIONS: "4", SPROYT_LOG_FORMAT: "pretty" };
    run(binary, ["migrate"], { env });
    server = spawn(binary, [], { env, windowsHide: true, stdio: "ignore" });
    let ready = false;
    for (let attempt = 0; attempt < 100 && server.exitCode === null; attempt++) {
      try { if ((await fetch(`${origin}/readyz`, { signal: AbortSignal.timeout(1000) })).ok) { ready = true; break; } }
      catch { /* Wait for the local server bind. */ }
      await sleep(100);
    }
    assert(ready, "synthetic app never became ready");
    chat = await startChat(`ws://127.0.0.1:${port}/ws?participant=export-bench`);
    const data = (await chat.command("create_channel", { slug: "export-data", name: "Synthetic export data", kind: "private" })).channel.id;
    const live = (await chat.command("create_channel", { slug: "export-live", name: "Synthetic concurrent chat", kind: "private" })).channel.id;
    assert.match(data, /^[a-f0-9-]{36}$/);
    pg(["psql", "-X", "-U", "sproyt", "-d", database, "-v", "ON_ERROR_STOP=1", "-q", "-c",
      `INSERT INTO messages(id,channel_id,sender_id,sender_display_name,sequence,body) SELECT gen_random_uuid(),c.id,c.created_by,'Export fixture',seq,repeat('x',${messageBytes}) FROM channels c CROSS JOIN generate_series(1,${rows}) seq WHERE c.id='${data}'; UPDATE channel_sequences SET next_sequence=${rows + 1} WHERE channel_id='${data}';`]);
    await chat.command("send_message", { channel_id: live, body: "warmup" });
    sampler = startSampling(server.pid);
    for (let attempt = 0; sampler.samples.length < 10 && attempt < 100; attempt++) await sleep(50);
    assert(sampler.samples.length >= 10, "no working-set baseline");
    const baseline = sampler.samples.length;
    const start = performance.now();
    const response = await fetch(`${origin}/api/v1/me/export?participant=export-bench`, { signal: AbortSignal.timeout(100_000) });
    assert.equal(response.status, 200);
    assert.equal(response.headers.get("cache-control"), "no-store");
    const firstResponseMs = performance.now() - start;
    const [exportConnections, exportLocks] = pg(["psql", "-X", "-U", "sproyt", "-d", database, "-Atc",
      "select count(distinct pid),count(*) from pg_locks where locktype='advisory' and granted and database=(select oid from pg_database where datname=current_database())"])
      .split("|").map(Number);
    assert.equal(exportConnections, 1, "one paused export must hold one database connection");
    assert.equal(exportLocks, 2, "the export must hold one global slot and one actor lock");
    const latencies = [];
    let active = true;
    const chatLoop = (async () => {
      while (active) {
        const sent = performance.now();
        await chat.command("send_message", { channel_id: live, body: `concurrent ${latencies.length + 1}` });
        latencies.push(performance.now() - sent);
        await sleep(40);
      }
    })();
    let bytes = 0;
    let pausedAtBytes = 0;
    let elapsedMs;
    writing = createWriteStream(target);
    try {
      for await (const chunk of response.body) {
        bytes += chunk.byteLength;
        if (!writing.write(chunk)) await once(writing, "drain");
        if (bytes - pausedAtBytes >= 256 * 1024) {
          pausedAtBytes = bytes;
          await sleep(12);
        }
      }
      writing.end();
      await once(writing, "finish");
      elapsedMs = performance.now() - start;
    } finally {
      active = false;
      await chatLoop;
    }
    sampler.close();
    const samples = sampler.samples.slice(baseline);
    assert(samples.length >= 10, "missing export memory samples");
    assert(latencies.length >= 4, "insufficient concurrent chat samples");
    const memory = {};
    for (const metric of ["workingSet", "privateBytes"]) {
      const idle = percentile(sampler.samples.slice(0, baseline).map(s => s[metric]), 0.5);
      const peak = Math.max(...samples.map(s => s[metric]));
      memory[metric] = { idleMiB: mib(idle), peakMiB: mib(peak), deltaMiB: mib(peak - idle) };
    }

    const json = JSON.parse(await readFile(target, "utf8"));
    assert.equal(json.format, "sproyt.user-export.v1");
    const exported = json.channels.find(entry => entry.channel.id === data);
    assert.equal(exported.messages.length, rows);
    exported.messages.forEach((message, index) => {
      assert.equal(message.sequence, index + 1);
      assert.equal(message.body.length, messageBytes);
    });
    const snapshotLive = json.channels.find(entry => entry.channel.id === live);
    assert.deepEqual(snapshotLive.messages.map(message => message.body), ["warmup"]);
    return { rows, bytes, firstResponseMs: round(firstResponseMs), elapsedMs: round(elapsedMs),
      samples: samples.length, exportConnections, exportLocks, memory, chat: { count: latencies.length, p50Ms: round(percentile(latencies, 0.5)),
        p95Ms: round(percentile(latencies, 0.95)), maxMs: round(Math.max(...latencies)) }, completeJsonAndSnapshot: true };
  } finally {
    sampler?.close();
    writing?.destroy();
    chat?.close();
    if (server && server.exitCode === null && server.signalCode === null) {
      server.kill();
      await once(server, "exit");
    }
    await unlink(target).catch(error => { if (error.code !== "ENOENT") throw error; });
    pg(["dropdb", "-U", "sproyt", database]);
  }
}

await mkdir(".local", { recursive: true });
for (const rows of sizes) console.log(JSON.stringify(await measure(rows)));
