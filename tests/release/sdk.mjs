import assert from "node:assert/strict";

import { Brain } from "@aexhq/brain";
import { pi } from "@aexhq/loop-pi";

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
  agentLoop: pi(),
});
assert.equal(session.state.status, "idle");
assert.equal((await brain.getSession(session.id)).id, session.id);
assert.ok((await brain.listSessions()).some((candidate) => candidate.id === session.id));
assert.equal((await session.end()).status, "ended");
await session.delete();
assert.deepEqual(await brain.listSessions(), []);
