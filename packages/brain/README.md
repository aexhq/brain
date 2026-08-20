# `@aexhq/brain`

The neutral TypeScript client and Tool authoring API for Brain. Brain is a long-lived server that
owns many durable sessions; it can run standalone or behind a downstream product such as Aex.

```ts
import { Brain, defineTool } from "@aexhq/brain";
import { z } from "zod";

const echo = defineTool({
  module: import.meta.url,
  name: "echo",
  description: "Return the supplied text.",
  input: z.object({ text: z.string() }),
  output: z.object({ text: z.string() }),
  async execute({ text }) {
    return { text };
  },
});

export default echo;

const brain = new Brain({ token: process.env.BRAIN_TOKEN! });
const session = await brain.sessions.create({
  model: { provider: "openai", name: "gpt-5", apiKey: process.env.OPENAI_API_KEY! },
  tools: [echo],
});
```

Tools run in the session Hand by default. Choose `echo.local()` explicitly to run its callback in
the attached application process. Brain exposes no model-visible tools when `tools` is omitted.
