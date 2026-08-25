import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
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
  const agentloop = pack("@aexhq/agentloop");
  writeFileSync(
    join(directory, "package.json"),
    `${JSON.stringify({
      private: true,
      type: "module",
      dependencies: {
        "@aexhq/brain": `file:${brain}`,
        "@aexhq/agentloop": `file:${agentloop}`,
      },
    }, null, 2)}\n`,
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
      `import { Brain } from "@aexhq/brain";\n` +
      `import { __bindHostCall, defineAgentloop } from "@aexhq/agentloop";\n` +
      `import { buildLoopBundle } from "@aexhq/agentloop/build";\n` +
      `assert.equal(typeof Brain, "function");\n` +
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
      `assert.match(LOOP_TOOLCHAIN, /^starlingmonkey-componentize-js-/);\n`,
  );
  run(["install", "--no-audit", "--no-fund"], directory);
  execFileSync(process.execPath, ["smoke.mjs"], { cwd: directory, stdio: "inherit" });
  process.stdout.write("packed Brain packages passed an empty-consumer smoke test\n");
} finally {
  rmSync(directory, { recursive: true, force: true });
}
