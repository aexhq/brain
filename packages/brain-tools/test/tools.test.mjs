import assert from "node:assert/strict";
import test from "node:test";

import { compileTools } from "@aexhq/brain";
import { bash, edit, glob, grep, ls, read, sandbox, storage, subagents, todo, write } from "../dist/index.js";

test("portable tools are ordinary ordered Brain Tool values", async () => {
  const values = [bash, read, write, edit, glob, grep, ls, todo, storage, sandbox, subagents];
  assert.deepEqual(values.map((tool) => tool.kind), Array(values.length).fill("brain.tool"));
  const compiled = await compileTools(values);
  assert.deepEqual(compiled.items.map((item) => item.definition.name), values.map((tool) => tool.name));
  assert.equal(compiled.items.at(-1).executor.kind, "engine");
  assert.equal(compiled.bundles.length, values.length - 3);
});

test("portable managed tools survive exact minified bundle evaluation", async () => {
  const managed = [bash, read, write, edit, glob, grep, ls, todo];
  const compiled = await compileTools(managed);
  assert.equal(compiled.bundles.length, managed.length);

  for (const [index, bundle] of compiled.bundles.entries()) {
    const loaded = await import(`data:text/javascript;base64,${bundle.content_base64}`);
    assert.equal(loaded.default.kind, "brain.tool-runtime");
    assert.equal(loaded.default.name, managed[index].name);
    assert.equal(typeof loaded.default.execute, "function");
  }
});

test("subagents exposes one provider-friendly action object", async () => {
  const compiled = await compileTools([subagents]);
  const schema = compiled.items[0].definition.input_schema;
  assert.equal(schema.type, "object");
  assert.equal(schema.oneOf, undefined);
  assert.doesNotThrow(() => subagents.input.parse({
    action: "spawn_agent",
    task_name: "smoke",
    message: "Reply CHILD_OK",
    fork_turns: "none",
    child_id: "unused",
    timeout_ms: 1,
    cursor: "",
    limit: 1,
  }));
});
