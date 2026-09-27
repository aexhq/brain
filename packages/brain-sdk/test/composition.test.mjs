import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { Brain, agentloop, brainEnv, clientBrowser, inspectTool, tool } from "../dist/index.js";
import { runToolHandler } from "../dist/runtime.js";

test("the default browser transport retains the global fetch receiver", async () => {
  const original = globalThis.fetch;
  globalThis.fetch = async function () {
    assert.equal(this, globalThis);
    return Response.json({ sessions: [] });
  };
  const client = new Brain({ baseUrl: "https://brain.example" });
  try { assert.deepEqual(await client.sessions.list(), []); }
  finally { await client.close(); globalThis.fetch = original; }
});

test("prepare compiles a browser placement without opening a host or executing its handler", async () => {
  const client = new Brain({ baseUrl: "https://brain.example", fetch: () => { throw new Error("unexpected transport"); } });
  const read = tool({ name: "read_selection", description: "Read selection", input: z.object({}),
    run: () => { throw new Error("must not execute during preparation"); } });
  const loop = agentloop({ implementation: { type: "brain_component", entrypoint: "turn", id: "loop" } });
  const options = { model: { provider: "openai", name: "model", apiKey: "private-model-key" },
    agentloop: loop({ env: brainEnv({ name: "brain" }) }), tools: [read({ env: clientBrowser({ name: "editor" }) })] };
  const request = await client.sessions.prepare(options, "authorized-host");
  assert.deepEqual(request.environments.find(env => env.name === "editor"),
    { name: "editor", lifecycle: "automatic", driver: "host", host_id: "authorized-host", configuration: {} });
  assert.deepEqual(request.tools[0].placements.editor.implementation, { type: "host_function", name: "read_selection" });
  await assert.rejects(client.sessions.prepare(options), /requires a hostId/);
  await assert.rejects(client.sessions.prepare(options, "invalid host"), /requires a hostId/);
  await client.close();
});

test("a declared Tool awaits the actual completion acknowledgment and preserves content", async () => {
  let acknowledge;
  let afterFinish = false;
  let observed;
  const called = new Promise(resolve => { observed = resolve; });
  const ack = new Promise(resolve => { acknowledge = resolve; });
  const read = tool({ name: "read", description: "Read", input: z.object({}), output: z.object({ count: z.number() }),
    run: async (_, ctx) => { await ctx.finish({ count: 3 }, { content: "Three records" }); afterFinish = true; } });
  const updates = [];
  const running = runToolHandler(inspectTool(read()), { sessionId: "session", environment: "app", sequence: 1, arguments: {},
    update: async value => { updates.push(value); observed(); return ack; },
    emit: unsupported, model: unsupported, environments: unsupported });
  await called;
  assert.equal(afterFinish, false);
  assert.deepEqual(updates, [{ type: "finish", outcome: { status: "ok", value: { count: 3 }, content: "Three records" } }]);
  acknowledge(7);
  await running;
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(afterFinish, true);
  assert.equal(updates.length, 1);
});

test("declared Tool cancellation closes an unfinished invocation", async () => {
  const controller = new AbortController();
  const updates = [];
  const read = tool({ name: "read", description: "Read", input: z.object({}), run: () => {} });
  await runToolHandler(inspectTool(read()), { sessionId: "session", environment: "app", sequence: 1, arguments: {},
    update: async value => { updates.push(value); if (value.type === "returned") controller.abort(); return updates.length; },
    emit: unsupported, model: unsupported, environments: unsupported }, controller.signal);
  assert.deepEqual(updates, [{ type: "returned" }, { type: "finish", outcome: { status: "cancelled" } }]);
});

function unsupported() { throw new Error("unexpected invocation service"); }
