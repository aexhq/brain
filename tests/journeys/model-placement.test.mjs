import assert from "node:assert/strict";
import test from "node:test";
import { agentloop, hostEnv, tool } from "@aexhq/brain";
import { z } from "zod";
import { fixture, callTools, collect } from "./support.mjs";

const f = fixture({ providers: { selector: async ({ operation }) => {
  const { request, sequence } = operation;
  let receipt = { type: "accepted" };
  if (request.type === "execute") {
    const callback = request.callback;
    const call = async (method, input) => {
      const response = await fetch(callback.url, { method: "POST", headers: {
        authorization: `Bearer ${callback.token}`, "content-type": "application/json",
      }, body: JSON.stringify({ method, input }) });
      assert.equal(response.status, 200);
      return response.json();
    };
    const [definition] = request.input.tools;
    const transcript = [{ role: "user", content: [{ type: "text", text: "choose a placement" }] }];
    const model = await call("model", { messages: transcript, tools: [{
      name: definition.name, description: definition.description,
      input_schema: { type: "object", properties: {
        environment: { type: "string", enum: definition.environments }, input: definition.input_schema,
      }, required: ["environment", "input"], additionalProperties: false },
    }] });
    transcript.push(model.message);
    const results = await call("dispatch", model.message.content.filter((block) => block.type === "tool_use").map((block) => ({
      call_id: block.id, name: block.name, environment: block.input.environment, input: block.input.input,
    })));
    transcript.push({ role: "user", content: results.map((result) => ({ type: "tool_result", tool_use_id: result.call_id, content: result.output, is_error: result.is_error })) });
    receipt = { type: "result", output: { transcript, kv: {}, result: results[0].output } };
  }
  return { contract: "environment/v1", sequence, receipt };
} } });

test("model-visible selection dispatches one canonical Tool to the chosen authorized Environment", { timeout: 30_000 }, async (t) => {
  const called = [];
  const lookup = tool({ name: "lookup", description: "Lookup", input: z.object({}),
    output: z.object({ where: z.string() }), options: z.object({ where: z.string() }),
    run: (_, context) => { called.push(context.options.where); return { where: context.options.where }; },
  });
  f.model = (_request, response) => callTools(response, [{ name: "lookup", input: { environment: "right", input: {} } }]);
  const session = await f.create(t, {
    agentloop: agentloop({ implementation: { type: "selection_example" } })({ env: f.provider("selector") }),
    tools: [lookup({ env: hostEnv({ name: "left" }), where: "left" }), lookup({ env: hostEnv({ name: "right" }), where: "right" })],
  });
  await session.send("choose right");
  assert.deepEqual(called, ["right"]);
  const events = await collect(session.events());
  const started = events.find((event) => event.type === "tool_call_started");
  assert.equal(started.data.environment, "right");
  assert.deepEqual(started.data.invocation.input, {});
  assert.deepEqual(events.at(-1).data.result, { where: "right" });
  assert.deepEqual(f.modelRequests[0].tools[0].function.parameters.properties.environment.enum, ["left", "right"]);
});
