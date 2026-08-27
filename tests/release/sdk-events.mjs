import assert from "node:assert/strict";

import { Brain, defineAgentLoop } from "@aexhq/brain";

const baseUrl = process.env.BRAIN_BASE_URL;
const token = process.env.BRAIN_API_TOKEN;
assert.ok(baseUrl);
assert.ok(token);

const brain = new Brain({ baseUrl, token });
const session = await brain.createSession({
  model: {
    provider: "vercel-ai-gateway",
    name: "openai/gpt-5-mini",
    apiKey: "release-smoke-key",
  },
  agentLoop: defineAgentLoop(new URL("./diagnostic.brain.json", import.meta.url)),
});
await session.send("first turn");
await session.send("second turn");

const complete = await brain.readEvents(session.id);
assert.equal(complete.nextCursor, session.state.throughSequence);
const finished = complete.events.filter(({ type }) => type === "turn_finished");
assert.equal(finished.length, 2);

const cursor = finished[0].sequence;
const suffix = await brain.readEvents(session.id, cursor);
assert.deepEqual(
  suffix.events.map(({ type }) => type),
  ["turn_started", "activation_intent", "activation_result", "context_updated", "turn_finished"],
);
assert.equal(suffix.nextCursor, session.state.throughSequence);

const streamed = [];
for await (const event of session.events(cursor)) streamed.push(event);
assert.deepEqual(streamed.map(({ id }) => id), suffix.events.map(({ id }) => id));

await session.end();
await session.delete();
