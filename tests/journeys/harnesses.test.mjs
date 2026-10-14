import assert from "node:assert/strict";
import test from "node:test";
import { fixture, collect, failure } from "./support.mjs";

const f = fixture();

test("a registered model harness journals model results and resumes native state after restart", { timeout: 60_000 }, async t => {
  const session = await f.create(t);
  await session.send("hello");
  const first = await collect(session.events());
  const kinds = first.map(event => event.type);
  assert(kinds.indexOf("model_call_ended") < kinds.indexOf("activation_ended"));
  assert.equal(first.at(-1).type, "turn_ended");
  assert.deepEqual((await session.transcript()).messages.map(message => message.role), ["user", "assistant"]);
  await f.stop(); await f.start();
  assert.equal(f.modelRequests.length, 1, "recovery does not start the agent");
  await session.send("again");
  assert.equal(f.modelRequests.length, 2);
  assert(f.modelRequests[1].input.some(item => item.content === "answered"));
});

test("a custom startup harness retains KV across activation and restart", { timeout: 60_000 }, async t => {
  const session = await f.create(t, { agentloop: { name: "fixture", configuration: { mode: "diagnostic" } } });
  await session.send("kv");
  await f.stop(); await f.start();
  await session.send("again");
  const events = await collect(session.events());
  assert.deepEqual(events.filter(event => event.type === "turn_ended").map(event => event.data.result), [
    { turns: 1, message: "kv" }, { turns: 2, message: "again" },
  ]);
  assert.equal(f.modelRequests.length, 0);
});

test("an interrupted native activation preserves commits and cannot replay on restart", { timeout: 60_000 }, async t => {
  const session = await f.create(t, { agentloop: { name: "fixture", configuration: { mode: "kv-fail" } } });
  await session.send("save");
  assert.equal((await collect(session.events())).at(-1).type, "turn_failed");
  await f.stop(); await f.start();
  await session.send("recover");
  const events = await collect(session.events());
  assert.equal(events.at(-1).type, "turn_failed");
  assert.match(JSON.stringify(events.at(-1).data), /native harness activation did not complete/);
  assert.equal(events.filter(event => event.type === "note").length, 1, "the failed activation never runs again");
  assert.deepEqual(events.find(event => event.type === "kv_set" && event.data.key === "memory").data.value, { turns: 1 });
});

test("session clients cannot install executable agentloops", { timeout: 30_000 }, async () => {
  const body = { model: { provider: "openai", name: "test", api_key: "fixture" }, tools: [], environments: [] };
  for (const extra of [{ agentloop: { implementation: {} } }, { agentloop: { name: "fixture", command: "node", args: ["user.mjs"] } }]) {
    await assert.rejects(f.brain.request("POST", "/v1/sessions", { ...body, ...extra }, crypto.randomUUID()), failure(422));
  }
  await assert.rejects(f.brain.sessions.create({ model: { provider: "openai", name: "test", apiKey: "fixture" }, agentloop: { name: "not-installed" } }), /not registered/u);
  await assert.rejects(f.brain.request("POST", "/v1/agentloops", {}, crypto.randomUUID()), failure(404));
});
