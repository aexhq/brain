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
  writeFileSync(
    join(directory, "smoke.mjs"),
    `import assert from "node:assert/strict";\n` +
      `import { Brain } from "@aexhq/brain";\n` +
      `assert.equal(typeof Brain, "function");\n`,
  );
  run(["install", "--no-audit", "--no-fund"], directory);
  execFileSync(process.execPath, ["smoke.mjs"], { cwd: directory, stdio: "inherit" });
  process.stdout.write("packed Brain packages passed an empty-consumer smoke test\n");
} finally {
  rmSync(directory, { recursive: true, force: true });
}
