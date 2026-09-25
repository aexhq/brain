<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>A minimal agent engine. A brain that outlives its sandbox.</strong></p>

<p align="center">
  <a href="https://aex.dev/brain/docs/quickstart">Quickstart</a> ·
  <a href="https://aex.dev/brain/docs">Docs</a> ·
  <a href="https://github.com/aexhq/extensions">Extensions</a> ·
  <a href="README.cn.md">中文</a>
</p>

Brain is an open-source engine for agents whose tools run across applications, browsers and
sandboxes. Start with a small core, choose your agent loop, and add the tools and environments
your application needs. Run it yourself or use [Aex](https://aex.dev) for hosting.

- **Minimal and extensible.** The core owns durable sessions and execution boundaries. Agent
  behavior, tools and runtime integrations are extensions built on public interfaces.
- **Independent lifetimes.** Agentloop, Tool and Env are separate building blocks. Keep the loop
  and session history outside a disposable sandbox so a sandbox failure leaves the agent's
  history available and its loop able to diagnose the failure.
- **Control over execution.** Place tools where their resources live. Brain records transport
  failures; environments can report resource failures even while idle. Choose automatic or
  explicit setup, and optionally give the model an ordinary `env` tool for authorized recovery.

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
npm install @aexhq/brain@0.34.0 @aexhq/agentloop-pi@7.2.1 zod@4
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

The client SDK supports JavaScript and TypeScript. Extension guides include JavaScript,
Rust and Python examples with their build steps. Other clients can use the
[HTTP API](https://aex.dev/brain/docs/reference/api).

## Why Brain?

The design takes inspiration from [Pi's minimal, extensible harness](https://pi.dev/) and
[Anthropic's separation of sessions, harnesses and sandboxes](https://www.anthropic.com/engineering/managed-agents).
Brain makes those boundaries available as an independently runnable engine with replaceable extensions.

Brain preserves committed history and reports uncertain outcomes. It does not restore lost files
or automatically retry effects. Inspection, restart and resource management belong to the Env;
the application decides which capabilities the model may use. See
[environment control](https://aex.dev/brain/docs/guides/environment-control).

For implementation details, see the [design decisions](references/adrs/README.md).
For source builds and checks, see [Contributing](CONTRIBUTING.md).
[Benchmarks](BENCHMARKS.md) describe measured workloads and their limits.

[MIT license](LICENSE). [Report an issue](https://github.com/aexhq/brain/issues)
or contact [support@aex.dev](mailto:support@aex.dev).

For short-lived requests, `session.submit()` returns a durable turn sequence;
`session.outcome(sequence)` reads its committed result from another client.
[Submission example](https://github.com/aexhq/brain/blob/main/examples/submitted-turn.mjs).
