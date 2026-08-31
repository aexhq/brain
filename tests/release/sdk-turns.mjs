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
  agentloop: diagnostic(),
});

const completed = await session.send("finish without external capabilities");
assert.equal(completed.status, "idle");

const events = [];
for await (const event of session.events()) events.push(event);
assert.deepEqual(
  events.slice(-4).map(({ type }) => type),
  ["turn_started", "activation_intent", "activation_result", "turn_finished"],
);
assert.deepEqual(events.at(-1)?.data.result, { activations: 1, observation: "user_message" });
assert.ok(events.every(({ recordedAt }) => recordedAt instanceof Date && !Number.isNaN(recordedAt.valueOf())));
assert.deepEqual(events.map(({ sequence }) => sequence), events.map((_, index) => index + 1));

await session.end();
await session.delete();
