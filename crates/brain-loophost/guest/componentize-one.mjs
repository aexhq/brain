// Componentize one admitted loop source bundle with the pinned toolchain. Invoked by the
// loop-host registry at custom-loop admission: argv = source path, wit path, output path.
import { componentize } from "@bytecodealliance/componentize-js";
import { readFile, writeFile } from "node:fs/promises";

const [, , sourcePath, witPath, outPath] = process.argv;
if (!sourcePath || !witPath || !outPath) {
  console.error("usage: componentize-one.mjs <source> <wit> <out>");
  process.exit(2);
}
const source = await readFile(sourcePath, "utf8");
const wit = await readFile(witPath, "utf8");
// No ambient network in the loop host: http features disabled.
const { component } = await componentize(source, wit, {
  worldName: "guest",
  disableFeatures: ["http", "fetch-event"],
});
await writeFile(outPath, component);
