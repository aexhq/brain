import { Brain, tool } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";
import { z } from "zod";

const apiKey = process.env.OPENAI_API_KEY;
if (!apiKey) throw new Error("OPENAI_API_KEY is required");

const lookupOrder = tool({
  name: "lookup_order",
  description: "Look up an order by id.",
  input: z.object({ id: z.string() }),
  run: ({ id }, ctx) => ctx.finish({ id, status: "shipped" }),
});

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080", token: "quickstart" });
brain.sessions.create({
  model: { provider: "openai", name: "gpt-5-mini", apiKey },
  agentloop: pi(),
  tools: [lookupOrder()],
}).then(async session => {
  const after = session.state.lastSequence;
  await session.send("Look up order A-1001. Has it shipped?");
  for await (const event of session.events(after)) {
    if (event.type === "turn_failed") throw new Error(JSON.stringify(event.data));
  }
  console.log(JSON.stringify(await session.transcript(), null, 2));
  console.log("Session:", session.id);
}).catch(error => {
  console.error(error);
  process.exitCode = 1;
});
