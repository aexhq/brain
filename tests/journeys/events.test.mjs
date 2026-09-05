import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { tool } from "@aexhq/brain";
import { fixture, collect, callTools, reply, failure } from "./support.mjs";

const f = fixture();

test("read and resume durable events without rerunning the conversation", { timeout: 30_000 }, async (t) => {
  const session = await f.create(t);
  await session.send("first");
  const first = await collect(session.events());
  const cursor = first.at(-1).sequence;
  await session.send("second");
  const suffix = await collect(session.events(cursor));
  assert.ok(suffix.every(({ sequence }) => sequence > cursor));
  assert.equal(suffix.filter(({ type }) => type === "turn_ended").length, 1);
  assert.deepEqual(await collect(session.events(cursor)), suffix);
  assert.deepEqual(await collect(session.events(suffix.at(-1).sequence)), []);
  assert.ok(first.every(({ id, recordedAt }) => typeof id === "string" && recordedAt instanceof Date && Number.isFinite(+recordedAt)));
  assert.equal(f.modelRequests.length, 2);
});

test("stream historical events then observe a live turn and reconnect by sequence", { timeout: 30_000 }, async (t) => {
  const session = await f.create(t);
  await session.send("before subscription");
  const cursor = session.state.lastSequence;
  const replay = [];
  for await (const event of f.brain.stream(session.id, 0, AbortSignal.timeout(10_000))) {
    if (event.sequence !== undefined) replay.push(event);
    if (event.sequence === cursor) break;
  }
  assert.deepEqual(replay.map(({ sequence }) => sequence), (await collect(session.events())).map(({ sequence }) => sequence));
  const live = (async () => {
    const events = [];
    for await (const event of session.stream(cursor, AbortSignal.timeout(10_000))) {
      if (event.sequence !== undefined) events.push(event);
      if (event.type === "turn_ended") return events;
    }
    assert.fail("stream closed before completion");
  })();
  await session.send("after subscription");
  const events = await live;
  assert.ok(events.every(({ sequence }) => sequence > cursor));
  assert.equal(new Set(events.map(({ sequence }) => sequence)).size, events.length);
  assert.deepEqual(events.map(({ sequence }) => sequence), (await collect(session.events(cursor))).map(({ sequence }) => sequence));
});

test("aborting one subscription leaves other sessions and streams usable", { timeout: 30_000 }, async (t) => {
  const first = await f.create(t);
  const second = await f.create(t);
  const controller = new AbortController();
  const waiting = collect(first.stream(first.state.lastSequence, controller.signal)).catch((error) => error);
  controller.abort();
  assert.equal((await waiting).name, "AbortError");
  await second.send("independent");
  const sequence = second.state.lastSequence;
  for await (const event of second.stream(0, AbortSignal.timeout(10_000))) {
    if (event.sequence === sequence) break;
  }
  assert.equal((await collect(first.events())).filter(({ type }) => type === "turn_started").length, 0);
  await assert.rejects(collect(f.brain.withToken("wrong").stream(second.id)), failure(401));
});

test("event iteration crosses a full page without omissions or duplicate progress", { timeout: 60_000 }, async (t) => {
  let next = 0;
  const progress = tool({ name: "progress", description: "Report progress", input: z.object({}), run: async (_input, context) => {
    for (let i = 0; i < 120; i++) await context.emit("journey_progress", { i: next++ });
    return "done";
  } });
  f.model = (request, response) => request.messages.at(-1).role === "tool"
    ? reply(response) : callTools(response, [{ name: "progress", input: {} }]);
  const session = await f.create(t, { tools: [progress()] });
  for (let turn = 0; turn < 9; turn++) await session.send(`progress batch ${turn}`);
  const events = await collect(session.events());
  assert.deepEqual(events.filter(({ type }) => type === "journey_progress").map(({ data }) => data.i), Array.from({ length: 1080 }, (_, i) => i));
  assert.equal(new Set(events.map(({ sequence }) => sequence)).size, events.length);
  const cursor = events[999].sequence;
  assert.deepEqual(await collect(session.events(cursor)), events.slice(1000));
  assert.equal(f.modelRequests.length, 18);
});
