// Build the loop-host's own guest artifacts: the engine-vocabulary bootstrap `aex` loop and
// the raw-ABI test fixtures. Official loops live outside Brain and use the same public builder.
// No ambient network: http features disabled.
import { componentize } from "@bytecodealliance/componentize-js";
import { build } from "esbuild";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

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

// Raw-ABI guests: the bootstrap aex loop and the contract-vocabulary probe fixture.
// The official aex loop builds through the PUBLIC builder like every other loop — it is an
// ordinary extension; only its default-selection status is special.
{
  const { buildLoopBundle } = await import(
    new URL("../../../packages/agentloop/dist/build.js", import.meta.url)
  );
  const aex = await buildLoopBundle({
    entry: fileURLToPath(new URL("./loop-aex.mjs", import.meta.url)),
  });
  await componentizeSource(aex.source, "dist/aex-loop.component.wasm");
}
await componentizeSource(
  await bundleRaw("loop-contract.mjs"),
  "dist/contract-loop.component.wasm",
);

// The rogue fixture is upload-only (the e2e proves the engine-op gate refuses it), so only
// its source bundle is emitted; the composition componentizes it at admission like any
// customer bundle.
await writeFile("dist/rogue-loop.source.mjs", await bundleRaw("loop-rogue.mjs"));

// The SDK fixture builds through the PUBLIC builder — the exact path a customer runs — so
// the upload e2e componentizes-and-drives a real `buildLoopBundle` artifact, and the
// in-process SDK test runs the same source prebuilt.
const { buildLoopBundle } = await import(
  new URL("../../../packages/agentloop/dist/build.js", import.meta.url)
);
const sdk = await buildLoopBundle({
  entry: fileURLToPath(new URL("./loop-sdk.mjs", import.meta.url)),
});
await writeFile("dist/sdk-loop.source.mjs", sdk.source);
await componentizeSource(sdk.source, "dist/sdk-loop.component.wasm");
