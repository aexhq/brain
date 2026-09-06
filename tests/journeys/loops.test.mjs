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
  if (url.endsWith("/model")) {
    callback = { base: url.slice(0, -"/model".length), authorization: init.headers.authorization };
    const response = await fetch(`${callback.base}/dispatch`, { method: "POST", headers: init.headers,
      body: JSON.stringify({ calls: [{ call_id: "remote_call", name: "lookup", input: { id: "7" } }] }) });
    dispatched.push(await response.json());
  }
  return fetch(url, init);
} });
const f = fixture({ providers: { loop: remote.handle, dispatcher: dispatching.handle } });

test("an Agentloop placed in an Environment reached over HTTP runs its turn through Brain's turn routes", { timeout: 30_000 }, async (t) => {
  const loop = agentloop({ implementation: f.reference, needs: ["https://models.example.com"] })({ env: f.provider("loop") });
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

test("a Tool the remote loop dispatches runs where it was placed, and the turn's routes close with the turn", { timeout: 30_000 }, async (t) => {
  const lookup = tool({ name: "lookup", description: "Lookup", input: z.object({ id: z.string() }), run: ({ id }) => ({ found: id }) });
  const session = await f.create(t, {
    agentloop: agentloop({ implementation: f.reference })({ env: f.provider("dispatcher") }),
    tools: [lookup({ env: hostEnv({ name: "app" }) })],
  });
  await session.send("dispatch something");
  assert.equal(dispatched.length, 1);
  assert.deepEqual(dispatched[0].results[0], { call_id: "remote_call", output: { found: "7" }, is_error: false });
  const events = await collect(session.events());
  const started = events.find(({ type }) => type === "tool_call_started");
  assert.equal(started.data.tool, "lookup");
  assert.ok(events.some(({ type, data }) => type === "tool_call_ended" && data.sequence === started.sequence));
  assert.equal(events.at(-1).type, "turn_ended");
  const closed = await fetch(`${callback.base}/emit`, { method: "POST",
    headers: { authorization: callback.authorization, "content-type": "application/json" },
    body: JSON.stringify({ event_type: "late", data: {} }) });
  assert.equal(closed.status, 404, "a finished turn's routes answer nothing");
  await assert.rejects(f.brain.withToken("wrong").request("POST", `${new URL(callback.base).pathname}/emit`, { event_type: "late", data: {} }), failure(404));
});
