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
assert.equal(session.state.status, "idle");
assert.equal((await brain.sessions.get(session.id)).id, session.id);
assert.ok((await brain.sessions.list()).some((candidate) => candidate.id === session.id));
assert.equal((await session.end()).status, "ended");
await session.delete();
assert.deepEqual(await brain.sessions.list(), []);
