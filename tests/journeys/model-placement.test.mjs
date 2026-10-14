import assert from "node:assert/strict";
import test from "node:test";
import { hostEnv, tool } from "@aexhq/brain";
import { z } from "zod";
import { fixture, callTools, collect } from "./support.mjs";

const f = fixture();

test("model-visible selection dispatches one canonical Tool to the chosen authorized Environment", { timeout: 30_000 }, async (t) => {
  const called = [];
  const lookup = tool({ name: "lookup", description: "Lookup", input: z.object({}),
    output: z.object({ where: z.string() }), options: z.object({ where: z.string() }),
    run: (_, context) => { called.push(context.options.where); return context.finish({ where: context.options.where }); },
  });
  f.model = (_request, response) => callTools(response, [{ name: "lookup", input: { environment: "right", input: {} } }]);
  const session = await f.create(t, {
    agentloop: { name: "fixture", configuration: { mode: "selection" } },
    tools: [lookup({ env: hostEnv({ name: "left" }), where: "left" }), lookup({ env: hostEnv({ name: "right" }), where: "right" })],
  });
  await session.send("choose right");
  assert.deepEqual(called, ["right"]);
  const events = await collect(session.events());
  const started = events.find((event) => event.type === "tool_call_started");
  assert.equal(started.data.environment, "right");
  assert.deepEqual(started.data.invocation.input, {});
  assert.deepEqual(events.at(-1).data.result, { where: "right" });
  assert.deepEqual(f.modelRequests[0].tools[0].parameters.properties.environment.enum, ["left", "right"]);
});
