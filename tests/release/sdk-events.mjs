import assert from "node:assert/strict";

import { Brain, agentloop, brainEnv, component } from "@aexhq/brain";

const baseUrl = process.env.BRAIN_BASE_URL;
const token = process.env.BRAIN_API_TOKEN;
assert.ok(baseUrl);
assert.ok(token);

const brain = new Brain({ baseUrl, token });
const diagnostic = agentloop({
  implementation: component(new URL("./diagnostic-agentloop.wasm", import.meta.url)),
});
try {
  const session = await brain.sessions.create({
    model: {
      provider: "vercel-ai-gateway",
      name: "openai/gpt-5-mini",
      apiKey: "release-smoke-key",
    },
    agentloop: diagnostic({ env: brainEnv({ name: "brain" }) }),
  });
  await session.send("first turn");
  await session.send("second turn");

  const complete = [];
  for await (const event of session.events()) complete.push(event);
  assert.equal(complete.at(-1)?.sequence, session.state.lastSequence);
  const ended = complete.filter(({ type }) => type === "turn_ended");
  assert.equal(ended.length, 2);

  const cursor = ended[0].sequence;
  const suffix = [];
  for await (const event of session.events(cursor)) suffix.push(event);
  assert.deepEqual(
    suffix.map(({ type }) => type),
    ["session_resumed", "turn_started", "activation_started", "note", "activation_ended", "turn_ended"],
  );
  assert.equal(suffix.at(-1)?.sequence, session.state.lastSequence);

  const streamed = [];
  for await (const event of session.events(cursor)) streamed.push(event);
  assert.deepEqual(streamed.map(({ sequence }) => sequence), suffix.map(({ sequence }) => sequence));

  await session.end();
  await session.delete();
} finally {
  await brain.close();
}
