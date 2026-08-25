// Build the loop-host's neutral raw-ABI conformance fixtures. Official loop extensions live
// outside Brain and arrive through the same custom-bundle admission path as any other loop.
import { componentize } from "@bytecodealliance/componentize-js";
import { build } from "esbuild";
import { mkdir, readFile, writeFile } from "node:fs/promises";

const wit = await readFile("../wit/guest.wit", "utf8");
await mkdir("dist", { recursive: true });

const componentizeSource = async (source, out) => {
  const { component } = await componentize(source, wit, {
    worldName: "guest",
    disableFeatures: ["http", "fetch-event"],
  });
  await writeFile(out, component);
  console.log(`wrote ${out} (${(component.length / 1024 / 1024).toFixed(2)} MiB)`);
};

const bundleRaw = async (entry) => {
  const bundled = await build({
    entryPoints: [entry],
    bundle: true,
    format: "esm",
    platform: "neutral",
    external: ["loophost:abi/host"],
    write: false,
  });
  return bundled.outputFiles[0].text;
};

const contract = await bundleRaw("loop-contract.mjs");
await writeFile("dist/contract-loop.source.mjs", contract);
await componentizeSource(contract, "dist/contract-loop.component.wasm");

// The rogue fixture is upload-only (the e2e proves the engine-op gate refuses it), so only
// its source bundle is emitted; the composition componentizes it at admission like any
// customer bundle.
await writeFile("dist/rogue-loop.source.mjs", await bundleRaw("loop-rogue.mjs"));
