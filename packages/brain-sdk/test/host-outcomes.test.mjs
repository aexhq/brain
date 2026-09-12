import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { HostToolRegistry } from "../dist/host.js";

const frame = { environment: "app", sequence: 1, name: "test", arguments: {}, deadline_ms: 5_000, emit: async () => 2 };
function registry(handler, output) {
  const tools = new HostToolRegistry();
  tools.register("app", { name: "test", description: "Test outcomes.", input: z.object({}), output }, handler);
  return tools;
}

test("ordinary values and successful Outcomes share output parsing", async () => {
  const output = z.object({ count: z.string().transform(Number) });
  for (const value of [{ count: "3" }, { status: "ok", value: { count: "3" } }]) {
    assert.deepEqual(await registry(() => value, output).run(frame), { status: "ok", value: { count: 3 } });
  }
  assert.deepEqual(await registry(() => undefined).run(frame), { status: "ok", value: null });
  assert.deepEqual(await registry(() => ({ status: "pending" })).run(frame), { status: "ok", value: { status: "pending" } });
  assert.deepEqual(await registry(() => ({ status: "ok", value: { status: "unknown" } })).run(frame), { status: "ok", value: { status: "unknown" } });
});

test("non-success Outcomes preserve structured errors and bypass the success schema", async () => {
  for (const outcome of [
    { status: "error", error: { code: "rate_limited", message: "Try later", retryable: true, details: { retry_after_ms: 1000 } } },
    { status: "timeout" },
    { status: "cancelled" },
    { status: "unknown", message: "Connection closed after the mutation" },
  ]) {
    assert.deepEqual(await registry(() => outcome, z.string()).run(frame), outcome);
  }
});

test("malformed reserved Outcomes fail output validation instead of becoming successful data", async () => {
  for (const value of [
    { status: "ok" }, { status: "unknown" }, { status: "error", error: "denied" },
    { status: "error", error: { code: "bad code", message: "denied" } },
    { status: "error", error: { code: "denied", message: "denied", retryable: "yes" } },
    { status: "error", error: { code: "denied", message: "denied", details: { value: 1n } } },
  ]) {
    const outcome = await registry(() => value).run(frame);
    assert.equal(outcome.status, "error");
    assert.equal(outcome.error.code, "invalid_output");
  }
  assert.equal((await registry(() => ({ status: "ok", value: 3 }), z.string()).run(frame)).error.code, "invalid_output");
  assert.equal((await registry(() => { throw new Error("known exception"); }).run(frame)).error.code, "tool_error");
});

test("deadline, explicit cancellation and disconnect retain the first cause despite late rejection", async () => {
  for (const cause of ["timeout", "cancelled", "unknown"]) {
    let signal;
    let reject;
    const tools = registry((_, call) => {
      signal = call.signal;
      return new Promise((_, fail) => { reject = fail; });
    });
    const running = tools.run({ ...frame, deadline_ms: cause === "timeout" ? 5 : 5_000 });
    if (cause === "cancelled") { tools.cancel(1); tools.disconnect(1); }
    if (cause === "unknown") { tools.disconnect(1); tools.cancel(1); }
    const outcome = await running;
    assert.equal(outcome.status, cause);
    assert.equal(signal.aborted, true);
    reject(new Error("late cleanup failure"));
    tools.cancel(1);
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(outcome.status, cause);
  }
});
