import assert from "node:assert/strict";
import test from "node:test";
import { lazyEnvironment } from "./lazy-environment.mjs";

test("logical setup and attach allocate nothing; concurrent calls share allocation and expiry is explicit", async () => {
  let allocations = 0;
  let now = 0;
  const env = lazyEnvironment({ now: () => now, allocate: async () => { allocations += 1; return new Map(); } });
  let sequence = 0;
  const send = (request, session = "ses_one") => env.handle({ contract: "environment/v1", binding: { environment_id: "env_one", directory_generation: 1 },
    operation: { environment_id: "env_one", session_id: session, attachment_id: "att_one", sequence: ++sequence, request } });
  assert.equal((await send({ type: "setup", configuration: { idle_ms: 100 } })).receipt.type, "accepted");
  assert.equal((await send({ type: "attach", bindings: {}, provisions: [{ manifest: { name: "echo", implementation: { type: "reference_echo" } } }] })).receipt.type, "accepted");
  assert.equal(allocations, 0);
  const invoke = (id) => send({ type: "invoke", tool: "echo", call_id: id, input: id, deadline_ms: 1000 });
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
  assert.equal((await send({ type: "invoke", tool: "echo" }, "ses_other")).receipt.code, "unavailable");
});
