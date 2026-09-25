# Brain SDK

Connect your JavaScript or TypeScript app to [Brain](https://aex.dev/brain), an open-source
server that runs AI agents and saves their conversations and progress. Add your own functions
as tools, send messages and read the results.

SDK 0.34 moves prompt-based typed answers to the Aex SDK. Raw Brain `send()` returns
session state and rejects `output` options. See the [typed-answer migration](https://aex.dev/docs#structured-output)
for existing callers; provider-native model formats and Tool schemas are unchanged.

## Get started

Start a Brain server with the [quickstart](https://aex.dev/brain/docs/quickstart), then install:

```sh
npm install @aexhq/brain@0.34.0 @aexhq/agentloop-pi@7.2.2 zod@4
```

Set `OPENAI_API_KEY`, save the following as `order.mjs`, and run `node order.mjs`:

```js
import { Brain, brainEnv, tool } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";
import { z } from "zod";

const lookupOrder = tool({
  name: "lookup_order",
  description: "Look up an order by id.",
  input: z.object({ id: z.string() }),
  run: ({ id }, ctx) => ctx.finish({ id, status: "shipped" }),
});

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080", token: "quickstart" });
try {
  const session = await brain.sessions.create({
    model: { provider: "openai", name: "gpt-5-mini", apiKey: process.env.OPENAI_API_KEY },
    agentloop: pi({ env: brainEnv({ name: "brain" }) }),
    tools: [lookupOrder()],
  });
  try {
    await session.send("Look up order A-1001. Has it shipped?");
    console.log(JSON.stringify(await session.transcript(), null, 2));
    console.log("Session:", session.id);
  } finally {
    await session.end();
  }
} finally {
  await brain.close();
}
```

The transcript includes the lookup result and the agent's answer. Your tool runs in this process;
keep the client connected while the agent needs it. Use `ctx.finish(value)` to complete a tool.

## Next steps

| Task | Guide |
| --- | --- |
| Send messages, reconnect, stream output or stop work | [Sessions](https://aex.dev/brain/docs/concepts/sessions) |
| Add application functions or packaged tools | [Write a tool](https://aex.dev/brain/docs/guides/write-a-tool) |
| Customize the agent's behavior | [Write an agent loop](https://aex.dev/brain/docs/guides/write-a-loop) |
| Use another model | [Models](https://aex.dev/brain/docs/concepts/model) |

`brain.close()` releases client connections; it keeps stored sessions. `session.interrupt()`
stops work, `session.end()` finishes the conversation, and `session.delete()` removes its history.
Hosted execution can continue after client close; tools in your app still need your process.

For the exact tool return, error and background-work behavior, see the
[tool contract](https://aex.dev/brain/docs/reference/tool-contract).

For short-lived requests, `session.submit()` returns a durable turn sequence;
`session.outcome(sequence)` reads its committed result from another client.
[Submission example](https://github.com/aexhq/brain/blob/main/examples/submitted-turn.mjs).
