import assert from "node:assert/strict";
import { test } from "node:test";
import { registryValue, waitFor } from "./npm-registry.mjs";

test("only missing registry metadata is treated as pending", () => {
  assert.equal(registryValue(() => '"sha512-ok"', "pkg@1.0.0", "dist.integrity"), "sha512-ok");
  assert.equal(registryValue(() => "", "pkg@1.0.0", "dist.integrity"), undefined);
  const failure = (code) => Object.assign(new Error(code), { stdout: JSON.stringify({ error: { code } }) });
  assert.equal(registryValue(() => { throw failure("E404"); }, "pkg@1.0.0", "dist.integrity"), undefined);
  for (const code of ["E401", "E403", "E500", "ETIMEDOUT"]) {
    assert.throws(() => registryValue(() => { throw failure(code); }, "pkg@1.0.0", "dist.integrity"), { message: code });
  }
  assert.throws(() => registryValue(() => "invalid JSON", "pkg@1.0.0", "dist.integrity"), SyntaxError);
});

function clock(t) {
  let now = 0;
  t.mock.method(Date, "now", () => now);
  t.mock.method(globalThis, "setTimeout", (callback, milliseconds) => {
    now += milliseconds;
    queueMicrotask(callback);
  });
  return () => now;
}

test("verification tolerates propagation beyond one minute", async (t) => {
  const now = clock(t);
  await waitFor(() => now() >= 125_000 ? "visible" : undefined, "visible", "archive");
  assert.equal(now(), 125_000);
});

test("permanent absence stops after five minutes with a verification-only recovery", async (t) => {
  const now = clock(t);
  await assert.rejects(waitFor(() => undefined, "visible", "archive"), /Re-run the failed verification job/);
  assert.equal(now(), 300_000);
});

test("verification does not retry integrity or authorization failures", async (t) => {
  const now = clock(t);
  await assert.rejects(waitFor(() => { throw new Error("different integrity"); }, "visible", "archive"), /different integrity/);
  assert.equal(now(), 0);
});
