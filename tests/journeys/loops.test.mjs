import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { agentloop, hostEnv, tool } from "@aexhq/brain";
import { loopEnvironment } from "../../examples/loop-environment.mjs";
import { fixture, collect, failure } from "./support.mjs";

const remote = loopEnvironment();
// A loop that dispatches a Tool through Brain before it answers, and remembers the
// routes it was handed so the test can knock on them after the turn is over.
const dispatched = [];
let callback;
const dispatching = loopEnvironment({ fetch: async (url, init) => {
  if (JSON.parse(init.body).method === "model") {
    callback = { base: url, authorization: init.headers.authorization };
    const response = await fetch(callback.base, { method: "POST", headers: init.headers,
      body: JSON.stringify({ method: "dispatch", input: [{ call_id: "remote_call", name: "lookup", environment: "app", input: { id: "7" } }] }) });
    dispatched.push(await response.json());
  }
  return fetch(url, init);
} });
async function kvEnvironment(command) {
  const op = command.operation;
  if (op.request.type !== "execute") return { contract: command.contract, sequence: op.sequence, receipt: { type: "accepted" } };
  const callback = op.request.callback;
  const call = async (method, input) => {
    assert.ok(callback.methods.includes(method));
    const response = await fetch(callback.url, { method: "POST", headers: { authorization: `Bearer ${callback.token}`, "content-type": "application/json" }, body: JSON.stringify({ method, input }) });
    assert.equal(response.status, 200);
    return response.json();
  };
  let receipt;
  if (op.request.input.input.message === "save") {
    assert.deepEqual(await call("kv_read", "removed"), {});
    const put = await call("kv_put", { key: "removed", value: null });
    assert.deepEqual(await call("kv_read", "removed"), { value: null });
    const removed = await call("kv_delete", "removed");
    assert.ok(removed > put);
    assert.equal(await call("kv_delete", "removed"), removed);
    assert.deepEqual(await call("kv_read", "removed"), {});
    await call("kv_put", { key: "kept", value: null });
    receipt = { type: "failure", code: "after_commit", message: "failed after KV commits", retryable: false };
  } else {
    assert.deepEqual(await call("kv_read", "removed"), {});
    assert.deepEqual(await call("kv_read", "kept"), { value: null });
    await call("kv_delete", "kept");
    assert.deepEqual(await call("kv_read", "kept"), {});
    receipt = { type: "result", output: { result: "recovered" } };
  }
  return { contract: command.contract, sequence: op.sequence, receipt };
}
const f = fixture({ providers: { loop: remote.handle, dispatcher: dispatching.handle, kv: kvEnvironment } });

test("an Agentloop placed in an Environment reached over HTTP runs its turn through Brain's granted execution services", { timeout: 30_000 }, async (t) => {
  const loop = agentloop({ implementation: { type: "reference_agentloop" } })({ env: f.provider("loop") });
  const session = await f.create(t, { agentloop: loop });
  await session.send("hello from afar");
  assert.equal(f.modelRequests.length, 1, "the model call went through Brain, which journaled it");
  assert.equal((await session.transcript()).messages.length, 2);
  const events = await collect(session.events());
  const kinds = events.map(({ type }) => type);
  assert.ok(kinds.includes("remote_note"), `the loop's emit reached the journal: ${kinds}`);
  assert.ok(kinds.indexOf("remote_note") < kinds.indexOf("model_call_started"));
  assert.ok(kinds.indexOf("model_call_ended") < kinds.indexOf("activation_ended"));
  assert.equal(events.at(-1).type, "turn_ended");
  assert.deepEqual(events.at(-1).data.result, { stop_reason: "end_turn" });
  await session.send("again");
  assert.equal((await collect(session.events())).filter(({ type }) => type === "remote_note").at(-1).data.turns, 2);
});

test("a Tool the remote loop dispatches runs where it was placed, and the invocation's callbacks close with the turn", { timeout: 30_000 }, async (t) => {
  const lookup = tool({ name: "lookup", description: "Lookup", input: z.object({ id: z.string() }), run: ({ id }) => ({ found: id }) });
  const session = await f.create(t, {
    agentloop: agentloop({ implementation: { type: "reference_agentloop" } })({ env: f.provider("dispatcher") }),
    tools: [lookup({ env: hostEnv({ name: "app" }) })],
  });
  await session.send("dispatch something");
  assert.equal(dispatched.length, 1);
  assert.deepEqual(dispatched[0][0], { call_id: "remote_call", output: { found: "7" }, is_error: false });
  const events = await collect(session.events());
  const started = events.find(({ type }) => type === "tool_call_started");
  assert.equal(started.data.tool, "lookup");
  assert.ok(events.some(({ type, data }) => type === "tool_call_ended" && data.sequence === started.sequence));
  assert.equal(events.at(-1).type, "turn_ended");
  const closed = await fetch(callback.base, { method: "POST",
    headers: { authorization: callback.authorization, "content-type": "application/json" },
    body: JSON.stringify({ method: "emit", input: { event_type: "late", data: {} } }) });
  assert.equal(closed.status, 404, "a finished invocation's callbacks answer nothing");
  await assert.rejects(f.brain.withToken("wrong").request("POST", new URL(callback.base).pathname, { method: "emit", input: { event_type: "late", data: {} } }), failure(404));
});

test("HTTP KV commits survive a failed turn and server restart; deletion preserves missing versus null", { timeout: 60_000 }, async (t) => {
  const session = await f.create(t, { agentloop: agentloop({ implementation: { type: "kv_test" } })({ env: f.provider("kv") }) });
  await session.send("save");
  assert.equal((await collect(session.events())).at(-1).type, "turn_failed");
  await f.stop();
  await f.start();
  await session.send("recover");
  const ended = (await collect(session.events())).at(-1);
  assert.equal(ended.type, "turn_ended");
  assert.equal(ended.data.result, "recovered");
});
