// Build the guest loops into wasm components. No ambient network: http features disabled.
import { componentize } from "@bytecodealliance/componentize-js";
import { build } from "esbuild";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const wit = await readFile("../wit/guest.wit", "utf8");
await mkdir("dist", { recursive: true });

// The SDK resolves from its in-repo TypeScript source — the same canonical bundling input
// `buildLoopBundle` pins for uploaded loops.
const sdkEntry = fileURLToPath(
  new URL("../../../packages/agentloop/src/index.ts", import.meta.url),
);

// COMPAT (recorded in the H0 report): the pinned StarlingMonkey engine rejects Unicode
// property escapes in regex literals at parse time. typebox (a pi dependency) emits
// `\p{ID_Start}` in its identifier guard and builds IDN validators from `\p{…}` classes; the
// guard rewrites to an ASCII-equivalent class and the unused IDN modules stub out.
const UNICODE_ID_REGEX_PATTERN = /\/\^\[\\p\{ID_Start\}[^/]*\*\$\/u/g;
const ASCII_ID_REGEX = String.raw`/^[A-Za-z_$][A-Za-z0-9_$]*$/`;
const compatRewrite = {
  name: "starlingmonkey-compat",
  setup(b) {
    b.onLoad({ filter: /typebox[\\/].*\.mjs$/ }, async (args) => {
      if (/[\\/]format[\\/]idn_email\.mjs$/.test(args.path)) {
        return { contents: "export function IsIdnEmail(){return false;}", loader: "js" };
      }
      if (/[\\/]format[\\/]_idna\.mjs$/.test(args.path)) {
        return {
          contents:
            "export function IsIdnLabel(){return false;}\nexport function IsLabel(){return false;}",
          loader: "js",
        };
      }
      let contents = await readFile(args.path, "utf8");
      if (UNICODE_ID_REGEX_PATTERN.test(contents)) {
        contents = contents.replace(UNICODE_ID_REGEX_PATTERN, ASCII_ID_REGEX);
      }
      return { contents, loader: "js" };
    });
  },
};

for (const guest of [
  { entry: "loop-aex.mjs", out: "dist/aex-loop.component.wasm" },
  { entry: "loop-contract.mjs", out: "dist/contract-loop.component.wasm" },
  { entry: "loop-sdk.mjs", out: "dist/sdk-loop.component.wasm", sdk: true, emitSource: true },
  { entry: "loop-pi.mjs", out: "dist/pi-loop.component.wasm", sdk: true, compat: true },
]) {
  const common = {
    bundle: true,
    format: "esm",
    platform: "neutral",
    external: ["loophost:abi/host"],
    plugins: guest.compat ? [compatRewrite] : [],
    inject: guest.compat ? ["./polyfill.mjs"] : [],
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
  if (guest.emitSource) {
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
