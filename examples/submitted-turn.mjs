import { Brain, brainEnv } from "@aexhq/brain";
import { example } from "./example-brain.mjs";

const options = {
  baseUrl: process.env.BRAIN_BASE_URL ?? "http://127.0.0.1:8080",
  ...(process.env.BRAIN_API_TOKEN ? { token: process.env.BRAIN_API_TOKEN } : {}),
};
const apiKey = process.env.VERCEL_AI_GATEWAY_API_KEY;
if (!apiKey) throw new Error("VERCEL_AI_GATEWAY_API_KEY is required");
const submitter = new Brain(options);
let id, sequence;
try {
  const session = await submitter.sessions.create({
    environmentLifecycle: { default: "automatic" }, model: { provider: "vercel-ai-gateway", name: "openai/gpt-5-mini", apiKey },
    agentloop: example({ env: brainEnv({ name: "brain" }) }),
  });
  id = session.id;
  sequence = await session.submit("Reply with READY.", { idempotencyKey: crypto.randomUUID() });
} finally {
  await submitter.close();
}

// A later request needs only the saved session id and submitted turn sequence.
const reader = new Brain(options);
try {
  const session = await reader.sessions.get(id);
  let outcome;
  do {
    outcome = await session.outcome(sequence);
    if (outcome.status === "pending") await new Promise(resolve => setTimeout(resolve, 500));
  } while (outcome.status === "pending");
  console.log(outcome);
  await session.end();
  await session.delete();
} finally {
  await reader.close();
}
