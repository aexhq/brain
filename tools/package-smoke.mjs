import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const directory = mkdtempSync(join(tmpdir(), "brain-package-smoke-"));
const npmEntry = process.env.npm_execpath;
assert.ok(npmEntry, "package smoke must be started through npm");

function run(args, cwd = root) {
  return execFileSync(process.execPath, [npmEntry, ...args], {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
}

function pack(workspace) {
  const result = JSON.parse(
    run(["pack", "--workspace", workspace, "--pack-destination", directory, "--json"]),
  );
  assert.equal(result.length, 1, `npm pack returned an unexpected result for ${workspace}`);
  return join(directory, result[0].filename);
}

try {
  const brain = pack("@aexhq/brain");
  const tools = pack("@aexhq/brain-tools");
  const agentloop = pack("@aexhq/agentloop");
  const loopPi = pack("@aexhq/loop-pi");
  const loopCodex = pack("@aexhq/loop-codex");
  writeFileSync(
    join(directory, "package.json"),
    `${JSON.stringify({
      private: true,
      type: "module",
      dependencies: {
        "@aexhq/brain": `file:${brain}`,
        "@aexhq/brain-tools": `file:${tools}`,
        "@aexhq/agentloop": `file:${agentloop}`,
        "@aexhq/loop-pi": `file:${loopPi}`,
        "@aexhq/loop-codex": `file:${loopCodex}`,
      },
    }, null, 2)}\n`,
  );
  // The independence proof: the identity a consumer installs from the packed loop package is
  // byte-for-byte the identity the repo build produced — pack-time prepack rebuilds through
  // the public buildLoopBundle and must land on the same deterministic digest.
  const repoIdentity = (workspace) =>
    JSON.parse(readFileSync(join(root, "packages", workspace, "dist", "identity.json"), "utf8"));
  writeFileSync(
    join(directory, "expected-identities.json"),
    JSON.stringify({ pi: repoIdentity("loop-pi"), codex: repoIdentity("loop-codex") }),
  );
  writeFileSync(
    join(directory, "loop.mjs"),
    `import { defineAgentloop } from "@aexhq/agentloop";\n` +
      `export const { activate } = defineAgentloop({\n` +
      `  async onMessage(ctx) { await ctx.turn.finish({ ok: true }); },\n` +
      `});\n`,
  );
  writeFileSync(
    join(directory, "smoke.mjs"),
    `import assert from "node:assert/strict";\n` +
      `import { Brain, compileTools } from "@aexhq/brain";\n` +
      `import { read } from "@aexhq/brain-tools";\n` +
      `import { __bindHostCall, defineAgentloop } from "@aexhq/agentloop";\n` +
      `import { buildLoopBundle } from "@aexhq/agentloop/build";\n` +
      `assert.equal(typeof Brain, "function");\n` +
      `const compiled = await compileTools([read]);\n` +
      `assert.equal(compiled.items[0].definition.name, "read");\n` +
      `assert.equal(compiled.items[0].executor.kind, "aex_managed");\n` +
      `assert.equal(compiled.bundles.length, 1);\n` +
      `assert.ok(compiled.bundles[0].bytes > 0);\n` +
      `const ops = [];\n` +
      `__bindHostCall((payload) => {\n` +
      `  ops.push(JSON.parse(payload).op.op);\n` +
      `  return JSON.stringify({ op_id: "x", result: { op: "turn_finish" } });\n` +
      `});\n` +
      `const { activate } = defineAgentloop({\n` +
      `  async onMessage(ctx) { await ctx.turn.finish({ ok: true }); },\n` +
      `});\n` +
      `const returned = JSON.parse(await activate("message", JSON.stringify({\n` +
      `  kind: "message", activation_id: "act-1",\n` +
      `  session: { session_id: "ses_00000000000000000001", model: "m",\n` +
      `             limits: { max_rounds_per_turn: 1, turn_wall_ms: 1, max_parallel_tools: 1 } },\n` +
      `  message: { seq: 1, at: "2026-01-01T00:00:00Z", content: [] },\n` +
      `})));\n` +
      `assert.equal(returned.outcome, "completed");\n` +
      `assert.deepEqual(ops, ["turn_finish"]);\n` +
      `const bundle = await buildLoopBundle({ entry: "./loop.mjs" });\n` +
      `assert.equal(bundle.sha256.length, 64);\n` +
      `assert.ok(bundle.source.includes("activate"));\n` +
      `const { LOOP_TOOLCHAIN } = await import("@aexhq/agentloop/build");\n` +
      `const { readFile } = await import("node:fs/promises");\n` +
      `const { createRequire } = await import("node:module");\n` +
      `const require = createRequire(import.meta.url);\n` +
      `const expected = JSON.parse(await readFile("./expected-identities.json", "utf8"));\n` +
      `const installedIdentity = async (specifier) =>\n` +
      `  JSON.parse(await readFile(require.resolve(specifier), "utf8"));\n` +
      `assert.deepEqual(await installedIdentity("@aexhq/loop-pi/identity"), expected.pi);\n` +
      `assert.deepEqual(await installedIdentity("@aexhq/loop-codex/identity"), expected.codex);\n` +
      `assert.equal(expected.pi.toolchain, LOOP_TOOLCHAIN);\n` +
      `assert.equal(expected.codex.toolchain, LOOP_TOOLCHAIN);\n`,
  );
  run(["install", "--no-audit", "--no-fund"], directory);
  execFileSync(process.execPath, ["smoke.mjs"], { cwd: directory, stdio: "inherit" });
  process.stdout.write("packed Brain packages passed an empty-consumer smoke test\n");
} finally {
  rmSync(directory, { recursive: true, force: true });
}
