/**
 * The deterministic source-bundle builder (node-only; import from `@aexhq/agentloop/build`).
 *
 * A custom loop's sealed identity is (source-bundle sha256, toolchain): componentization is
 * non-deterministic, so the deterministic esbuild bundle produced here is what uploads, and
 * the composition componentizes it server-side, cached by that pair. The SDK itself is
 * bundled in from its TypeScript source — the one canonical input — so the same entry always
 * digests the same.
 */

import { createHash } from "node:crypto";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { build, type Plugin } from "esbuild";

/**
 * The loop toolchain compositions run: the pinned guest engine plus componentizer. The other
 * half of a bundle's sealed identity — pass it as the upload's `toolchain`; a composition
 * running a different toolchain refuses the bundle rather than guessing.
 */
export const LOOP_TOOLCHAIN = "starlingmonkey-componentize-js-0.22.0";

/** The upload bound for a source bundle (contracts/agentloop/v1 `bundle_base64` limit). */
export const MAX_LOOP_BUNDLE_BYTES = 8 * 1024 * 1024;

export interface LoopBundle {
  /** The complete ESM source bundle, the exact bytes to upload. */
  source: string;
  /** SHA-256 of the UTF-8 source bytes: half of the sealed identity. */
  sha256: string;
  bytes: number;
}

export interface BuildLoopBundleOptions {
  /** The loop entry module exporting `activate` (typically via `defineAgentloop`). */
  entry: string;
  /**
   * Skip the Unicode-property-escape gate. The pinned guest engine rejects `\p{…}` in regex
   * literals at parse time, so a bundle carrying one fails at session create; only set this
   * when every occurrence is provably inside a plain string.
   */
  allowUnicodePropertyEscapes?: boolean;
  /**
   * esbuild plugins applied while bundling — the hook for dependency compatibility rewrites
   * (e.g. replacing a library's `\p{…}` regex literals with engine-parseable equivalents).
   * Plugins shape the deterministic source bundle, so they are part of what the sealed
   * digest covers.
   */
  plugins?: Plugin[];
  /** Modules injected before the bundle's own top-level code (esbuild `inject`) — polyfills. */
  inject?: string[];
}

const SDK_ENTRY = fileURLToPath(new URL("../src/index.ts", import.meta.url));

export async function buildLoopBundle(options: BuildLoopBundleOptions): Promise<LoopBundle> {
  const entryPath = resolve(options.entry);
  // The virtual entry binds the real host import around the author's module, so authored
  // loops never import "loophost:abi/host" themselves and stay unit-testable in node.
  const virtualEntry = [
    'import { call } from "loophost:abi/host";',
    'import { __bindHostCall } from "@aexhq/agentloop";',
    "__bindHostCall(call);",
    `export { activate } from ${JSON.stringify(entryPath.replaceAll("\\", "/"))};`,
    "",
  ].join("\n");
  const bundled = await build({
    stdin: {
      contents: virtualEntry,
      resolveDir: dirname(entryPath),
      loader: "js",
      sourcefile: "agentloop-entry.js",
    },
    bundle: true,
    format: "esm",
    platform: "neutral",
    external: ["loophost:abi/host"],
    alias: { "@aexhq/agentloop": SDK_ENTRY },
    plugins: options.plugins ?? [],
    inject: options.inject ?? [],
    write: false,
    legalComments: "none",
  });
  const output = bundled.outputFiles[0];
  if (!output) {
    throw new Error("esbuild produced no output for the loop bundle");
  }
  const source = output.text;
  lintLoopBundle(source, options);
  const bytes = Buffer.byteLength(source, "utf8");
  if (bytes > MAX_LOOP_BUNDLE_BYTES) {
    throw new Error(`the loop bundle is ${bytes} bytes; the upload bound is 8 MiB`);
  }
  const sha256 = createHash("sha256").update(source, "utf8").digest("hex");
  return { source, sha256, bytes };
}

/**
 * Refuse bundles that would fail at guest parse time. `\p{…}`/`\P{…}` in a regex literal is a
 * SyntaxError in the pinned engine; occurrences inside plain strings are indistinguishable
 * without a full parse, so the gate is strict with an explicit override.
 */
export function lintLoopBundle(
  source: string,
  options?: Pick<BuildLoopBundleOptions, "allowUnicodePropertyEscapes">,
): void {
  if (options?.allowUnicodePropertyEscapes) {
    return;
  }
  const offenders: number[] = [];
  const lines = source.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    if (/\\[pP]\{/.test(lines[index] ?? "")) {
      offenders.push(index + 1);
    }
  }
  if (offenders.length > 0) {
    throw new Error(
      `the bundle contains Unicode property escapes (\\p{…}) on line(s) ${offenders
        .slice(0, 8)
        .join(", ")}; the guest engine rejects them in regex literals at parse time. ` +
        "Rewrite the pattern with explicit ranges, or pass allowUnicodePropertyEscapes: true " +
        "if every occurrence is provably inside a plain string.",
    );
  }
}
