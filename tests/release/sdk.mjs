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
assert.equal(session.state.status, "idle");
assert.equal((await brain.sessions.get(session.id)).id, session.id);
assert.ok((await brain.sessions.list()).some((candidate) => candidate.id === session.id));
assert.equal((await session.end()).status, "ended");
await session.delete();
assert.deepEqual(await brain.sessions.list(), []);
