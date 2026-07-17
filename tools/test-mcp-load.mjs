import assert from "node:assert/strict";
import http from "node:http";
import { spawn } from "node:child_process";

const credential = "test-secret-that-must-not-appear";
const channelId = "01900000-0000-7000-8000-000000000001";
const messagesByRequest = new Map();
let nextSequence = 10;
let requestCount = 0;

const server = http.createServer(async (request, response) => {
  requestCount += 1;
  assert.equal(request.headers.authorization, `Bearer ${credential}`);
  assert.match(request.headers.accept, /application\/json/);
  assert.match(request.headers.accept, /text\/event-stream/);
  let body = "";
  for await (const chunk of request) body += chunk;
  const rpc = JSON.parse(body);
  const name = rpc.params.name;
  let result;
  if (name === "list_channels") {
    result = [{ id: channelId }];
  } else {
    assert.equal(name, "send_message");
    const args = rpc.params.arguments;
    let message = messagesByRequest.get(args.request_id);
    if (!message) {
      message = { id: `message-${nextSequence}`, sequence: nextSequence++ };
      messagesByRequest.set(args.request_id, message);
    }
    result = { message, provenance: { provenance: "generated" } };
  }
  response.writeHead(200, { "content-type": "application/json" });
  response.end(JSON.stringify({ jsonrpc: "2.0", id: rpc.id, result }));
});

await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const { port } = server.address();

function run(extraArguments = []) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, ["tools/mcp-load.mjs", ...extraArguments], {
      cwd: new URL("..", import.meta.url),
      env: {
        ...process.env,
        SPROYT_MCP_URL: `http://127.0.0.1:${port}/mcp`,
        SPROYT_AGENT_CREDENTIAL: credential,
        SPROYT_CHANNEL_ID: channelId,
      },
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

const refused = await run(["--messages", "4"]);
assert.equal(refused.code, 1);
assert.match(refused.stderr, /--confirm-write/);
assert.doesNotMatch(refused.stderr, new RegExp(credential));

const accepted = await run([
  "--confirm-write", "--messages", "4", "--concurrency", "2", "--p99-ms", "5000",
]);
assert.equal(accepted.code, 0, accepted.stderr);
assert.doesNotMatch(accepted.stdout + accepted.stderr, new RegExp(credential));
const evidence = JSON.parse(accepted.stdout);
assert.equal(evidence.schema, "sproyt.mcp-load-evidence.v1");
assert.equal(evidence.messages, 4);
assert.equal(evidence.idempotentReplay, true);
assert.equal(evidence.firstSequence, 10);
assert.equal(evidence.lastSequence, 13);
assert.equal(requestCount, 6);

await new Promise((resolve) => server.close(resolve));
console.log("MCP load evidence contract passed");
