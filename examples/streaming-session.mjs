import { Brain } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";

const apiKey = process.env.OPENAI_API_KEY;
if (!apiKey) throw new Error("OPENAI_API_KEY is required");
const brain = new Brain({
  baseUrl: process.env.BRAIN_BASE_URL ?? "http://127.0.0.1:8080",
  token: process.env.BRAIN_API_TOKEN ?? "quickstart",
});

try {
  const session = await brain.sessions.create({
    model: { provider: "openai", name: "gpt-5-mini", apiKey },
    agentloop: pi(),
  });
  const subscription = new AbortController();
  let cursor = session.state.lastSequence;
  const watching = (async () => {
    for await (const event of session.stream(cursor, subscription.signal)) {
      console.log(event.type, event.data);
      if (event.sequence !== undefined) cursor = event.sequence;
      if (event.type === "turn_failed") throw new Error(JSON.stringify(event.data));
      if (event.type === "turn_ended") return;
    }
    throw new Error("Stream closed before the turn finished");
  })();
  try {
    await Promise.all([watching, session.send("Explain event streaming in one sentence.")]);
    console.log(JSON.stringify(await session.transcript(), null, 2));
    console.log("Last saved event:", cursor);
  } finally {
    subscription.abort();
    await watching.catch(() => {});
    await session.end();
  }
} finally {
  await brain.close();
}
