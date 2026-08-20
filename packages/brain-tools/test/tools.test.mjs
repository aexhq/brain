import assert from "node:assert/strict";
import test from "node:test";

import { compileTools } from "@aexhq/brain";
import { bash, edit, glob, grep, ls, read, subagents, todo, write } from "../dist/index.js";

test("portable tools are ordinary ordered Brain Tool values", async () => {
  const values = [bash, read, write, edit, glob, grep, ls, todo, subagents];
  assert.deepEqual(values.map((tool) => tool.kind), Array(values.length).fill("brain.tool"));
  const compiled = await compileTools(values);
  assert.deepEqual(compiled.items.map((item) => item.definition.name), values.map((tool) => tool.name));
  assert.equal(compiled.items.at(-1).executor.kind, "intrinsic");
  assert.equal(compiled.bundles.length, values.length - 1);
});
