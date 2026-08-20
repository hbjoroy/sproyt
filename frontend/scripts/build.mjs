import { build } from "esbuild";
import { fileURLToPath } from "node:url";

const frontendRoot = fileURLToPath(new URL("..", import.meta.url));
const outputDirectory = process.env.SPROYT_FRONTEND_OUT_DIR ?? `${frontendRoot}/dist`;

await build({
  bundle: true,
  entryPoints: {
    app: `${frontendRoot}/src/app.ts`,
    "client-store": `${frontendRoot}/src/client-store.ts`
  },
  format: "esm",
  outdir: outputDirectory,
  platform: "browser",
  sourcemap: false,
  target: "es2022"
});
