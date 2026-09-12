import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { agentloop, StructuredOutputError } from "@aexhq/brain";
import { loopEnvironment } from "../../examples/loop-environment.mjs";
import { fixture, collect, reply, deferred } from "./support.mjs";

const remote = loopEnvironment({ fetch: async (url, init) => {
  const response = await fetch(url, init);
  const { method, input } = JSON.parse(init.body);
  if (response.ok && method === "set_transcript" && input.at(-1)?.role === "assistant") {
    const message = input.at(-1).content.filter(block => block.type === "text").map(block => block.text).join("");
    const emitted = await fetch(url, { ...init, body: JSON.stringify({ method: "emit", input: {
      event_type: "output_emitted", data: { type: "assistant_message", message },
    } }) });
    assert.equal(emitted.status, 200);
  }
  return response;
} });
const f = fixture({ providers: { output: remote.handle } });
const create = t => f.create(t, { agentloop: agentloop({ implementation: { type: "reference_agentloop" } })({ env: f.provider("output") }) });

test("prompt output corrects twice, replays keyed turns, and changes schemas on the same session", { timeout: 30_000 }, async t => {
  const answers = ["not JSON", '{"age":"wrong"}', '{"age":37}', '["Ada"]'];
  f.model = (request, response) => {
    assert.equal(request.text?.format, undefined);
    reply(response, answers.shift());
  };
  const session = await create(t);
  const options = { output: { type: z.object({ age: z.number() }) }, idempotencyKey: "structured-once" };
  assert.deepEqual(await session.send("Extract Ada's age", options), { age: 37 });
  assert.equal(f.modelRequests.length, 3);
  assert.match(JSON.stringify(f.modelRequests[1].input), /Validation feedback/);
  assert.deepEqual(await session.send("Extract Ada's age", options), { age: 37 });
  assert.equal(f.modelRequests.length, 3, "idempotent correction turns do not rerun inference");
  assert.deepEqual(await session.send("List names", { output: { type: z.array(z.string()) } }), ["Ada"]);
  const reopened = await f.brain.sessions.get(session.id);
  const events = await collect(reopened.events());
  assert.equal(events.filter(e => e.type === "turn_ended").length, 4);
  assert.equal(events.filter(e => e.type === "output_emitted").length, 4);
  assert.equal(answers.length, 0);
});

test("exhaustion is local while completed turns remain durable", { timeout: 30_000 }, async t => {
  f.model = (_, response) => reply(response, "{}");
  const session = await create(t);
  await assert.rejects(session.send("Extract", { output: { type: z.object({ name: z.string() }) } }), error => {
    assert.ok(error instanceof StructuredOutputError);
    assert.equal(error.attempts, 3);
    return true;
  });
  assert.equal(f.modelRequests.length, 3);
  const events = await collect(session.events());
  assert.equal(events.filter(e => e.type === "turn_ended").length, 3);
  assert.equal(events.filter(e => e.type === "turn_failed").length, 0);
});

test("cancellation of a correction stops the sequence without another model call", { timeout: 30_000 }, async t => {
  const correcting = deferred();
  f.model = (_, response) => {
    if (f.modelRequests.length === 1) reply(response, "not JSON");
    else correcting.resolve();
  };
  const session = await create(t);
  const controller = new AbortController();
  const pending = session.send("Extract", { output: { type: z.string() }, signal: controller.signal }).catch(error => error);
  await Promise.race([correcting.promise, pending.then(error => { throw error; })]);
  controller.abort();
  assert.ok(await pending instanceof Error);
  assert.equal(f.modelRequests.length, 2);
  assert.ok((await collect(session.events())).some(e => e.type === "turn_failed"));
});

test("a loop without assistant output fails clearly without correction turns", { timeout: 30_000 }, async t => {
  const session = await f.create(t);
  await assert.rejects(session.send("Extract", { output: { type: z.string() } }), /requires a completed turn/);
  assert.equal(f.modelRequests.length, 1);
});
