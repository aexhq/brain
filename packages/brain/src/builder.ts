import { createHash } from "node:crypto";
import { builtinModules } from "node:module";
import { dirname, extname, normalize, relative } from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

export interface PreparedBundle {
  readonly checksum: string;
  readonly bytes: Uint8Array;
}

/**
 * Bundle a Tool module without importing it. The returned ESM is first evaluated by the Environment's
 * per-call runner, after Brain has durably journaled the call intent.
 */
export async function buildToolModule(moduleUrl: string): Promise<PreparedBundle> {
  let modulePath: string;
  try {
    const url = new URL(moduleUrl);
    if (url.protocol !== "file:") throw new TypeError("deployable Tool modules must use file: URLs");
    modulePath = fileURLToPath(url);
  } catch (cause) {
    throw new TypeError("Tool module must be an explicit import.meta.url file URL", { cause });
  }

  if (![".js", ".mjs", ".cjs", ".ts", ".mts", ".cts", ".tsx", ".jsx"].includes(extname(modulePath))) {
    throw new TypeError(`Unsupported Tool module extension ${extname(modulePath) || "(none)"}`);
  }
  const resolveDir = dirname(modulePath);
  const relativeEntry = `./${relative(resolveDir, modulePath).replaceAll("\\", "/")}`;
  let result;
  try {
    result = await build({
      bundle: true,
      format: "esm",
      platform: "node",
      target: "node22",
      write: false,
      minify: true,
      legalComments: "none",
      sourcemap: false,
      metafile: true,
      treeShaking: true,
      charset: "utf8",
      logLevel: "silent",
      stdin: {
        contents: `
import selected from ${JSON.stringify(relativeEntry)};
if (selected === null || typeof selected !== "object" || selected.kind !== "brain.tool" || selected.execution !== "aex_managed") {
  throw new TypeError("default export is not an Aex-managed Brain Tool");
}
if (typeof selected.execute !== "function") throw new TypeError("Aex-managed Brain Tool has no executable handler");
const runtime = Object.freeze({
  kind: "brain.tool-runtime",
  name: selected.name,
  description: selected.description,
  contractDigest: selected.contract.contractDigest,
  input: selected.input,
  output: selected.output,
  requiredEnv: selected.requiredEnv,
  execute: selected.execute,
});
export default runtime;
`,
        resolveDir,
        sourcefile: "brain-tool-entry.mjs",
        loader: "js",
      },
    });
  } catch (cause) {
    throw new TypeError(
      "Tool could not be bundled for Node 22. Native addons, unresolved dynamic modules, install-script output, and external runtime files are not supported; choose .client() when those are required.",
      { cause },
    );
  }
  if (result.warnings.length !== 0) {
    throw new TypeError(`Tool bundle has unsupported dynamic behavior: ${result.warnings[0]?.text ?? "esbuild warning"}`);
  }
  for (const input of Object.keys(result.metafile.inputs)) {
    if (extname(input) === ".node") throw new TypeError(`Native Node addon is not supported: ${input}`);
  }
  const builtins = new Set(builtinModules.flatMap((name) => [name, `node:${name}`]));
  for (const output of Object.values(result.metafile.outputs)) {
    for (const imported of output.imports) {
      if (imported.external && !builtins.has(imported.path)) {
        throw new TypeError(`Tool bundle leaves an unsupported runtime import: ${imported.path}`);
      }
    }
  }
  const output = result.outputFiles[0];
  if (output === undefined || result.outputFiles.length !== 1) throw new TypeError("Tool build did not produce one ESM file");
  const text = output.text;
  if (/\bimport\s*\(/u.test(text)) {
    throw new TypeError(
      "Tool bundle contains a dynamic import that cannot be prepared reproducibly; use static imports or choose .client().",
    );
  }
  const normalizedRoot = normalize(resolveDir).replaceAll("\\", "/");
  if (text.includes(normalizedRoot) || /^[A-Za-z]:\//mu.test(text)) {
    throw new TypeError("Tool bundle contains a local absolute path");
  }
  const bytes = new Uint8Array(output.contents);
  return { checksum: createHash("sha256").update(bytes).digest("hex"), bytes };
}
