import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { hostEnv, tool, inspectTool } from "@aexhq/brain";
import { fixture, collect, callTools, reply, deferred, failure } from "./support.mjs";
import { lookupRecord } from "../../examples/tool-outcomes.mjs";

const f = fixture();

test("cancelling a parent reaches an owned child turn through the host Tool signal", { timeout: 30_000 }, async (t) => {
  const entered = deferred();
  const finished = deferred();
  const child = await f.create(t);
  const unrelated = await f.create(t);
  const delegate = tool({ name: "delegate", description: "Run child", input: z.object({}), run: async (_, context) => {
    try { return await child.send("child task", { signal: context.signal }); }
    finally { finished.resolve(); }
  } });
  f.model = (request, response) => {
    if (JSON.stringify(request.messages).includes("child task")) { entered.resolve(); return; }
    callTools(response, [{ name: "delegate", input: {} }]);
  };
  const parent = await f.create(t, { tools: [delegate({ env: hostEnv({ name: "owner" }) })] });
  const running = parent.send("delegate work");
  await entered.promise;
  await parent.cancel();
  await running;
  await finished.promise;
  assert.equal((await f.brain.sessions.get(child.id)).state.status, "idle");
  assert.equal((await f.brain.sessions.get(unrelated.id)).state.status, "idle");
  const events = await collect(child.events());
  assert.equal(events.filter((event) => event.type === "model_call_started").length, 1);
  assert.ok(events.some((event) => event.type === "model_call_failed" && event.data.ambiguous));
});
const app = hostEnv({ name: "app" });
const dispatch = (name, input) => (request, response) => request.messages.at(-1).role === "tool"
  ? reply(response) : callTools(response, [{ name, input }]);

test("the Tool outcome example returns structured failure despite its successful output schema", { timeout: 30_000 }, async t => {
  const session = await f.create(t, { tools: [lookupRecord({ env: app })] });
  f.model = dispatch("lookup_record", { id: "missing" });
  await session.send("find the missing record");
  const events = await collect(session.events());
  const result = events.find(event => event.type === "tool_call_ended").data.result;
  assert.equal(result.is_error, true);
  assert.deepEqual(result.output, { code: "not_found", message: "Record not found", retryable: false, details: { id: "missing" } });
  assert.deepEqual(JSON.parse(f.modelRequests.at(-1).messages.at(-1).content), result.output);
  f.model = dispatch("lookup_record", { id: "1" });
  await session.send("find Ada");
  assert.match(f.modelRequests.at(-1).messages.at(-1).content, /Ada/u);
});

for (const outcome of [{ status: "timeout" }, { status: "cancelled" }, { status: "unknown", message: "Remote result lost" }]) {
  test(`a direct host ${outcome.status} reaches the journal and model as one failed result`, { timeout: 30_000 }, async t => {
    let calls = 0;
    const remote = tool({ name: "remote", description: "Read a remote result", input: z.object({}), output: z.string(), run: () => { calls++; return outcome; } });
    const session = await f.create(t, { tools: [remote({ env: app })] });
    f.model = dispatch("remote", {});
    await session.send("read once");
    const events = await collect(session.events());
    const ended = events.filter(event => event.type === "tool_call_ended");
    assert.equal(ended.length, 1);
    assert.equal(ended[0].data.result.is_error, true);
    assert.equal(ended[0].data.result.output.code, outcome.status);
    assert.equal(events.some(event => event.type === "environment_unreachable"), false);
    assert.equal(calls, 1);
    assert.match(f.modelRequests.at(-1).messages.at(-1).content, new RegExp(outcome.status));
  });
}

test("graceful shutdown lets a host Tool finish and saves the completed turn", { timeout: 30_000 }, async (t) => {
  const entered = deferred();
  const finish = deferred();
  const wait = tool({ name: "wait", description: "Wait", input: z.object({}), run: async (_, context) => {
    entered.resolve();
    await finish.promise;
    await context.emit("finished_during_drain", {});
    return "completed";
  } });
  const session = await f.create(t, { tools: [wait({ env: app })] });
  f.model = dispatch("wait", {});
  const running = session.send("finish before shutdown");
  await entered.promise;
  const stopped = f.stop();
  const admitted = [];
  for (;;) {
    try { admitted.push(await f.brain.sessions.create(f.options())); }
    catch (error) { assert.ok(failure(503)(error)); break; }
  }
  finish.resolve();
  await running;
  await stopped;
  await f.start();
  for (const extra of admitted) { await extra.end(); await extra.delete(); }
  const events = await collect(session.events());
  assert.ok(events.some((event) => event.type === "finished_during_drain"));
  assert.equal(events.at(-1).type, "turn_ended");
  assert.equal((await session.transcript()).messages.at(-1).content[0].text, "answered");
});

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
  const progress = events.find(({ type }) => type === "lookup_progress");
  assert.deepEqual(progress.origin, { kind: "tool", sequence: started.sequence });
  const replay = session.stream(progress.sequence - 1);
  const streamed = (await replay.next()).value;
  assert.deepEqual(streamed.origin, progress.origin);
  assert.deepEqual(streamed.data, progress.data);
  await replay.return();
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
  const events = await collect(session.events());
  assert.ok(events.some(({ type }) => type === "turn_failed"));
  const result = events.find(event => event.type === "tool_call_ended").data.result;
  assert.equal(result.is_error, true);
  assert.equal(result.output.code, "cancelled");
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
