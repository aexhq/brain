# `@aexhq/brain`

The typed client and extension composition contract for any Brain server.

```ts
import { Brain } from "@aexhq/brain";
import { awsMicroVm } from "@aexhq/env-aws-microvm";
import { pi } from "@aexhq/loop-pi";
import { bash, read } from "@aexhq/tools";

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080" });
const workspace = awsMicroVm({ region: "eu-west-2" });
const session = await brain.createSession({
  model: {
    provider: "vercel-ai-gateway",
    name: "openai/gpt-5-mini",
    apiKey: process.env.VERCEL_AI_GATEWAY_API_KEY!,
  },
  agentLoop: pi(),
  tools: [read().runIn(workspace), bash().runIn(workspace)],
});

await session.send("Read README.md and summarize it.");
for await (const event of session.events()) console.log(event);
```

The SDK admits Agentloops, infers Environment requirements from Tools, and supplies operation keys.
An explicit `idempotencyKey` remains available on mutating calls for durable caller retries.
