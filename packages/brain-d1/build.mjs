import { build } from "esbuild";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(fileURLToPath(import.meta.url));
await build({
  absWorkingDir: root,
  entryPoints: ["src/worker.mjs"],
  outfile: "dist/worker.mjs",
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2023",
  loader: { ".sql": "text" },
  sourcemap: true,
  logLevel: "warning",
});
