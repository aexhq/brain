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
  writeFileSync(
    join(directory, "package.json"),
    `${JSON.stringify({
      private: true,
      type: "module",
      dependencies: {
        "@aexhq/brain": `file:${brain}`,
      },
    }, null, 2)}\n`,
  );
  run(["install", "--no-audit", "--no-fund"], directory);
  writeFileSync(
    join(directory, "smoke.mjs"),
    `import assert from "node:assert/strict";\n` +
      `import { readFileSync } from "node:fs";\n` +
      `import * as brainSdk from "@aexhq/brain";\n` +
      `import { serveHarness } from "@aexhq/brain/harness";\n` +
      `const { Brain } = brainSdk;\n` +
      `for (const name of ["tool"]) assert.match(readFileSync(new URL(import.meta.resolve("@aexhq/brain/contracts/" + name + ".wit")), "utf8"), /package brain:/u);\n` +
      `globalThis.fetch = async () => { throw new Error("validated Brain reached fetch"); };\n` +
      `const brain = new Brain({ baseUrl: "http://127.0.0.1:8080" });\n` +
      `assert.equal(typeof brain.sessions.create, "function");\n` +
      `assert.equal(typeof brainSdk.SessionHandle.prototype.interrupt, "function");\n` +
      `assert.equal(typeof serveHarness, "function");\n` +
      `await assert.rejects(brain.sessions.create({ agentloop: { name: "fixture" }, model: { provider: "vercel-ai-gateway", name: "openai/test", apiKey: "test" } }), /validated Brain reached fetch/u);\n` +
      `assert.equal(typeof brainSdk.tool, "function");\n` +
      `assert.equal(typeof brainSdk.environment, "function");\n` +
      `assert.equal("DurableEventBridge" in brainSdk, false);\n` +
      `await brain.close();\n` +
      `await assert.rejects(brain.sessions.list(), { name: "AbortError" });\n`,
  );
  execFileSync(process.execPath, ["smoke.mjs"], { cwd: directory, stdio: "inherit" });
  process.stdout.write("packed Brain packages passed an empty-consumer smoke test\n");
} finally {
  rmSync(directory, { recursive: true, force: true });
}
