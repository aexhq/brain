import assert from "node:assert/strict";
import test from "node:test";
import { Brain, SessionHandle } from "../dist/index.js";

test("owned turn cancellation waits for admission and sends one cancellation", async () => {
  const admitted = Promise.withResolvers();
  const finished = Promise.withResolvers();
  let controller;
  let cancellations = 0;
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async (url) => {
    if (url.endsWith("/messages")) return finished.promise;
    if (url.includes("/events?")) {
      const body = new ReadableStream({ start(value) { controller = value; admitted.resolve(); } });
      return new Response(body, { headers: { "content-type": "text/event-stream" } });
    }
    assert.ok(url.endsWith("/cancel"));
    cancellations++;
    finished.resolve(Response.json({ session_id: "child", status: "idle", last_sequence: 5 }));
    return new Response(null, { status: 204 });
  } });
  const session = new SessionHandle(client, { id: "child", status: "idle", lastSequence: 1 });
  const parent = new AbortController();
  const sending = session.send("work", { signal: parent.signal });
  parent.abort();
  await admitted.promise;
  assert.equal(cancellations, 0);
  controller.enqueue(new TextEncoder().encode(`id: 2\nevent: turn_started\ndata: ${JSON.stringify({ sequence: 2, recorded_at_ms: 1, event_type: "turn_started", data: { input: { message: "work" } } })}\n\n`));
  await sending;
  assert.equal(cancellations, 1);
  assert.equal(session.state.status, "idle");
});

test("an already cancelled owner cannot start child work", async () => {
  const client = new Brain({ baseUrl: "https://brain.example", fetch: () => { throw new Error("must not dispatch"); } });
  const session = new SessionHandle(client, { id: "child", status: "idle", lastSequence: 1 });
  await assert.rejects(session.send("work", { signal: AbortSignal.abort() }), { name: "AbortError" });
});
