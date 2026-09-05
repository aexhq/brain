import { Brain, brainWasm } from "@aexhq/brain";
import { example } from "./example-brain.mjs";

const apiKey = process.env.VERCEL_AI_GATEWAY_API_KEY;
if (!apiKey) throw new Error("VERCEL_AI_GATEWAY_API_KEY is required");

const brain = new Brain({
  baseUrl: process.env.BRAIN_BASE_URL ?? "http://127.0.0.1:8080",
  ...(process.env.BRAIN_API_TOKEN ? { token: process.env.BRAIN_API_TOKEN } : {}),
});

const session = await brain.sessions.create({
  model: {
    provider: "vercel-ai-gateway",
    name: process.env.BRAIN_MODEL ?? "openai/gpt-5-mini",
    apiKey,
  },
  agentloop: example({ env: brainWasm() }),
});

try {
  await session.send("Reply with FIRST.");
  await session.send("Reply with SECOND.");

  const complete = [];
  for await (const event of session.events()) complete.push(event);
  const firstTurn = complete.find((event) => event.type === "turn_ended");
  if (!firstTurn) throw new Error("the first turn did not finish");

  console.log(`Public Events through sequence ${complete.at(-1)?.sequence ?? 0}`);
  console.log(`Events after the first turn (${firstTurn.sequence}):`);

  for await (const event of session.events(firstTurn.sequence)) {
    console.log(event.sequence, event.type);
  }
} finally {
  await session.end();
  await session.delete();
}
