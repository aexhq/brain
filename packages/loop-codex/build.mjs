// Build the publishable loop artifact with the public toolchain — exactly what any external
// loop author runs. The output pair (deterministic source bundle + sealed identity) is what a
// composition seeds or a customer uploads; componentization always happens server-side.
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { LOOP_TOOLCHAIN, buildLoopBundle } from "@aexhq/agentloop/build";

const here = (path) => fileURLToPath(new URL(path, import.meta.url));
const pkg = JSON.parse(await readFile(here("./package.json"), "utf8"));

const bundle = await buildLoopBundle({ entry: here("./src/loop.mjs") });

await mkdir(here("./dist"), { recursive: true });
await writeFile(here("./dist/loop.bundle.mjs"), bundle.source);
await writeFile(
  here("./dist/identity.json"),
  `${JSON.stringify(
    {
      name: pkg.agentloop.name,
      version: pkg.version,
      toolchain: LOOP_TOOLCHAIN,
      source_bundle_sha256: bundle.sha256,
      bytes: bundle.bytes,
    },
    null,
    2,
  )}\n`,
);
// stderr: `npm pack --json` consumers parse stdout.
console.error(`${pkg.agentloop.name}@${pkg.version} -> ${bundle.sha256} (${bundle.bytes} bytes)`);
