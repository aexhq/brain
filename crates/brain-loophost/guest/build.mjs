// Build the guest loops into wasm components. No ambient network: http features disabled.
import { componentize } from "@bytecodealliance/componentize-js";
import { build } from "esbuild";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const wit = await readFile("../wit/guest.wit", "utf8");
await mkdir("dist", { recursive: true });

// The SDK-authored guest resolves the SDK from its in-repo TypeScript source — the same
// canonical bundling input `buildLoopBundle` pins for uploaded loops — and gets the host
// binding injected around it, exactly like a customer bundle.
const sdkEntry = fileURLToPath(
  new URL("../../../packages/agentloop/src/index.ts", import.meta.url),
);

for (const guest of [
  { entry: "loop-aex.mjs", out: "dist/aex-loop.component.wasm" },
  { entry: "loop-contract.mjs", out: "dist/contract-loop.component.wasm" },
  { entry: "loop-sdk.mjs", out: "dist/sdk-loop.component.wasm", sdk: true },
]) {
  const common = {
    bundle: true,
    format: "esm",
    platform: "neutral",
    external: ["loophost:abi/host"],
    write: false,
  };
  const bundled = await build(
    guest.sdk
      ? {
          ...common,
          stdin: {
            contents: [
              'import { call } from "loophost:abi/host";',
              'import { __bindHostCall } from "@aexhq/agentloop";',
              "__bindHostCall(call);",
              `export { activate } from "./${guest.entry}";`,
              "",
            ].join("\n"),
            resolveDir: process.cwd(),
            loader: "js",
            sourcefile: "agentloop-entry.js",
          },
          alias: { "@aexhq/agentloop": sdkEntry },
        }
      : { ...common, entryPoints: [guest.entry] },
  );
  const source = bundled.outputFiles[0].text;
  if (guest.sdk) {
    // The pre-componentize source bundle is what a customer uploads; the e2e reuses it.
    await writeFile(guest.out.replace(/\.component\.wasm$/, ".source.mjs"), source);
  }

  const { component } = await componentize(source, wit, {
    worldName: "guest",
    disableFeatures: ["http", "fetch-event"],
  });
  await writeFile(guest.out, component);
  console.log(`wrote ${guest.out} (${(component.length / 1024 / 1024).toFixed(2)} MiB)`);
}
