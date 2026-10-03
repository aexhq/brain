import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const entry = fileURLToPath(new URL("../dist/agentloop.js", import.meta.url));
const bundled = await build({
  stdin: { contents: `export { defineAgentloop } from ${JSON.stringify(entry)}; export * as host from "brain:agentloop/host@0.2.0";`, resolveDir: process.cwd() },
  bundle: true, format: "esm", platform: "neutral", write: false,
  plugins: [{ name: "brain-host", setup(build) {
    build.onResolve({ filter: /^brain:agentloop\/host/ }, args => ({ path: args.path, namespace: "brain-host" }));
    build.onLoad({ filter: /.*/, namespace: "brain-host" }, () => ({ contents: `
      export const state = new Map();
      export const calls = [];
      export function kvRead(key) { return state.get(key); }
      export function kvPut(key, value) { state.set(key, value); return 7n; }
      export function kvDelete(key) { state.delete(key); return 8n; }
      export function setTranscript(messages) { state.set("transcript", messages); return 9n; }
      export function model(request) {
        calls.push(JSON.parse(request));
        return JSON.stringify({message: {role: "assistant", content: [{type: "text", text: "hello"}]}, stop_reason: "end_turn", usage: {}});
      }
      export function dispatch(request) {
        return JSON.stringify(JSON.parse(request).map(call => ({call_id: call.call_id, sequence: 10, events: [], finished: false})));
      }
      export function events(after) { return JSON.stringify({events: [], next_cursor: Number(after)}); }
      export function emit() { return 11n; }
      export function environments() { return "[]"; }
      export function telemetry() {}
    ` }));
  } }],
});
const { defineAgentloop, host } = await import(`data:text/javascript;base64,${Buffer.from(bundled.outputFiles[0].text).toString("base64")}`);
const input = () => ({
  inputJson: '{"message":"hello"}', transcriptJson: "[]", kvJson: "{}", eventsJson: "[]",
  configurationJson: "{}", toolsJson: "[]", system: "help",
  runtime: { logicalTimeMs: 1n, deterministicSeed: new Uint8Array(32) },
});

test("a loop uses one context with durable KV and typed model and tool calls", async () => {
  host.state.clear();
  host.calls.length = 0;
  const turn = defineAgentloop(async ctx => {
    assert.equal(ctx.input.message, "hello");
    assert.equal("acknowledge" in ctx, false);
    assert.equal(await ctx.kv.get("missing"), undefined);
    await ctx.kv.set("value", null);
    assert.equal(await ctx.kv.get("value"), null);
    await ctx.kv.set("value", { count: 2 });
    assert.deepEqual(await ctx.kv.get("value"), { count: 2 });
    await ctx.kv.delete("value");
    assert.equal(await ctx.kv.get("value"), undefined);
    const reply = await ctx.model({ messages: [], tools: [] });
    assert.equal(host.state.has("transcript"), false);
    await ctx.setTranscript([reply.message]);
    const call = { call_id: "lookup-1", name: "lookup", environment: "app", input: { id: 1 } };
    assert.deepEqual(await ctx.callTool(call), { call_id: "lookup-1", sequence: 10, events: [], finished: false });
    assert.equal((await ctx.callTools([call, { ...call, call_id: "lookup-2" }])).length, 2);
    assert.deepEqual(await ctx.readEvents(10), { events: [], next_cursor: 10 });
    assert.deepEqual(await ctx.environments.list(), []);
    return { message: reply.message.content[0].text };
  });
  assert.deepEqual(await turn(input()), { resultJson: '{"message":"hello"}' });
  assert.equal(JSON.parse(host.state.get("transcript"))[0].role, "assistant");
});

test("a failed handler preserves earlier writes and its failure code", async () => {
  host.state.clear();
  const turn = defineAgentloop(async ctx => {
    await ctx.kv.set("saved", true);
    throw Object.assign(new Error("budget reached"), { code: "decision_limit" });
  });
  await assert.rejects(turn(input()), error => error.payload.code === "decision_limit");
  assert.equal(host.state.get("saved"), "true");
});

test("background activations have no fabricated input and undefined is not a stored JSON value", async () => {
  const background = { ...input(), inputJson: "null" };
  assert.deepEqual(await defineAgentloop(ctx => { assert.equal(ctx.input, undefined); })(background), { resultJson: undefined });
  await assert.rejects(defineAgentloop(ctx => ctx.kv.set("invalid", undefined))(background), /JSON value/);
});
