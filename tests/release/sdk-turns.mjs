import assert from "node:assert/strict";

import { Brain, agentloop, brainWasm, component } from "@aexhq/brain";

const baseUrl = process.env.BRAIN_BASE_URL;
const token = process.env.BRAIN_API_TOKEN;
assert.ok(baseUrl);
assert.ok(token);

const brain = new Brain({ baseUrl, token });
const diagnostic = agentloop({
  implementation: component(new URL("./diagnostic-agentloop.wasm", import.meta.url)),
});
const session = await brain.sessions.create({
  model: {
    provider: "vercel-ai-gateway",
    name: "openai/gpt-5-mini",
    apiKey: "release-smoke-key",
  },
  agentloop: diagnostic({ env: brainWasm({ filesystem: { workspace: false } }) }),
});

const completed = await session.send("finish without external capabilities");
assert.equal(completed.status, "idle");

const events = [];
for await (const event of session.events()) events.push(event);
assert.deepEqual(
  events.slice(-5).map(({ type }) => type),
  ["turn_started", "activation_started", "note", "activation_ended", "turn_ended"],
);
assert.deepEqual(events.at(-1)?.data.result, { turns: 1, message: "finish without external capabilities" });
assert.ok(events.every(({ recordedAt }) => recordedAt instanceof Date && !Number.isNaN(recordedAt.valueOf())));
// One sequence counter numbers both logs, so the feed is strictly increasing, not contiguous.
assert.ok(events.every(({ sequence }, index) => index === 0 || sequence > events[index - 1].sequence));

await session.end();
await session.delete();
