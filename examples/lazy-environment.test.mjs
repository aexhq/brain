import assert from "node:assert/strict";
import test from "node:test";
import { lazyEnvironment } from "./lazy-environment.mjs";

test("setup allocates nothing; concurrent calls share allocation and expiry is explicit", async () => {
  let allocations = 0;
  let now = 0;
  const env = lazyEnvironment({ now: () => now, allocate: async () => { allocations += 1; return new Map(); } });
  let sequence = 0;
  const send = (request, session = "ses_one") => env.handle({ contract: "environment/v1",
    operation: { environment: "east", session_id: session, sequence: ++sequence, request } });
  assert.equal((await send({ type: "setup", configuration: { idle_ms: 100 }, needs: ["file:///workspace?access=write"] })).receipt.type, "accepted");
  assert.equal(allocations, 0);
  const invoke = (input) => send({ type: "invoke", tool: "echo", implementation: { type: "reference_echo" }, needs: [], input, deadline_ms: 1000 });
  const outcomes = await Promise.all([invoke("one"), invoke("two")]);
  assert(outcomes.every(({ receipt }) => receipt.type === "outcome"));
  assert(outcomes.every(({ receipt }) => receipt.outcome.status === "ok"));
  assert.equal(allocations, 1);
  now = 101;
  env.expire();
  assert.equal((await invoke("three")).receipt.code, "expired");
  assert.equal(allocations, 1);
  assert.equal((await send({ type: "call", name: "restart", input: {} })).receipt.output.restored, false);
  assert.equal((await invoke("four")).receipt.outcome.value.entries, 1);
  assert.equal(allocations, 2);
  assert.equal((await send({ type: "invoke", implementation: { type: "reference_echo" }, needs: [], input: {}, deadline_ms: 1 }, "ses_other")).receipt.code, "unavailable");
  const refused = await send({ type: "setup", configuration: {}, needs: ["pkg:apt/ffmpeg"] }, "ses_other");
  assert.equal(refused.receipt.code, "unmet_need");
  assert.match(refused.receipt.message, /pkg:apt\/ffmpeg/u);
});
