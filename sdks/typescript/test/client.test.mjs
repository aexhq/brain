import assert from "node:assert/strict";
import { test } from "node:test";
import { Brain, BrainError } from "../dist/index.js";

test("an unsupported callback transport fails before registration", async () => {
  const brain = new Brain("http://brain.test", "token", async () => { assert.fail("invalid transport made a request"); });
  await assert.rejects(brain.create({
    harness: "pi", config: { model: "test/model", tools: [] }, runtime_transport: "invalid",
    runtime_tools: [{ name: "read", description: "Read", input_schema: { type: "object" }, run: () => null }],
  }), /unsupported runtime transport/);
});

test("unsafe requests are never retried and errors preserve identity", async () => {
  let count = 0;
  const client = new Brain("http://brain.test", "token", async () => {
    count++;
    return Response.json({ error: { code: "overloaded", message: "busy" }, request_id: "request" }, { status: 429 });
  });
  await assert.rejects(client.send("session", { type: "user.message", text: "hello" }), e => e instanceof BrainError && e.code === "overloaded" && e.requestId === "request");
  assert.equal(count, 1);
});

test("SSE handles split UTF-8 and comments without treating keepalives as events", async () => {
  const payload = new TextEncoder().encode(': keepalive\n\nevent: assistant.message\ndata: {"id":"one","type":"assistant.message","data":{"text":"你好"}}\n\n');
  const client = new Brain("http://brain.test", "token", async () => new Response(new ReadableStream({ start(controller) { for (const byte of payload) controller.enqueue(Uint8Array.of(byte)); controller.close(); } })));
  const events = []; for await (const event of client.observe("session")) events.push(event);
  assert.equal(events.length, 1); assert.equal(events[0].data.text, "你好");
});
