import assert from "node:assert/strict";
import test from "node:test";
import { loopEnvironment } from "./loop-environment.mjs";

test("a turn outside Brain calls back through its token and returns what the loop kept", async () => {
  const calls = [];
  const env = loopEnvironment({ fetch: async (url, init) => {
    calls.push({ url, authorization: init.headers.authorization, body: JSON.parse(init.body) });
    if (url.endsWith("/emit")) return Response.json({ sequence: 5 });
    if (url.endsWith("/model")) {
      return Response.json({ message: { role: "assistant", content: [{ type: "text", text: "hello back" }] }, stop_reason: "end_turn", usage: {} });
    }
    return new Response("no such route", { status: 404 });
  } });
  const command = (request) => env.handle({ contract: "environment/v1", operation: { sequence: 3, environment: "loop", session_id: "ses_one", request } });
  assert.equal((await command({ type: "setup", configuration: {}, needs: ["https://models.example.com"] })).receipt.type, "accepted");
  const turned = await command({
    type: "turn",
    id: "a".repeat(64),
    needs: [],
    input: { input: { message: "hello" }, transcript: [], kv: {}, events: [], configuration: {}, system: "", tools: [], runtime: { logical_time_ms: 3, deterministic_seed: [] } },
    callback: { url: "http://brain.example/v1/sessions/ses_one/turns/3", token: "turn-token" },
  });
  assert.equal(turned.receipt.type, "turned");
  assert.equal(turned.receipt.output.transcript.length, 2);
  assert.deepEqual(turned.receipt.output.kv, { turns: 1 });
  assert.deepEqual(calls.map(({ url }) => url), [
    "http://brain.example/v1/sessions/ses_one/turns/3/emit",
    "http://brain.example/v1/sessions/ses_one/turns/3/model",
  ]);
  assert.ok(calls.every(({ authorization }) => authorization === "Bearer turn-token"));
  assert.equal(calls[1].body.messages.length, 1);
  const refused = await command({ type: "setup", configuration: {}, needs: ["file:///workspace"] });
  assert.equal(refused.receipt.code, "unmet_need");
  assert.equal((await command({ type: "invoke", tool: "x", needs: [], input: {}, deadline_ms: 1 })).receipt.code, "unsupported");
});
