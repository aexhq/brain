import { Brain, brainEnv } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";

const options = {
  baseUrl: process.env.BRAIN_BASE_URL ?? "http://127.0.0.1:8080",
  token: process.env.BRAIN_API_TOKEN ?? "quickstart",
};
const apiKey = process.env.OPENAI_API_KEY;
if (!apiKey) throw new Error("OPENAI_API_KEY is required");
const submitter = new Brain(options);
let id, sequence;
try {
  const session = await submitter.sessions.create({
    model: { provider: "openai", name: "gpt-5-mini", apiKey },
    agentloop: pi({ env: brainEnv({ name: "brain" }) }),
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
  try {
    let outcome;
    do {
      outcome = await session.outcome(sequence);
      if (outcome.status === "pending") await new Promise(resolve => setTimeout(resolve, 500));
    } while (outcome.status === "pending");
    if (outcome.status === "failed") throw new Error(JSON.stringify(outcome.terminal.data));
    console.log(outcome.answer ?? JSON.stringify(await session.transcript(), null, 2));
  } finally {
    await session.end();
  }
} finally {
  await reader.close();
}
