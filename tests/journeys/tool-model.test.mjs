import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { tool } from "@aexhq/brain";
import { fixture, collect, callTools, reply } from "./support.mjs";

const f = fixture();

test("default host Tools make independent model calls and present summaries through the real server", { timeout: 30_000 }, async t => {
  let privateCalls = 0;
  const summarize = tool({ name: "summarize", description: "Summarize a document", input: z.object({ document: z.string() }),
    run: async ({ document }, ctx) => {
      const response = await ctx.model({ messages: [{ role: "user", content: [{ type: "text", text: `private:${document}` }] }] });
      return ctx.finish({ document, response }, { content: `summary:${document}` });
    } });
  f.model = (request, response) => {
    if (JSON.stringify(request.input).includes("private:")) { privateCalls++; return reply(response, "independent result"); }
    const results = request.input.filter(item => item.type === "function_call_output");
    if (results.length) {
      assert.deepEqual(results.map(result => result.output), ["summary:one", "summary:two"]);
      return reply(response, "conversation answer");
    }
    return callTools(response, [{ name: "summarize", input: { document: "one" } }, { name: "summarize", input: { document: "two" } }]);
  };
  const session = await f.create(t, { tools: [summarize()] });
  await session.send("summarize both");
  assert.equal(privateCalls, 2);
  const events = await collect(session.events());
  const models = events.filter(event => event.type === "model_call_started" && event.origin?.kind === "tool");
  assert.equal(models.length, 2);
  assert.equal(new Set(models.map(event => event.origin.sequence)).size, 2);
  assert.equal(events.filter(event => event.type === "tool_result_emitted").length, 2);
  assert.ok(!JSON.stringify((await session.transcript()).messages).includes("private:"));
  const credentials = await f.brain.credentials();
  await assert.rejects(f.brain.withToken(credentials.token).request("POST", `/v1/hosts/${credentials.hostId}/model`, {
    session_id: session.id, sequence: models[0].origin.sequence, request: { messages: [{ role: "user", content: [{ type: "text", text: "late" }] }] },
  }), error => error.status === 409);
});
