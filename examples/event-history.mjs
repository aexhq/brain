import { Brain } from "@aexhq/brain";
import { pi } from "@aexhq/loop-pi";

const apiKey = process.env.VERCEL_AI_GATEWAY_API_KEY;
if (!apiKey) throw new Error("VERCEL_AI_GATEWAY_API_KEY is required");

const brain = new Brain({
  baseUrl: process.env.BRAIN_BASE_URL ?? "http://127.0.0.1:8080",
  ...(process.env.BRAIN_API_TOKEN ? { token: process.env.BRAIN_API_TOKEN } : {}),
});

const session = await brain.createSession({
  model: {
    provider: "vercel-ai-gateway",
    name: process.env.BRAIN_MODEL ?? "openai/gpt-5-mini",
    apiKey,
  },
  agentLoop: pi(),
});

try {
  await session.send("Reply with FIRST.");
  await session.send("Reply with SECOND.");

  const complete = await brain.readEvents(session.id);
  const firstTurn = complete.events.find((event) => event.type === "turn_finished");
  if (!firstTurn) throw new Error("the first turn did not finish");

  console.log(`Journal through sequence ${complete.nextCursor}`);
  console.log(`Events after the first turn (${firstTurn.sequence}):`);

  for await (const event of session.events(firstTurn.sequence)) {
    console.log(event.sequence, event.type);
  }
} finally {
  await session.end();
  await session.delete();
}
