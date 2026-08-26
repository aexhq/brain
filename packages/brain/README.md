# `@aexhq/brain`

The neutral TypeScript client and Tool authoring API for Brain. Brain is a long-lived server that
owns many durable sessions; it can run standalone or behind a downstream product such as Aex.

```ts
import { Brain, tool } from "@aexhq/brain";
import { z } from "zod";

const echo = tool(
  z.object({ text: z.string() }),
  async function echo({ text }) {
    return { text };
  },
)
  .describe("Return the supplied text.")
  .returns(z.object({ text: z.string() }))
  .server(import.meta.url);

export default echo;

const brain = new Brain({ token: process.env.BRAIN_TOKEN! });
const session = await brain.sessions.create({
  model: {
    dialect: "openai",
    baseUrl: "https://api.openai.com/v1",
    name: "gpt-4.1-nano",
    apiKey: process.env.OPENAI_API_KEY!,
  },
  tools: [echo],
});

console.log(await session.send("Echo hello."));
```

`model` is the whole model contract: the dialect the endpoint speaks, that endpoint, the
credential and the model id. Brain has no notion of a provider, so any endpoint speaking the
OpenAI or Anthropic shape works by pointing `baseUrl` at it — including a gateway, a proxy or a
local server. `baseUrl` is the full API root, version segment included, and Brain appends only the
operation path. `typed()` is exported here for component authors: it makes an export report its
declared `extension-error` instead of trapping.

`.server(import.meta.url)` bundles the function for Node 22 execution in the session Environment. Choose
`.client()` with a stable `Brain({ client: { id } })` identity when the callback must remain in the
application process. Brain exposes no model-visible tools when `tools` is omitted. Call
`brain.close()` when a long-lived customer Environment is no longer needed.

Large direct sandbox-file tickets are a process-local happy-path convenience. The SDK does not
automatically retry their prepare or complete calls; restart, expiry, missing state, or an
ambiguous outcome returns a typed error and requires inspecting the file before preparing again.
Use durable session storage plus `copyToSandbox`/`copyFromSandbox` when transfer recovery matters.
