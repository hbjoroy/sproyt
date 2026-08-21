import { build } from "esbuild";
import { copyFile, mkdir, mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

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
  await build({
    bundle: true,
    entryPoints: [fileURLToPath(new URL("../tests/boundaries.test.ts", import.meta.url))],
    format: "esm",
    outfile: outputFile,
    platform: "node",
    target: "node24"
  });
  await import(pathToFileURL(outputFile).href);
} finally {
  await rm(outputDirectory, { recursive: true, force: true });
}
