import { Brain } from "@aexhq/brain";
import { example } from "./dist/index.mjs";

const apiKey = process.env.VERCEL_AI_GATEWAY_API_KEY;
if (!apiKey) throw new Error("VERCEL_AI_GATEWAY_API_KEY is required");

const brain = new Brain({
  baseUrl: process.env.BRAIN_BASE_URL ?? "http://127.0.0.1:8080",
  ...(process.env.BRAIN_API_TOKEN ? { token: process.env.BRAIN_API_TOKEN } : {}),
});

const created = await brain.sessions.create({
  model: {
    provider: "vercel-ai-gateway",
    name: process.env.BRAIN_MODEL ?? "openai/gpt-5-mini",
    apiKey,
  },
  agentloop: example(),
});

console.log("created", created.state);
console.log("listed", (await brain.sessions.list()).map(({ id, status }) => ({ id, status })));

const session = await brain.sessions.get(created.id);
console.log("reopened", session.state);

await session.cancel();
console.log("ended", await session.end());
await session.delete();

console.log("remaining", await brain.sessions.list());
