import assert from "node:assert/strict";

import { Brain } from "@aexhq/brain";
import { diagnostic } from "./built/index.mjs";

const baseUrl = process.env.BRAIN_BASE_URL;
const token = process.env.BRAIN_API_TOKEN;
assert.ok(baseUrl);
assert.ok(token);

const brain = new Brain({ baseUrl, token });
const session = await brain.sessions.create({
  model: {
    provider: "vercel-ai-gateway",
    name: "openai/gpt-5-mini",
    apiKey: "release-smoke-key",
  },
  brain: diagnostic(),
});
await session.send("first turn");
await session.send("second turn");

const complete = [];
for await (const event of session.events()) complete.push(event);
assert.equal(complete.at(-1)?.sequence, session.state.throughSequence);
const finished = complete.filter(({ type }) => type === "turn_finished");
assert.equal(finished.length, 2);

const cursor = finished[0].sequence;
const suffix = [];
for await (const event of session.events(cursor)) suffix.push(event);
assert.deepEqual(
  suffix.map(({ type }) => type),
  ["turn_started", "activation_intent", "activation_result", "context_updated", "turn_finished"],
);
assert.equal(suffix.at(-1)?.sequence, session.state.throughSequence);

const streamed = [];
for await (const event of session.events(cursor)) streamed.push(event);
assert.deepEqual(streamed.map(({ id }) => id), suffix.map(({ id }) => id));

await session.end();
await session.delete();
