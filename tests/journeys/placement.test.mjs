import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { brainEnv, inspectEnvironment, inspectTool, tool } from "@aexhq/brain";
import { lazyEnvironment } from "../../examples/lazy-environment.mjs";
import { fixture, callTools, reply, collect } from "./support.mjs";

let allocations = 0;
const east = lazyEnvironment({ allocate: async () => { allocations++; return new Map(); } });
const west = lazyEnvironment({ allocate: async () => { allocations++; return new Map(); } });
const f = fixture({ providers: { east: east.handle, west: west.handle } });
const dispatch = (calls) => (request, response) => request.input.at(-1).type === "function_call_output" ? reply(response) : callTools(response, calls);
const echo = (name) => tool({ name, description: "Echo in a provider", input: z.object({ value: z.string() }), implementation: { type: "reference_echo" } });

test("compose separately configured environments and allocate only on first invocation", { timeout: 30_000 }, async (t) => {
  const eastEnv = f.provider("east", { label: "east workspace" });
  const westEnv = f.provider("west");
  assert.equal(inspectEnvironment(eastEnv).configuration.label, "east workspace");
  assert.equal(inspectEnvironment(eastEnv).driver.url, `${f.upstreamUrl}/east`);
  const first = echo("first")({ env: eastEnv });
  assert.equal(inspectTool(first).environment, eastEnv);
  const before = allocations;
  const session = await f.create(t, { tools: [first, echo("second")({ env: westEnv })] });
  assert.equal(allocations, before);
  f.model = dispatch([{ name: "first", input: { value: "east" } }, { name: "second", input: { value: "west" } }]);
  await session.send("use both");
  assert.equal(allocations, before + 2);
  const events = await collect(session.events());
  assert.equal(events.filter(({ type }) => type === "tool_call_ended").length, 2);
  const started = events.find(({ type }) => type === "tool_call_started");
  assert.deepEqual(Object.keys(started.data).sort(), ["deadline_ms", "environment", "invocation", "tool"]);
  const setup = events.find(({ type, data }) => type === "environment_setup_started" && data.environment === "east");
  assert.deepEqual(setup.data.request.configuration, { label: "east workspace" });
  assert.equal("needs" in setup.data.request, false);
  assert.ok(JSON.stringify(f.modelRequests.at(-1).input).includes("east"));
  assert.ok(JSON.stringify(f.modelRequests.at(-1).input).includes("west"));
});

test("the environment validates its own configuration at setup", { timeout: 30_000 }, async () => {
  const result = await east.handle({ contract: "environment/v1", operation: {
    sequence: 1, environment: "east", session_id: "ses_invalid_config",
    request: { type: "setup", configuration: { label: 123 } },
  } });
  assert.equal(result.receipt.type, "failure");
  assert.equal(result.receipt.code, "invalid_configuration");
});

test("caller teardown is visible without replacement allocation", { timeout: 30_000 }, async (t) => {
  const env = f.provider("east");
  const session = await f.create(t, { tools: [echo("echo")({ env })] });
  f.model = dispatch([{ name: "echo", input: { value: "value" } }]);
  await session.send("allocate");
  const before = allocations;
  await east.handle({ contract: "environment/v1", operation: { session_id: session.id, environment: "east", sequence: 999, request: { type: "teardown" } } });
  await session.send("use the workspace again");
  assert.equal(allocations, before);
  const ended = (await collect(session.events())).filter((event) => event.type === "tool_call_ended").at(-1);
  assert.equal(ended.data.result.is_error, true);
  assert.equal(ended.data.result.output.code, "unavailable");
  assert.ok(JSON.stringify(f.modelRequests.at(-1).input.at(-1)).includes("unavailable"));
});

test("admit a native tool and use it from a model-driven conversation", { timeout: 30_000 }, async (t) => {
  const id = await f.brain.admitTool(f.toolComponent);
  assert.match(id, /^[a-f0-9]{64}$/u);
  const native = tool({ name: "native", description: "Native echo", input: z.object({ value: z.string() }), implementation: f.toolComponent });
  const session = await f.create(t, { tools: [native({ env: brainEnv({ name: "brain" }) })] });
  f.model = dispatch([{ name: "native", input: { value: "native value" } }]);
  await session.send("execute native tool");
  assert.ok(JSON.stringify(f.modelRequests.at(-1).input).includes("native value"));
  assert.equal((await collect(session.events())).filter(({ type }) => type === "tool_progress").length, 1);
});

test("native workspaces persist between turns but remain separate between sessions", { timeout: 30_000 }, async (t) => {
  const workspace = tool({ name: "workspace", description: "Read and write a marker", input: z.object({ workspace: z.boolean(), write: z.string().optional() }),
    implementation: f.toolComponent });
  const placed = workspace({ env: brainEnv({ name: "workspace", filesystem: { workspace: "write" } }) });
  const first = await f.create(t, { tools: [placed] });
  const second = await f.create(t, { tools: [placed] });
  f.model = dispatch([{ name: "workspace", input: { workspace: true, write: "private marker" } }]);
  await first.send("write");
  f.model = dispatch([{ name: "workspace", input: { workspace: true } }]);
  await first.send("read again");
  assert.ok(JSON.stringify(f.modelRequests.at(-1).input.at(-1)).includes("private marker"));
  await second.send("read isolated workspace");
  const output = JSON.stringify(f.modelRequests.at(-1).input.at(-1));
  assert.ok(!output.includes("private marker"));
  assert.ok(output.includes("null"));
});

test("a native tool that needs no workspace cannot write one", { timeout: 30_000 }, async (t) => {
  const write = tool({ name: "write", description: "Write a marker", input: z.object({ workspace: z.boolean(), write: z.string() }), implementation: f.toolComponent });
  const session = await f.create(t, { tools: [write({ env: brainEnv({ name: "brain" }) })] });
  f.model = dispatch([{ name: "write", input: { workspace: true, write: "denied" } }]);
  await session.send("try writing");
  const transcript = await session.transcript();
  const result = transcript.messages.flatMap(({ content }) => content).find(({ type }) => type === "tool_result");
  assert.equal(result.is_error, true);
  assert.equal(f.modelRequests.length, 2);
});

test("the brain env refuses at create what it cannot grant", { timeout: 30_000 }, async (t) => {
  const fetcher = tool({ name: "fetcher", description: "Reach the network", input: z.object({}), implementation: f.toolComponent });
  await assert.rejects(
    f.create(t, { tools: [fetcher({ env: brainEnv({ name: "network", network: ["https://api.example.com"] }) })] }),
    (error) => { assert.match(error.message, /https:\/\/api\.example\.com/u); return true; },
  );
});
