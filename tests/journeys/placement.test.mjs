import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { brainWasm, environment, inspectEnvironment, inspectPlacedTool, tool } from "@aexhq/brain";
import { lazyEnvironment } from "../../examples/lazy-environment.mjs";
import { fixture, callTools, reply, collect } from "./support.mjs";

let clock = 0;
let allocations = 0;
const east = lazyEnvironment({ now: () => clock, allocate: async () => { allocations++; return new Map(); } });
const west = lazyEnvironment({ allocate: async () => { allocations++; return new Map(); } });
const f = fixture({ providers: { east: east.handle, west: west.handle } });
const dispatch = (calls) => (request, response) => request.messages.at(-1).role === "tool" ? reply(response) : callTools(response, calls);
const echo = (name) => tool({ name, description: "Echo in a provider", input: z.object({ value: z.string() }), implementation: { type: "reference_echo" } });

test("compose separately configured environments and allocate only on first invocation", { timeout: 30_000 }, async (t) => {
  const eastFactory = environment({ driver: "east", options: z.object({ idleMs: z.number() }),
    configure: ({ idleMs }) => ({ idle_ms: idleMs }), bindings: () => ({ label: "east" }) });
  const eastEnv = eastFactory({ idleMs: 10_000 });
  const westEnv = environment({ driver: "west" })();
  assert.equal(inspectEnvironment(eastEnv).configuration.idle_ms, 10_000);
  assert.deepEqual(inspectEnvironment(eastEnv).bindings, { label: "east" });
  const first = echo("first")({ env: eastEnv });
  assert.equal(inspectPlacedTool(first).environment, eastEnv);
  const before = allocations;
  const session = await f.create(t, { tools: [first, echo("second")({ env: westEnv })] });
  assert.equal(allocations, before);
  f.model = dispatch([{ name: "first", input: { value: "east" } }, { name: "second", input: { value: "west" } }]);
  await session.send("use both");
  assert.equal(allocations, before + 2);
  assert.equal((await collect(session.events())).filter(({ type }) => type === "tool_call_ended").length, 2);
  assert.ok(JSON.stringify(f.modelRequests.at(-1).messages).includes("east"));
  assert.ok(JSON.stringify(f.modelRequests.at(-1).messages).includes("west"));
});

test("provider expiry is visible to the model without replacement allocation", { timeout: 30_000 }, async (t) => {
  const env = environment({ driver: "east", options: z.object({ idle_ms: z.number() }) })({ idle_ms: 1 });
  const session = await f.create(t, { tools: [echo("echo")({ env })] });
  f.model = dispatch([{ name: "echo", input: { value: "value" } }]);
  await session.send("allocate");
  const before = allocations;
  clock += 10;
  await session.send("use expired environment");
  assert.equal(allocations, before);
  assert.ok(JSON.stringify(f.modelRequests.at(-1).messages).includes("expired"));
});

test("admit a native tool and use it from a model-driven conversation", { timeout: 30_000 }, async (t) => {
  const identity = await f.brain.admitTool(f.toolComponent);
  assert.match(identity, /^[a-f0-9]{64}$/u);
  const native = tool({ name: "native", description: "Native echo", input: z.object({ value: z.string() }), implementation: f.toolComponent });
  const session = await f.create(t, { tools: [native({ env: brainWasm() })] });
  f.model = dispatch([{ name: "native", input: { value: "native value" } }]);
  await session.send("execute native tool");
  assert.ok(JSON.stringify(f.modelRequests.at(-1).messages).includes("native value"));
  assert.equal((await collect(session.events())).filter(({ type }) => type === "tool_progress").length, 1);
});

test("native workspaces persist between turns but remain separate between sessions", { timeout: 30_000 }, async (t) => {
  const workspace = tool({ name: "workspace", description: "Read and write a marker", input: z.object({ workspace: z.boolean(), write: z.string().optional() }),
    implementation: f.toolComponent });
  const binding = workspace({ env: brainWasm({ filesystem: { workspace: true } }) });
  const first = await f.create(t, { tools: [binding] });
  const second = await f.create(t, { tools: [binding] });
  f.model = dispatch([{ name: "workspace", input: { workspace: true, write: "private marker" } }]);
  await first.send("write");
  f.model = dispatch([{ name: "workspace", input: { workspace: true } }]);
  await first.send("read again");
  assert.ok(JSON.stringify(f.modelRequests.at(-1).messages.at(-1)).includes("private marker"));
  await second.send("read isolated workspace");
  const output = JSON.stringify(f.modelRequests.at(-1).messages.at(-1));
  assert.ok(!output.includes("private marker"));
  assert.ok(output.includes("null"));
});

test("a native tool without a workspace grant cannot write a workspace", { timeout: 30_000 }, async (t) => {
  const write = tool({ name: "write", description: "Write a marker", input: z.object({ workspace: z.boolean(), write: z.string() }), implementation: f.toolComponent });
  const session = await f.create(t, { tools: [write({ env: brainWasm() })] });
  f.model = dispatch([{ name: "write", input: { workspace: true, write: "denied" } }]);
  await session.send("try writing");
  const transcript = await session.transcript();
  const result = transcript.messages.flatMap(({ content }) => content).find(({ type }) => type === "tool_result");
  assert.equal(result.is_error, true);
  assert.equal(f.modelRequests.length, 2);
});
