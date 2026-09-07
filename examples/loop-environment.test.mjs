import assert from "node:assert/strict";
import test from "node:test";
import { loopEnvironment } from "./loop-environment.mjs";

test("a turn outside Brain calls back through its token and returns what the loop kept", async () => {
  const calls = [];
  const env = loopEnvironment({ fetch: async (url, init) => {
    calls.push({ url, authorization: init.headers.authorization, body: JSON.parse(init.body) });
    if (["set_transcript", "set_kv"].includes(JSON.parse(init.body).method)) return Response.json(4);
    if (JSON.parse(init.body).method === "emit") return Response.json({ sequence: 5 });
    if (JSON.parse(init.body).method === "model") {
      return Response.json({ message: { role: "assistant", content: [{ type: "text", text: "hello back" }] }, stop_reason: "end_turn", usage: {} });
    }
    return new Response("no such route", { status: 404 });
  } });
  const command = (request) => env.handle({ contract: "environment/v1", operation: { sequence: 3, environment: "loop", session_id: "ses_one", request } });
  assert.equal((await command({ type: "setup", configuration: {}, needs: ["https://models.example.com"] })).receipt.type, "accepted");
  const turned = await command({
    type: "execute",
    implementation: { type: "reference_agentloop" },
    needs: [],
    input: { input: { message: "hello" }, transcript: [], kv: {}, events: [], configuration: {}, system: "", tools: [], runtime: { logical_time_ms: 3, deterministic_seed: [] } },
    callback: { url: "http://brain.example/v1/sessions/ses_one/executions/3/call", token: "execution-token", methods: ["model", "emit", "set_transcript", "set_kv"] },
  });
  assert.equal(turned.receipt.type, "result");
  assert.equal(calls.filter(({ body }) => body.method === "set_transcript").at(-1).body.input.length, 2);
  assert.deepEqual(calls.find(({ body }) => body.method === "set_kv").body.input, { key: "turns", value: 1 });
  assert.ok(calls.every(({ url }) => url === "http://brain.example/v1/sessions/ses_one/executions/3/call"));
  assert.ok(calls.every(({ authorization }) => authorization === "Bearer execution-token"));
  assert.equal(calls.find(({ body }) => body.method === "model").body.input.messages.length, 1);
  const refused = await command({ type: "setup", configuration: {}, needs: ["file:///workspace"] });
  assert.equal(refused.receipt.code, "unmet_need");
  assert.equal((await command({ type: "execute", implementation: { type: "unknown" }, needs: [], input: {}, deadline_ms: 1 })).receipt.code, "unsupported");
});
