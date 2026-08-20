import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";

import { buildToolModule, compileTools, defineTool } from "../dist/index.js";
import { z } from "zod";

test("the local builder is deterministic and does not evaluate customer code", async () => {
  const directory = await mkdtemp(join(tmpdir(), "brain-builder-"));
  try {
    const marker = join(directory, "evaluated.txt");
    const modulePath = join(directory, "tool.mjs");
    await writeFile(
      modulePath,
      `import { writeFileSync } from "node:fs";\nwriteFileSync(${JSON.stringify(marker)}, "bad");\nexport default { kind: "brain.tool" };\n`,
    );
    const url = pathToFileURL(modulePath).href;
    const first = await buildToolModule(url);
    const second = await buildToolModule(url);
    assert.equal(first.checksum, second.checksum);
    assert.deepEqual(first.bytes, second.bytes);
    await assert.rejects(readFile(marker), { code: "ENOENT" });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("the conformance fixture has one cross-platform bundle identity", async () => {
  const fixture = new URL("../fixtures/fixture-tool.mjs", import.meta.url).href;
  assert.equal(
    (await buildToolModule(fixture)).checksum,
    "ba5129527642c60c28499ca08036bdc4b00aa2eed881a1df4f6e0a8c5aac79ee",
  );
});

test("custom and official values compile through one ordered Tool path", async () => {
  const module = new URL("../fixtures/fixture-tool.mjs", import.meta.url).href;
  const one = defineTool({
    module,
    name: "one",
    description: "Return one.",
    input: z.object({ value: z.string() }),
    output: z.object({ value: z.string() }),
    execute: async ({ value }) => ({ value }),
  });
  const two = defineTool({
    module,
    name: "two",
    description: "Return two.",
    input: z.object({ value: z.string() }),
    output: z.object({ value: z.string() }),
    execute: async ({ value }) => ({ value }),
  }).local({ callbackId: "callback-two" });
  const compiled = await compileTools([one, two]);
  assert.deepEqual(compiled.items.map((item) => item.definition.name), ["one", "two"]);
  assert.equal(compiled.items[0].executor.kind, "hand");
  assert.equal(compiled.items[1].executor.kind, "attached");
  assert.equal(compiled.attached.get("callback-two"), two);
  await assert.rejects(compileTools([one, one]), /selected more than once/u);
});

test("the builder rejects runtime module discovery before session creation", async () => {
  const directory = await mkdtemp(join(tmpdir(), "brain-builder-dynamic-"));
  try {
    const modulePath = join(directory, "dynamic.mjs");
    await writeFile(
      modulePath,
      `export default { kind: "brain.tool", async execute() { return import(process.env.RUNTIME_MODULE); } };\n`,
    );
    await assert.rejects(
      buildToolModule(pathToFileURL(modulePath).href),
      /dynamic import|unsupported dynamic behavior/u,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
