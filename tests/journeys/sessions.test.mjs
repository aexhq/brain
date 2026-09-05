import assert from "node:assert/strict";
import test from "node:test";
import { fixture, collect, failure, text } from "./support.mjs";

const f = fixture();

test("create, list, reopen, end, and delete a conversation", { timeout: 30_000 }, async (t) => {
  const session = await f.create(t);
  assert.equal(session.state.status, "idle");
  assert.ok((await f.brain.sessions.list()).some(({ id }) => id === session.id));
  const reopened = await f.client().sessions.get(session.id);
  assert.deepEqual(reopened.state, session.state);
  await assert.rejects(session.delete(), failure(400));
  await reopened.send("hello");
  assert.equal((await reopened.end()).status, "ended");
  assert.equal((await session.transcript()).messages.length, 2);
  await assert.rejects(reopened.send("too late"), failure(400));
  await reopened.delete();
  assert.ok(!(await f.brain.sessions.list()).some(({ id }) => id === session.id));
  await assert.rejects(f.brain.sessions.get(session.id), failure(404));
});

test("cancel an idle session and repeat deletion using the same operation key", { timeout: 30_000 }, async (t) => {
  const session = await f.create(t);
  await session.cancel();
  assert.equal((await f.brain.sessions.get(session.id)).state.status, "idle");
  await session.send("still usable");
  await session.end();
  const operation = { idempotencyKey: "delete-once" };
  await session.delete(operation);
  await session.delete(operation);
  await assert.rejects(session.transcript(), failure(404));
});

test("initial transcript, system prompt, and response format reach the model", { timeout: 30_000 }, async (t) => {
  const transcript = [{ role: "user", content: [{ type: "text", text: "earlier context" }] }];
  const responseFormat = { type: "json_object" };
  const session = await f.create(t, { transcript, system: "Answer briefly", responseFormat, idleTtlMs: 0 });
  assert.deepEqual((await session.transcript()).messages, transcript);
  await session.send({ message: "next question" });
  const request = f.modelRequests.at(-1);
  assert.ok(JSON.stringify(request.messages).includes("Answer briefly"));
  assert.ok(JSON.stringify(request.messages).includes("earlier context"));
  assert.deepEqual(request.response_format, responseFormat);
  assert.equal(text((await session.transcript()).messages.at(-2)), "next question");
});

test("retrying creation with the same key returns one session", { timeout: 30_000 }, async (t) => {
  const operation = { idempotencyKey: "create-once" };
  const first = await f.create(t, {}, f.brain, operation);
  const second = await f.brain.sessions.create(f.options(), operation);
  assert.equal(second.id, first.id);
  assert.equal((await f.brain.sessions.list()).filter(({ id }) => id === first.id).length, 1);
  await assert.rejects(f.brain.sessions.create(f.options({ system: "different" }), operation), failure(409));
});

test("end is repeatable with a key and preserves the recorded conversation", { timeout: 30_000 }, async (t) => {
  const session = await f.create(t);
  await session.send("keep this");
  const before = await session.transcript();
  const operation = { idempotencyKey: "end-once" };
  assert.deepEqual(await session.end(operation), await session.end(operation));
  assert.deepEqual((await session.transcript()).messages, before.messages);
  assert.ok((await session.transcript()).through_sequence >= before.through_sequence);
  const ended = (await collect(session.events())).filter(({ type }) => type === "session_ended");
  assert.equal(ended.length, 1);
});

test("parallel conversations preserve separate transcripts and independent deletion", { timeout: 30_000 }, async (t) => {
  const sessions = await Promise.all([f.create(t), f.create(t)]);
  await Promise.all(sessions.map((session, i) => session.send(`conversation ${i}`)));
  for (const [i, session] of sessions.entries()) assert.equal(text((await session.transcript()).messages[0]), `conversation ${i}`);
  await sessions[0].end();
  await sessions[0].delete();
  await sessions[1].send("still available");
  assert.equal((await sessions[1].transcript()).messages.length, 4);
});
