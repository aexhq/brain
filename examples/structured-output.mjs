import { Brain, brainEnv } from "@aexhq/brain";
import { z } from "zod";
import { example } from "./example-brain.mjs";

const apiKey = process.env.VERCEL_AI_GATEWAY_API_KEY;
if (!apiKey) throw new Error("VERCEL_AI_GATEWAY_API_KEY is required");
const brain = new Brain({
  baseUrl: process.env.BRAIN_BASE_URL ?? "http://127.0.0.1:8080",
  token: process.env.BRAIN_API_TOKEN,
});
try {
  const session = await brain.sessions.create({
    model: { provider: "vercel-ai-gateway", name: process.env.BRAIN_MODEL ?? "openai/gpt-4.1-mini", apiKey },
    agentloop: example({ env: brainEnv({ name: "brain" }) }),
  });
  try {
    const person = await session.send("Ada is 37 years old. Extract her details.", {
      output: {
        type: z.object({ name: z.string(), age: z.number() }),
        maxRetries: 2,
      },
    });
    console.log(person);
  } finally {
    await session.end();
    await session.delete();
  }
} finally {
  await brain.close();
}
