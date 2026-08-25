import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const here = new URL("./", import.meta.url);
const root = new URL("../../../", here);
const { componentize } = await import(
  new URL(
    "../../brain-loophost/guest/node_modules/@bytecodealliance/componentize-js/src/componentize.js",
    here,
  )
);
const definitions = [
  { kind: "agentloop", source: "agentloop.mjs" },
  { kind: "tool", source: "tool.mjs" },
  { kind: "environment", source: "environment.mjs" },
  { kind: "model", source: "model.mjs" },
];

await mkdir(new URL("dist/", here), { recursive: true });
for (const definition of definitions) {
  const wit = await readFile(
    new URL(`contracts/${definition.kind}/v1/${definition.kind}.wit`, root),
    "utf8",
  );
  const source = await readFile(new URL(`fixtures/${definition.source}`, here), "utf8");
  const { component } = await componentize(source, wit, {
    worldName: definition.kind,
    disableFeatures: ["http", "fetch-event"],
  });
  const destination = new URL(`dist/${definition.kind}.component.wasm`, here);
  await writeFile(destination, component);
  console.log(`${definition.kind}: ${fileURLToPath(destination)} (${component.length} bytes)`);
}
