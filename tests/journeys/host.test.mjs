import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { hostEnv, tool, inspectTool } from "@aexhq/brain";
import { fixture, collect, callTools, reply, deferred, failure } from "./support.mjs";

const f = fixture();
const app = hostEnv({ name: "app" });
const dispatch = (name, input) => (request, response) => request.messages.at(-1).role === "tool"
  ? reply(response) : callTools(response, [{ name, input }]);

test("a tool this process holds receives validated options and commits progress before its result", { timeout: 30_000 }, async (t) => {
  const contexts = [];
  const lookup = tool({ name: "lookup", description: "Look up a value", input: z.object({ id: z.string() }),
    output: z.object({ value: z.string() }), options: z.object({ prefix: z.string() }),
    run: async ({ id }, context) => {
      contexts.push(context);
      await context.emit("lookup_progress", { id });
      return { value: context.options.prefix + id };
    } });
  const placed = lookup({ env: app, prefix: "item-" });
  assert.equal(inspectTool(placed).definition.name, "lookup");
  f.model = dispatch("lookup", { id: "42" });
  const session = await f.create(t, { tools: [placed] });
  await session.send("find item");
  assert.equal(contexts.length, 1);
  assert.ok(contexts[0].deadline instanceof Date);
  assert.ok(contexts[0].signal instanceof AbortSignal);
  const events = await collect(session.events());
  const started = events.find(({ type }) => type === "tool_call_started");
  assert.equal(contexts[0].sequence, started.sequence, "the call is named by its started record");
  assert.ok(events.find(({ type }) => type === "lookup_progress").sequence < events.find(({ type }) => type === "tool_call_ended").sequence);
  assert.ok(JSON.stringify(f.modelRequests.at(-1).messages).includes("item-42"));
});

for (const mode of ["input", "output", "throw"]) {
  test(`a host ${mode} failure reaches the model once without automatic retry`, { timeout: 30_000 }, async (t) => {
    let calls = 0;
    const lookup = tool({ name: "lookup", description: "Validate a result", input: z.object({ id: z.string() }),
      output: z.object({ value: z.string() }), run: () => {
        calls++;
        if (mode === "throw") throw new Error("lookup unavailable");
        return { value: 42 };
      } });
    f.model = dispatch("lookup", { id: mode === "input" ? 42 : "42" });
    const session = await f.create(t, { tools: [lookup({ env: app })] });
    await session.send("lookup");
    assert.equal(calls, mode === "input" ? 0 : 1);
    const expected = mode === "throw" ? "tool_error" : `invalid_${mode}`;
    assert.ok(JSON.stringify(f.modelRequests.at(-1).messages).includes(expected));
    assert.equal(f.modelRequests.length, 2);
  });
}

test("saved host credentials reattach a tool after its connection is closed", { timeout: 30_000 }, async (t) => {
  const firstClient = f.client();
  const lookup = tool({ name: "lookup", description: "Lookup", input: z.object({}), run: () => "original" });
  const original = await f.create(t, { tools: [lookup({ env: app })] }, firstClient);
  const credentials = await firstClient.credentials();
  const host = await firstClient.register();
  host.pump.stop();
  await host.pump.closed;
  const restored = f.client({ credentials });
  const rebound = tool({ name: "lookup", description: "Lookup", input: z.object({}), run: () => "restored" });
  await assert.rejects(restored.sessions.get(original.id, { tools: [] }), /exactly those the session placed/u);
  const restoredHost = await restored.register();
  t.after(() => restoredHost.pump.stop());
  const session = await restored.sessions.get(original.id, { tools: [rebound({ env: app })] });
  assert.deepEqual(await restored.credentials(), credentials);
  f.model = dispatch("lookup", {});
  await session.send("after reconnect");
  assert.ok(JSON.stringify(f.modelRequests.at(-1).messages).includes("restored"));
  await session.end();
});

test("cancellation reaches the tool's signal and does not execute the tool twice", { timeout: 30_000 }, async (t) => {
  const entered = deferred();
  const cancelled = deferred();
  let calls = 0;
  const wait = tool({ name: "wait", description: "Wait", input: z.object({}), run: async (_input, context) => {
    calls++;
    context.signal.addEventListener("abort", () => cancelled.resolve(), { once: true });
    entered.resolve();
    await cancelled.promise;
    return "cancelled";
  } });
  f.model = dispatch("wait", {});
  const session = await f.create(t, { tools: [wait({ env: app })] });
  const pending = session.send("wait").catch((error) => error);
  await entered.promise;
  await session.cancel();
  await cancelled.promise;
  await pending;
  assert.equal(calls, 1);
  assert.ok((await collect(session.events())).some(({ type }) => type === "turn_failed"));
});

test("one host serves tools for two sessions concurrently", { timeout: 30_000 }, async (t) => {
  const bothEntered = deferred();
  const entered = [];
  const rendezvous = tool({ name: "rendezvous", description: "Meet another invocation", input: z.object({}), run: async (_input, context) => {
    entered.push(context.sequence);
    if (entered.length === 2) bothEntered.resolve();
    await bothEntered.promise;
    return "met";
  } });
  const placed = rendezvous({ env: app });
  const first = await f.create(t, { tools: [placed] });
  const second = await f.create(t, { tools: [placed] });
  f.model = dispatch("rendezvous", {});
  await Promise.all([first.send("first"), second.send("second")]);
  assert.equal(entered.length, 2);
  await first.end();
  f.model = (_request, response) => reply(response);
  await second.send("the other session remains connected");
  assert.equal(second.state.status, "idle");
});

test("retrying session creation keeps one working registration", { timeout: 30_000 }, async (t) => {
  let calls = 0;
  const lookup = tool({ name: "lookup", description: "Lookup", input: z.object({}), run: () => { calls++; return "found"; } });
  const options = { tools: [lookup({ env: app })] };
  const operation = { idempotencyKey: "host-create-once" };
  const first = await f.create(t, options, f.brain, operation);
  const repeated = await f.brain.sessions.create(f.options(options), operation);
  assert.equal(repeated.id, first.id);
  f.model = dispatch("lookup", {});
  await repeated.send("lookup once");
  assert.equal(calls, 1);
  assert.equal((await collect(first.events())).filter(({ type }) => type === "session_creation_ended").length, 1);
});

test("replaying creation during a tool call preserves its cancellation handler", { timeout: 30_000 }, async (t) => {
  const entered = deferred();
  const cancelled = deferred();
  let calls = 0;
  const wait = tool({ name: "wait", description: "Wait", input: z.object({}), run: async (_input, context) => {
    calls++;
    context.signal.addEventListener("abort", () => cancelled.resolve(), { once: true });
    entered.resolve();
    await cancelled.promise;
    return "stopped";
  } });
  const options = { tools: [wait({ env: app })] };
  const operation = { idempotencyKey: "active-host-create-once" };
  const first = await f.create(t, options, f.brain, operation);
  f.model = dispatch("wait", {});
  const pending = first.send("wait").catch((error) => error);
  await entered.promise;
  const repeated = await f.brain.sessions.create(f.options(options), operation);
  assert.equal(repeated.id, first.id);
  await repeated.cancel();
  await cancelled.promise;
  await pending;
  assert.equal(calls, 1);
});

test("a tool may emit observations but cannot forge protected runtime events", { timeout: 30_000 }, async (t) => {
  let denied = false;
  const observer = tool({ name: "observer", description: "Emit an observation", input: z.object({}), run: async (_input, context) => {
    await assert.rejects(context.emit("turn_ended", {}), failure(400));
    denied = true;
    await context.emit("application_observation", { ready: true });
    return "observed";
  } });
  const session = await f.create(t, { tools: [observer({ env: app })] });
  f.model = dispatch("observer", {});
  await session.send("observe");
  assert.equal(denied, true);
  const events = await collect(session.events());
  assert.equal(events.filter(({ type }) => type === "turn_ended").length, 1);
  assert.deepEqual(events.find(({ type }) => type === "application_observation").data, { ready: true });
});
