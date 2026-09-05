import assert from "node:assert/strict";
import test from "node:test";
import { fixture, collect, deferred, reply, failure, text } from "./support.mjs";

const f = fixture();

test("string and structured input accumulate across suspended turns", { timeout: 30_000 }, async (t) => {
  const session = await f.create(t);
  await session.send("first");
  await session.send({ message: "second" });
  assert.deepEqual((await session.transcript()).messages.map(text), ["first", "answered", "second", "answered"]);
  assert.equal((await collect(session.events())).filter(({ type }) => type === "session_resumed").length, 2);
  await assert.rejects(session.send(""), TypeError);
  await assert.rejects(session.send({ message: 1 }), TypeError);
  assert.equal(f.modelRequests.length, 2);
});

test("resending an acknowledged message with its key never repeats the model effect", { timeout: 30_000 }, async (t) => {
  const session = await f.create(t);
  const operation = { idempotencyKey: "message-once" };
  const first = await session.send("one answer", operation);
  assert.deepEqual(await session.send({ message: "one answer" }, operation), first);
  assert.equal(f.modelRequests.length, 1);
  await assert.rejects(session.send("another answer", operation), failure(409));
  assert.equal((await session.transcript()).messages.length, 2);
});

test("cancel a running turn, inspect its outcome, and explicitly send again", { timeout: 30_000 }, async (t) => {
  const entered = deferred();
  f.model = () => { entered.resolve(); };
  const session = await f.create(t);
  const pending = session.send("wait for cancellation").then((value) => ({ value }), (error) => ({ error }));
  await entered.promise;
  assert.equal((await f.brain.sessions.get(session.id)).state.status, "running");
  await session.cancel({ idempotencyKey: "cancel-once" });
  await session.cancel({ idempotencyKey: "cancel-once" });
  await pending;
  const events = await collect(session.events());
  assert.ok(events.some(({ type }) => type === "turn_failed"));
  assert.equal(f.modelRequests.length, 1);
  f.model = (_request, response) => reply(response, "recovered");
  await session.send("try a new turn");
  assert.equal(text((await session.transcript()).messages.at(-1)), "recovered");
});

test("a client timeout does not silently retry or cancel server execution", { timeout: 30_000 }, async (t) => {
  const entered = deferred();
  const release = deferred();
  f.model = async (_request, response) => { entered.resolve(); await release.promise; reply(response); };
  const session = await f.create(t);
  const impatient = await f.client({ timeoutMs: 500 }).sessions.get(session.id);
  const pending = impatient.send("finish later").then(() => undefined, (error) => error);
  await entered.promise;
  const error = await pending;
  assert.equal(error?.name, "TimeoutError");
  assert.equal((await f.brain.sessions.get(session.id)).state.status, "running");
  const completion = (async () => {
    for await (const event of session.stream(0, AbortSignal.timeout(10_000))) if (event.type === "turn_ended") return;
    assert.fail("stream ended without turn completion");
  })();
  release.resolve();
  await completion;
  assert.equal(f.modelRequests.length, 1);
  assert.equal((await session.transcript()).messages.length, 2);
});

test("restart keeps history cold, and lost execution becomes an observation", { timeout: 30_000 }, async (t) => {
  const session = await f.create(t);
  await session.send("remember");
  const saved = await session.transcript();
  await f.stop();
  await f.start();
  const reopened = await f.client().sessions.get(session.id);
  assert.deepEqual(await reopened.transcript(), saved);
  assert.equal(f.modelRequests.length, 1);
  const entered = deferred();
  f.model = () => { entered.resolve(); };
  const pending = reopened.send("unfinished").catch((error) => error);
  await entered.promise;
  await f.stop("SIGKILL");
  await pending;
  await f.start();
  const events = await collect(reopened.events());
  assert.ok(events.some(({ type, data }) => type === "turn_failed" && JSON.stringify(data).includes("interrupted")));
  const recovered = await reopened.transcript();
  assert.deepEqual(recovered.messages.slice(0, saved.messages.length), saved.messages);
  assert.equal(text(recovered.messages.at(-1)), "unfinished");
  assert.ok(recovered.through_sequence > saved.through_sequence);
  assert.equal(f.modelRequests.length, 2);
  f.model = (_request, response) => reply(response);
  await reopened.send("continue explicitly");
  assert.ok(JSON.stringify(f.modelRequests.at(-1).messages).includes("interrupted"));
});
