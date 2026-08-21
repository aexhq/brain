// Build the guest loops into wasm components. No ambient network: http features disabled.
import { componentize } from "@bytecodealliance/componentize-js";
import { build } from "esbuild";
import { mkdir, readFile, writeFile } from "node:fs/promises";

const wit = await readFile("../wit/guest.wit", "utf8");
await mkdir("dist", { recursive: true });

for (const guest of [
  { entry: "loop-aex.mjs", out: "dist/aex-loop.component.wasm" },
  { entry: "loop-contract.mjs", out: "dist/contract-loop.component.wasm" },
]) {
  const bundled = await build({
    entryPoints: [guest.entry],
    bundle: true,
    format: "esm",
    platform: "neutral",
    external: ["loophost:abi/host"],
    write: false,
  });
  const source = bundled.outputFiles[0].text;
  const { component } = await componentize(source, wit, {
    worldName: "guest",
    disableFeatures: ["http", "fetch-event"],
  });
  await writeFile(guest.out, component);
  console.log(`wrote ${guest.out} (${(component.length / 1024 / 1024).toFixed(2)} MiB)`);
}
