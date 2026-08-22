// Componentize one admitted loop source bundle with the pinned toolchain. Invoked by the
// loop-host registry at custom-loop admission: argv = source path, wit path, output path,
// expected toolchain string.
import { componentize } from "@bytecodealliance/componentize-js";
import { readFile, writeFile } from "node:fs/promises";

const [, , sourcePath, witPath, outPath, expectedToolchain] = process.argv;
if (!sourcePath || !witPath || !outPath || !expectedToolchain) {
  console.error("usage: componentize-one.mjs <source> <wit> <out> <toolchain>");
  process.exit(2);
}

// The sealed toolchain string must name the componentizer actually installed here: a
// dependency bump without renaming the constant would otherwise seal identities as a lie.
// (The package's exports map hides its package.json, so read the manifest off disk.)
const installed = JSON.parse(
  await readFile(
    new URL("./node_modules/@bytecodealliance/componentize-js/package.json", import.meta.url),
    "utf8",
  ),
).version;
const actualToolchain = `starlingmonkey-componentize-js-${installed}`;
if (actualToolchain !== expectedToolchain) {
  console.error(
    `toolchain mismatch: this install componentizes as ${actualToolchain}, ` +
      `but the composition seals ${expectedToolchain}`,
  );
  process.exit(3);
}
const source = await readFile(sourcePath, "utf8");
const wit = await readFile(witPath, "utf8");
// No ambient network in the loop host: http features disabled.
const { component } = await componentize(source, wit, {
  worldName: "guest",
  disableFeatures: ["http", "fetch-event"],
});
await writeFile(outPath, component);
