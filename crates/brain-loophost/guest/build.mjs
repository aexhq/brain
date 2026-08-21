// Build the aex guest loop into a wasm component. No ambient network: http features disabled.
import { componentize } from "@bytecodealliance/componentize-js";
import { build } from "esbuild";
import { mkdir, readFile, writeFile } from "node:fs/promises";

const bundled = await build({
  entryPoints: ["loop-aex.mjs"],
  bundle: true,
  format: "esm",
  platform: "neutral",
  external: ["loophost:abi/host"],
  write: false,
});
const source = bundled.outputFiles[0].text;

const wit = await readFile("../wit/guest.wit", "utf8");
const { component } = await componentize(source, wit, {
  worldName: "guest",
  disableFeatures: ["http", "fetch-event"],
});
await mkdir("dist", { recursive: true });
await writeFile("dist/aex-loop.component.wasm", component);
console.log(`wrote dist/aex-loop.component.wasm (${(component.length / 1024 / 1024).toFixed(2)} MiB)`);
