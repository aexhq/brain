import assert from "node:assert/strict";
import test from "node:test";
import { lazyEnvironment } from "./lazy-environment.mjs";

test("setup allocates nothing; concurrent calls share allocation and teardown is caller controlled", async () => {
  let allocations = 0;
  const env = lazyEnvironment({ allocate: async () => { allocations += 1; return new Map(); } });
  let sequence = 0;
  const send = (request, session = "ses_one") => env.handle({ contract: "environment/v1",
    operation: { environment: "east", session_id: session, sequence: ++sequence, request } });
  assert.equal((await send({ type: "setup", configuration: {}, needs: ["file:///workspace?access=write"] })).receipt.type, "accepted");
  assert.equal(allocations, 0);
  const invoke = (input) => send({ type: "execute", implementation: { type: "reference_echo" }, needs: [], input, deadline_ms: 1000 });
  const outcomes = await Promise.all([invoke("one"), invoke("two")]);
  assert(outcomes.every(({ receipt }) => receipt.type === "result"));
  assert.equal(allocations, 1);
  await send({ type: "detach" });
  assert.equal((await invoke("three")).receipt.output.entries, 3);
  assert.equal(allocations, 1);
  assert.equal((await send({ type: "call", name: "restart", input: {} })).receipt.output.restored, false);
  assert.equal((await invoke("four")).receipt.output.entries, 1);
  assert.equal(allocations, 2);
  await send({ type: "teardown" });
  assert.equal((await invoke("after teardown")).receipt.code, "unavailable");
  assert.equal((await send({ type: "execute", implementation: { type: "reference_echo" }, needs: [], input: {}, deadline_ms: 1 }, "ses_other")).receipt.code, "unavailable");
  const refused = await send({ type: "setup", configuration: {}, needs: ["pkg:apt/ffmpeg"] }, "ses_other");
  assert.equal(refused.receipt.code, "unmet_need");
  assert.match(refused.receipt.message, /pkg:apt\/ffmpeg/u);
});
