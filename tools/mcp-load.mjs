#!/usr/bin/env node
import { performance } from "node:perf_hooks";
import { randomUUID } from "node:crypto";

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

function integerArgument(name, fallback, minimum, maximum) {
  const index = process.argv.indexOf(name);
  const raw = index === -1 ? fallback : process.argv[index + 1];
  const value = Number(raw);
  if (!Number.isInteger(value) || value < minimum || value > maximum) {
    fail(`${name} must be an integer from ${minimum} through ${maximum}`);
  }
  return value;
}

const url = process.env.SPROYT_MCP_URL;
const credential = process.env.SPROYT_AGENT_CREDENTIAL;
const channelId = process.env.SPROYT_CHANNEL_ID;
if (!url || !credential || !channelId) {
  fail("set SPROYT_MCP_URL, SPROYT_AGENT_CREDENTIAL and SPROYT_CHANNEL_ID");
}
if (!process.argv.includes("--confirm-write")) {
  fail("refusing to create load-test messages without --confirm-write");
}

const messages = integerArgument("--messages", "40", 2, 10000);
const concurrency = integerArgument("--concurrency", "4", 1, 100);
const latencyObjectiveMs = integerArgument("--p99-ms", "750", 1, 60000);
const runId = randomUUID();
let rpcId = 0;

async function callTool(name, args) {
  const response = await fetch(url, {
    method: "POST",
    headers: {
      accept: "application/json, text/event-stream",
      authorization: `Bearer ${credential}`,
      "content-type": "application/json",
      "mcp-protocol-version": "2025-11-25",
    },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: `${runId}-${++rpcId}`,
      method: "tools/call",
      params: { name, arguments: args },
    }),
    signal: AbortSignal.timeout(30000),
  });
  if (!response.ok) {
    throw new Error(`MCP returned HTTP ${response.status}`);
  }
  const envelope = await response.json();
  if (envelope.error) {
    throw new Error(`MCP tool failed with code ${envelope.error.code}`);
  }
  return envelope.result;
}

const granted = await callTool("list_channels", {});
if (!Array.isArray(granted) || !granted.some((channel) => channel.id === channelId)) {
  fail("agent is not granted access to SPROYT_CHANNEL_ID");
}

const jobs = Array.from({ length: messages }, (_, index) => ({
  index,
  requestId: `${runId}-${index + 1}`,
}));
const results = new Array(messages);
let cursor = 0;

async function worker() {
  while (cursor < jobs.length) {
    const job = jobs[cursor++];
    const started = performance.now();
    const result = await callTool("send_message", {
      channel_id: channelId,
      body: `Sproyt capacity probe ${runId} ${job.index + 1}`,
      request_id: job.requestId,
      provenance: "generated",
    });
    results[job.index] = {
      latencyMs: performance.now() - started,
      id: result?.message?.id,
      sequence: result?.message?.sequence,
      requestId: job.requestId,
    };
  }
}

try {
  await Promise.all(Array.from({ length: Math.min(concurrency, messages) }, worker));
} catch (error) {
  fail(error instanceof Error ? error.message : "load request failed");
}

const invalid = results.some(
  (result) => !result?.id || !Number.isSafeInteger(result.sequence) || result.sequence < 1,
);
if (invalid) fail("MCP returned an invalid message identity or sequence");

const ids = new Set(results.map((result) => result.id));
if (ids.size !== messages) fail("load run returned duplicate message identities");

const sequences = results.map((result) => result.sequence).sort((a, b) => a - b);
if (new Set(sequences).size !== messages) fail("load run returned duplicate sequences");
if (sequences.at(-1) - sequences[0] + 1 !== messages) {
  fail("load channel contained interleaved writes; use a dedicated channel for release evidence");
}

const first = results[0];
const replay = await callTool("send_message", {
  channel_id: channelId,
  body: `Sproyt capacity probe ${runId} 1`,
  request_id: first.requestId,
  provenance: "generated",
});
if (replay?.message?.id !== first.id || replay?.message?.sequence !== first.sequence) {
  fail("idempotent replay did not return the original message");
}

const latencies = results.map((result) => result.latencyMs).sort((a, b) => a - b);
const percentile = (value) => latencies[Math.max(0, Math.ceil(messages * value) - 1)];
const p50 = percentile(0.5);
const p99 = percentile(0.99);
if (p99 >= latencyObjectiveMs) {
  fail(`p99 send latency ${p99.toFixed(1)} ms exceeded ${latencyObjectiveMs} ms`);
}

process.stdout.write(`${JSON.stringify({
  schema: "sproyt.mcp-load-evidence.v1",
  runId,
  endpoint: new URL(url).origin,
  channelId,
  messages,
  concurrency,
  firstSequence: sequences[0],
  lastSequence: sequences.at(-1),
  p50Ms: Number(p50.toFixed(1)),
  p99Ms: Number(p99.toFixed(1)),
  objectiveMs: latencyObjectiveMs,
  idempotentReplay: true,
}, null, 2)}\n`);
