import assert from "node:assert/strict";
import test from "node:test";

import { Transport, parseEventStream } from "../dist/transport.js";
import { MAX_PUBLIC_EVENT_BYTES } from "../dist/limits.js";

function eventPayload(bytes) {
  const empty = JSON.stringify({ type: "test", value: "" });
  return JSON.stringify({ type: "test", value: "x".repeat(bytes - empty.length) });
}

function eventStream(body) {
  return new Response(body).body;
}

test("transport serializes an idempotent request exactly once across retry", async () => {
  let serializations = 0;
  const bodies = [];
  let calls = 0;
  const transport = new Transport("token", "https://brain.invalid", async (_input, init) => {
    calls += 1;
    bodies.push(init.body);
    if (calls === 1) throw new Error("response lost");
    return new Response(JSON.stringify({ ok: true }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  });
  const mutable = {
    toJSON() {
      serializations += 1;
      return { serialization: serializations };
    },
  };
  assert.deepEqual(
    await transport.json("POST", "/v1/example", { body: mutable, retry: true }),
    { ok: true },
  );
  assert.equal(serializations, 1);
  assert.equal(bodies.length, 2);
  assert.equal(bodies[0], bodies[1]);
});

test("provisional events never advance the durable reconnect cursor", async () => {
  const requests = [];
  const frames = [
    [
      { type: "session.created", seq: 1, session_id: "ses_test", at: "2026-01-01T00:00:00Z" },
      {
        type: "assistant.delta", seq: 999, session_id: "ses_test", turn_id: "turn_test",
        agent_id: "root", attempt_id: "att_12345678901234567890", provisional: true,
        delta: "partial A", at: "2026-01-01T00:00:01Z",
      },
    ],
    [
      {
        type: "model.attempt_superseded", seq: 2, session_id: "ses_test", turn_id: "turn_test",
        logical_operation_id: "model:turn_test:1", superseded_attempt_id: "att_12345678901234567890",
        replacement_attempt_id: "att_09876543210987654321", reason: "unknown",
        at: "2026-01-01T00:00:02Z",
      },
      {
        type: "assistant.message", seq: 3, session_id: "ses_test", turn_id: "turn_test",
        agent_id: "root", attempt_id: "att_09876543210987654321", text: "canonical B",
        at: "2026-01-01T00:00:03Z",
      },
    ],
  ];
  const transport = new Transport("token", "https://brain.invalid", async (input, init) => {
    requests.push({ url: String(input), headers: init.headers });
    const events = frames.shift() ?? [];
    const body = events.map((event) => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`).join("");
    return new Response(body, { status: 200, headers: { "content-type": "text/event-stream" } });
  });
  const received = [];
  for await (const event of transport.events("ses_test")) {
    received.push(event);
    if (received.length === 4) break;
  }
  assert.equal(new URL(requests[1].url).searchParams.get("after"), "1");
  assert.equal(requests[1].headers["Last-Event-ID"], "1");
  assert.deepEqual(received.map((event) => event.type), [
    "session.created",
    "assistant.delta",
    "model.attempt_superseded",
    "assistant.message",
  ]);
});

test("SSE payload bound excludes framing and rejects exact + 1", async () => {
  const exact = eventPayload(MAX_PUBLIC_EVENT_BYTES);
  assert.equal(new TextEncoder().encode(exact).byteLength, MAX_PUBLIC_EVENT_BYTES);
  const events = [];
  for await (const event of parseEventStream(eventStream(`event: test\nid: 1\ndata: ${exact}\n\n`))) {
    events.push(event);
  }
  assert.equal(events.length, 1);

  const over = eventPayload(MAX_PUBLIC_EVENT_BYTES + 1);
  await assert.rejects(async () => {
    for await (const _event of parseEventStream(eventStream(`data: ${over}\n\n`))) {
      // The parser must reject before yielding the oversized payload.
    }
  }, /payload exceeds/);
});

test("SSE requires a blank-delimited terminal frame", async () => {
  await assert.rejects(async () => {
    for await (const _event of parseEventStream(eventStream('data: {"type":"test"}'))) {
      // A response EOF is not a frame delimiter.
    }
  }, /truncated frame/);
});
