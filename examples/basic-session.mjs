import { Brain } from "@aexhq/brain";
import { example } from "./dist/index.mjs";

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
  agentloop: example(),
  system: "Answer briefly and directly.",
});

try {
  await session.send("Explain what an ephemeral execution runtime does in one sentence.");

  for await (const event of session.events()) {
    console.log(event.sequence, event.type, event.data);
  }
} finally {
  await session.end();
  await session.delete();
}
