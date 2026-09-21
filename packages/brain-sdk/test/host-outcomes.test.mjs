import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { HostToolRegistry } from "../dist/host.js";

const settle = () => new Promise(resolve => setImmediate(resolve));
function registry(handler, output) {
  const tools = new HostToolRegistry();
  tools.register("app", { name: "test", description: "Test outcomes.", input: z.object({}), output }, handler);
  return tools;
}
function invoke(tools, options = {}) {
  const updates = [];
  const running = tools.run({
    sessionId: "session", environment: "app", sequence: 1, name: "test", arguments: {},
    emit: async (kind, data) => { updates.push({ type: "event", kind, data }); return updates.length; },
    update: async value => { updates.push(value); return updates.length; },
    ...options,
  });
  return { updates, running };
}

test("finish with a value parses ordinary and wrapped successes", async () => {
  const output = z.object({ count: z.string().transform(Number) });
  for (const value of [{ count: "3" }, { status: "ok", value: { count: "3" } }]) {
    const { running, updates } = invoke(registry((_, call) => call.finish(value), output));
    await running;
    assert.deepEqual(updates, [{ type: "finish", outcome: { status: "ok", value: { count: 3 } } }]);
  }
  for (const value of [null, { status: "pending" }, { status: "ok", value: { status: "unknown" } }]) {
    const { running, updates } = invoke(registry((_, call) => call.finish(value)));
    await running;
    assert.deepEqual(updates[0].outcome, value?.status === "ok" ? value : { status: "ok", value });
  }
});

test("return releases dispatch and retained callbacks commit results before finish", async () => {
  let call;
  const { running, updates } = invoke(registry((_, ctx) => { call = ctx; return "sync"; }));
  let finished = false;
  void running.then(() => { finished = true; });
  await settle();
  assert.deepEqual(updates, [{ type: "returned", outcome: { status: "ok", value: "sync" } }]);
  assert.equal(finished, false);
  const first = call.emitResult("later");
  const second = call.emit("progress", { count: 2 });
  const last = call.finish("final");
  await Promise.all([first, second, last, running]);
  assert.deepEqual(updates.map(update => update.type), ["returned", "result", "event", "finish"]);
  assert.equal(finished, true);
  await assert.rejects(call.emitResult("too late"), /finished/u);
  await assert.rejects(call.finish(), /finished/u);
  assert.throws(() => call.emit("late", {}), /finished/u);
});

test("void return and finish carry no synthetic result; finish before return is sufficient", async () => {
  let call;
  const { running, updates } = invoke(registry((_, ctx) => { call = ctx; }));
  await settle();
  assert.deepEqual(updates, [{ type: "returned" }]);
  await call.finish();
  await running;
  assert.deepEqual(updates, [{ type: "returned" }, { type: "finish" }]);
  const immediate = invoke(registry(async (_, ctx) => { await ctx.finish(); return; }));
  await immediate.running;
  await settle();
  assert.deepEqual(immediate.updates, [{ type: "finish" }]);
});

test("a caught emission refusal keeps the returned Tool registered for cancellation", async () => {
  let call;
  const tools = registry(async (_, context) => {
    call = context;
    await assert.rejects(context.emit("reserved", {}), /refused/u);
  });
  const { running, updates } = invoke(tools, { emit: async () => { throw new Error("refused"); } });
  let completed = false;
  void running.then(() => { completed = true; });
  await settle();
  assert.equal(completed, false);
  tools.cancel(1);
  await running;
  assert.equal(call.signal.aborted, true);
  assert.deepEqual(updates, [{ type: "returned" }, { type: "finish", outcome: { status: "cancelled" } }]);
});

test("a lost completion acknowledgement settles the registry without replaying finish", async () => {
  let calls = 0;
  const { running } = invoke(registry((_, context) => context.finish("saved")), {
    update: async () => { calls++; throw new Error("lost acknowledgement"); },
  });
  await assert.rejects(running, /lost acknowledgement/u);
  await settle();
  assert.equal(calls, 1);
});

test("structured failures preserve details and bypass the success schema", async () => {
  for (const outcome of [
    { status: "error", error: { code: "rate_limited", message: "Try later", retryable: true, details: { retry_after_ms: 1000 } } },
    { status: "timeout" }, { status: "cancelled" },
    { status: "unknown", message: "Connection closed after the mutation" },
  ]) {
    const { running, updates } = invoke(registry(() => outcome, z.string()));
    await running;
    assert.deepEqual(updates, [{ type: "finish", outcome }]);
  }
});

test("final wrapped and transformed outputs must be JSON", async () => {
  for (const value of [
    { status: "ok" }, { status: "unknown" }, { status: "error", error: "denied" },
    { status: "error", error: { code: "bad code", message: "denied" } },
    { status: "error", error: { code: "denied", message: "denied", retryable: "yes" } },
    { status: "error", error: { code: "denied", message: "denied", details: { value: 1n } } },
    1n, new Date(), () => 1, { nested: undefined }, { value: Infinity },
  ]) {
    const { running, updates } = invoke(registry(() => value));
    await running;
    assert.equal(updates.at(-1).outcome.error.code, "invalid_output");
  }
  for (const output of [z.string().transform(() => new Date()), z.string().transform(() => 1n), z.string().transform(() => undefined)]) {
    const { running, updates } = invoke(registry((_, ctx) => ctx.finish("value"), output));
    await running;
    assert.equal(updates.at(-1).outcome.error.code, "invalid_output");
  }
});

test("expired calls validate input but never enter customer code", async () => {
  let called = 0;
  for (const [arguments_, expected] of [[{}, "timeout"], [null, "invalid_input"]]) {
    const { running, updates } = invoke(registry(() => { called++; }), { arguments: arguments_, deadline_at_ms: Date.now() - 1 });
    await running;
    const outcome = updates[0].outcome;
    assert.equal(outcome.status === "error" ? outcome.error.code : outcome.status, expected);
  }
  assert.equal(called, 0);
});

test("deadline, cancellation and loss retain the first cause despite a late handler failure", async () => {
  for (const cause of ["timeout", "cancelled", "unknown"]) {
    let signal, reject;
    const tools = registry((_, call) => {
      signal = call.signal;
      return new Promise((_, fail) => { reject = fail; });
    });
    const { running, updates } = invoke(tools, { deadline_at_ms: Date.now() + (cause === "timeout" ? 5 : 5000) });
    if (cause === "cancelled") { tools.cancel(1); tools.disconnect(1); }
    if (cause === "unknown") { tools.disconnect(1); tools.cancel(1); }
    await running;
    assert.equal(updates[0].outcome.status, cause);
    assert.equal(signal.aborted, true);
    reject(new Error("late cleanup failure"));
    await settle();
    assert.equal(updates.length, 1);
  }
});

test("unlimited and long deadlines retain their original lifetime after return", async t => {
  t.mock.timers.enable({ apis: ["Date", "setTimeout"], now: 1000 });
  let unlimited;
  const infinite = invoke(registry((_, call) => { unlimited = call; }));
  const expires = Date.now() + 2_147_483_647 + 7000;
  const finite = invoke(registry(() => {}), { deadline_at_ms: expires });
  await settle();
  t.mock.timers.tick(600_001);
  await settle();
  assert.equal(finite.updates.length, 1);
  t.mock.timers.tick(expires - Date.now() - 1);
  await settle();
  assert.equal(finite.updates.length, 1);
  t.mock.timers.tick(1);
  await finite.running;
  assert.equal(finite.updates.at(-1).outcome.status, "timeout");
  assert.equal(unlimited.deadline, undefined);
  assert.equal(unlimited.signal.aborted, false);
  await unlimited.finish();
  await infinite.running;
});
