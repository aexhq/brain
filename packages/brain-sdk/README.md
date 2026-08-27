# `@aexhq/brain`

The TypeScript client for any Brain server.

```ts
import { BrainClient } from "@aexhq/brain";

const brain = new BrainClient({
  baseUrl: "https://brain.example.com",
  apiKey: process.env.BRAIN_API_TOKEN,
});

const admission = await brain.admitAgentloop(packageBytes, crypto.randomUUID());
const session = await brain.createSession({
  agentloop_digest: admission.digest,
  model: { binding_id: "gateway", model: "openai/gpt-5" },
  presentation: { system: "You are helpful.", tools: [] },
  environments: [],
  tool_bindings: [],
}, crypto.randomUUID());

await session.send("Hello", crypto.randomUUID());
for await (const event of session.events()) console.log(event);
```

Mutating operations require an idempotency key. The SDK exposes the key explicitly so callers can
retry safely. `DurableEventBridge` is a small reference bridge for publishing committed events to a
durable queue. It advances its caller-owned cursor only after the queue acknowledges an event, so
retries deliver at least once.
