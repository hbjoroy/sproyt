import { spawn } from "node:child_process";
import { mkdir, rm } from "node:fs/promises";
import net from "node:net";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";

const frontendRoot = fileURLToPath(new URL("..", import.meta.url));
const repositoryRoot = fileURLToPath(new URL("../..", import.meta.url));
const runDirectory = `${frontendRoot}/.playwright/run-${randomUUID()}`;

async function allocateFreePort() {
  const reservation = net.createServer();
  await new Promise((resolve, reject) => {
    reservation.once("error", reject);
    reservation.listen({ host: "127.0.0.1", port: 0 }, resolve);
  });
  const address = reservation.address();
  if (!address || typeof address === "string") throw new Error("could not reserve a local TCP port");
  await new Promise((resolve, reject) => reservation.close((error) => error ? reject(error) : resolve()));
  return address.port;
}

const port = await allocateFreePort();
await mkdir(runDirectory, { recursive: true });
const databaseUrl = `sqlite://frontend/.playwright/${runDirectory.split("/").at(-1)}/sproyt.sqlite`;
const cli = `${frontendRoot}/node_modules/@playwright/test/cli.js`;

try {
  const result = await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [cli, "test"], {
      cwd: frontendRoot,
      env: {
        ...process.env,
        SPROYT_E2E_PORT: String(port),
        SPROYT_E2E_RUN_DIRECTORY: runDirectory,
        SPROYT_E2E_DATABASE_URL: databaseUrl
      },
      stdio: "inherit"
    });
    child.once("error", reject);
    child.once("exit", (code) => resolve(code ?? 1));
  });
  process.exitCode = Number(result);
} finally {
  await rm(runDirectory, { recursive: true, force: true });
}
