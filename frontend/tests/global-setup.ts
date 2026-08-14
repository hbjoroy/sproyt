import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { rm } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const repositoryRoot = fileURLToPath(new URL("../..", import.meta.url));
const runDirectory = process.env.SPROYT_E2E_RUN_DIRECTORY;
const port = Number(process.env.SPROYT_E2E_PORT);
const databaseUrl = process.env.SPROYT_E2E_DATABASE_URL;

function requireEnvironment(): { runDirectory: string; port: number; databaseUrl: string } {
  if (!runDirectory || !databaseUrl || !Number.isInteger(port)) {
    throw new Error("the e2e runner must provide an isolated database, directory, and port");
  }
  return { runDirectory, port, databaseUrl };
}

function cargoCommand(): string {
  return process.platform === "win32" ? "cargo.exe" : "cargo";
}

function commandEnvironment(port: number, databaseUrl: string): NodeJS.ProcessEnv {
  return {
    ...process.env,
    SPROYT_ADDR: `127.0.0.1:${port}`,
    SPROYT_AUTH_MODE: "development",
    SPROYT_ENV: "test",
    SPROYT_LOG_FORMAT: "pretty",
    SPROYT_FRONTEND_PREBUILT: "1",
    DATABASE_URL: databaseUrl
  };
}

async function runMigration(environment: NodeJS.ProcessEnv): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const child = spawn(cargoCommand(), ["run", "--quiet", "--", "migrate"], {
      cwd: repositoryRoot,
      env: environment,
      stdio: "inherit"
    });
    child.once("error", reject);
    child.once("exit", (code) => code === 0
      ? resolve()
      : reject(new Error(`database migration for Playwright exited with ${code ?? "no status"}`)));
  });
}

async function waitForReady(port: number, child: ChildProcess): Promise<void> {
  const deadline = Date.now() + 120_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`Sproyt test server exited with ${child.exitCode}`);
    try {
      const response = await fetch(`http://127.0.0.1:${port}/readyz`);
      if (response.ok) return;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("Sproyt test server did not become ready within 120 seconds");
}

function stopProcessTree(child: ChildProcess): void {
  if (child.exitCode !== null || child.pid === undefined) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/pid", String(child.pid), "/t", "/f"], { stdio: "ignore" });
    return;
  }
  try {
    process.kill(-child.pid, "SIGTERM");
  } catch {
    child.kill("SIGTERM");
  }
}

async function waitForExit(child: ChildProcess): Promise<boolean> {
  if (child.exitCode !== null) return true;
  return Promise.race([
    new Promise<boolean>((resolve) => child.once("exit", () => resolve(true))),
    new Promise<boolean>((resolve) => setTimeout(() => resolve(false), 10_000))
  ]);
}

async function stopAndWait(child: ChildProcess): Promise<void> {
  stopProcessTree(child);
  if (await waitForExit(child) || process.platform === "win32" || child.pid === undefined) return;
  try {
    process.kill(-child.pid, "SIGKILL");
  } catch {
    child.kill("SIGKILL");
  }
  await waitForExit(child);
}

export default async function globalSetup(): Promise<() => Promise<void>> {
  const isolated = requireEnvironment();
  const environment = commandEnvironment(isolated.port, isolated.databaseUrl);
  await runMigration(environment);
  const child = spawn(cargoCommand(), ["run", "--quiet"], {
    cwd: repositoryRoot,
    env: environment,
    stdio: "inherit",
    detached: process.platform !== "win32"
  });
  try {
    await waitForReady(isolated.port, child);
  } catch (error) {
    await stopAndWait(child);
    await rm(isolated.runDirectory, { recursive: true, force: true });
    throw error;
  }
  return async () => {
    await stopAndWait(child);
    await rm(isolated.runDirectory, { recursive: true, force: true });
  };
}
