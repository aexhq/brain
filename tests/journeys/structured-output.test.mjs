import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { StructuredOutputError } from "@aexhq/brain";
import { fixture, collect, reply, deferred } from "./support.mjs";

const f = fixture();

test("prompt output corrects twice, replays keyed turns, and changes schemas on the same session", { timeout: 30_000 }, async t => {
  const answers = ["not JSON", '{"age":"wrong"}', '{"age":37}', '["Ada"]'];
  f.model = (request, response) => {
    assert.equal(request.response_format, undefined);
    reply(response, answers.shift());
  };
  const session = await f.create(t);
  const options = { output: { type: z.object({ age: z.number() }) }, idempotencyKey: "structured-once" };
  assert.deepEqual(await session.send("Extract Ada's age", options), { age: 37 });
  assert.equal(f.modelRequests.length, 3);
  assert.match(JSON.stringify(f.modelRequests[1].messages), /Validation feedback/);
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
  const session = await f.create(t);
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
  const session = await f.create(t);
  const controller = new AbortController();
  const pending = session.send("Extract", { output: { type: z.string() }, signal: controller.signal }).catch(error => error);
  await correcting.promise;
  controller.abort();
  assert.ok(await pending instanceof Error);
  assert.equal(f.modelRequests.length, 2);
  assert.ok((await collect(session.events())).some(e => e.type === "turn_failed"));
});
