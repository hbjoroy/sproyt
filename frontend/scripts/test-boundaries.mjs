import { build } from "esbuild";
import { spawn } from "node:child_process";
import { copyFile, mkdir, mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const outputDirectory = await mkdtemp(path.join(os.tmpdir(), "sproyt-frontend-boundaries-"));
const outputFile = path.join(outputDirectory, "boundaries.mjs");

try {
  await mkdir(path.join(outputDirectory, "fixtures"));
  await copyFile(
    fileURLToPath(new URL("../tests/fixtures/rust-serde-joinable-channels-listed.json", import.meta.url)),
    path.join(outputDirectory, "fixtures", "rust-serde-joinable-channels-listed.json")
  );
  await copyFile(
    fileURLToPath(new URL("../tests/fixtures/rust-serde-client-commands.json", import.meta.url)),
    path.join(outputDirectory, "fixtures", "rust-serde-client-commands.json")
  );
  await copyFile(
    fileURLToPath(new URL("../tests/fixtures/durable-send-admission.json", import.meta.url)),
    path.join(outputDirectory, "fixtures", "durable-send-admission.json")
  );
  await build({
    bundle: true,
    entryPoints: [fileURLToPath(new URL("../tests/boundaries.test.ts", import.meta.url))],
    format: "esm",
    // React's server renderer uses Node builtins through CommonJS internally.
    banner: { js: 'import { createRequire } from "node:module"; const require = createRequire(import.meta.url);' },
    outfile: outputFile,
    platform: "node",
    target: "node24"
  });
  // Import completion does not mean async node:test cases have finished.
  // Keep their temporary fixtures alive until the test process has exited.
  const code = await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, ["--test", outputFile], { stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", resolve);
  });
  if (code !== 0) process.exitCode = 1;
} finally {
  await rm(outputDirectory, { recursive: true, force: true });
}
