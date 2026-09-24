<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>Run AI agents. Keep their conversations and progress.</strong></p>

<p align="center">
  <a href="https://aex.dev/brain/docs/quickstart">Quickstart</a> ·
  <a href="https://aex.dev/brain/docs">Docs</a> ·
  <a href="https://github.com/aexhq/extensions">Extensions</a> ·
  <a href="README.cn.md">中文</a>
</p>

Brain is an open-source server for AI agents. Connect your model and tools, send a message,
and read the answer. Brain saves the conversation, tool results and progress so your app can
return to them later.

- Add functions from your application as tools the agent can call.
- Follow live output and inspect what happened in a session.
- Use a ready-made agent loop or write your own behavior.
- Run Brain on your own infrastructure, or use [Aex](https://aex.dev) for hosting.

> **Early preview.** APIs may change before 1.0. Saved history survives a server restart;
> interrupted work is reported as failed and is not automatically retried.

## Get started

You need Docker, Node.js 22 or newer, and an OpenAI API key. Start Brain:

```sh
docker run --rm -p 127.0.0.1:8080:8080 \
  -e BRAIN_LISTEN=0.0.0.0:8080 -e BRAIN_API_TOKEN=quickstart \
  -v brain-data:/var/lib/brain ghcr.io/aexhq/brain:latest
```

In another terminal, install the packages:

```sh
npm install @aexhq/brain@0.30.0 @aexhq/agentloop-pi@7.1.0 zod@4
```

Set `OPENAI_API_KEY` in your environment. Save this as `order.mjs`:

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

Run `node order.mjs`. The printed conversation includes the tool result and an answer that
order A-1001 has shipped. The example returns sample order data; replace the function with your
own lookup. It runs in your app, so keep that process connected while its tools are needed.

## Build your agent

| I want to… | Start here |
| --- | --- |
| Continue a conversation or read its history | [Sessions](https://aex.dev/brain/docs/concepts/sessions) |
| Connect my database or API | [Write a tool](https://aex.dev/brain/docs/guides/write-a-tool) |
| Change how the agent reasons and uses tools | [Write an agent loop](https://aex.dev/brain/docs/guides/write-a-loop) |
| Run code in a browser or sandbox | [Environments](https://aex.dev/brain/docs/concepts/environment) |
| Get a typed JSON answer | [Structured output](https://aex.dev/brain/docs/guides/structured-output) |

The client SDK supports JavaScript and TypeScript. Extension guides include JavaScript,
Rust and Python examples with their build steps. Other clients can use the
[HTTP API](https://aex.dev/brain/docs/reference/api).

## Why Brain?

An agent needs more than a model call: conversations need history, tools need results,
and applications need to know when work stops. Brain handles that session lifecycle while
you choose the model, tools and agent behavior.

For implementation details, see the [design decisions](references/adrs/README.md).
For source builds and checks, see [Contributing](CONTRIBUTING.md).
[Benchmarks](BENCHMARKS.md) describe measured workloads and their limits.

[MIT license](LICENSE). [Report an issue](https://github.com/aexhq/brain/issues)
or contact [support@aex.dev](mailto:support@aex.dev).

For short-lived requests, `session.submit()` returns a durable turn sequence;
`session.outcome(sequence)` reads its committed result from another client.
[Submission example](https://github.com/aexhq/brain/blob/main/examples/submitted-turn.mjs).
