import { build } from "esbuild";
import { componentize } from "@bytecodealliance/componentize-js";
import { writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const [entry = "loop.mjs", output = "loop.wasm"] = process.argv.slice(2);
const bundled = await build({
  entryPoints: [entry], bundle: true, format: "esm", platform: "neutral", write: false,
  external: ["brain:agentloop/host@0.2.0"],
});
const { component } = await componentize(bundled.outputFiles[0].text, {
  witPath: fileURLToPath(import.meta.resolve("@aexhq/brain/contracts/agentloop.wit")),
  worldName: "agentloop",
  disableFeatures: ["stdio", "random", "clocks", "http", "fetch-event"],
});
await writeFile(output, component);
