<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>Build agents your way. Run their tools wherever the work lives.</strong></p>

<p align="center">
<a href="https://aex.dev/brain/docs/quickstart">Quickstart</a> ·
<a href="https://aex.dev/brain/docs">Docs</a> ·
<a href="https://github.com/aexhq/extensions">Extensions</a> ·
<a href="README.cn.md">中文</a>
</p>

Brain is an open-source agent runtime. You choose the agent loop, the model, the tools and where
each one runs. Brain saves the session and gives your application one API to drive it.

```ts
const lookupOrder = tool({
  name: "lookup_order",
  description: "Look up an order by id.",
  input: z.object({ id: z.string() }),
  run: ({ id }, ctx) => ctx.finish({ id, status: "shipped" }),
});

const session = await brain.sessions.create({
  model: { provider: "openai", name: "gpt-5-mini", apiKey },
  agentloop: pi(),
  tools: [lookupOrder()],
});
await session.send("Has order A-1001 shipped?");
```

The loop runs on the Brain server. `lookup_order` runs in your app, next to your own data.

## Why Brain

- **Own the agent's behavior.** Start with a ready-made loop, or write your own context
  management, model calls and tool sequencing. Official loops use the same public interfaces as yours.
- **Run each tool where it belongs.** Your app's process, the user's browser tab, an HTTPS
  backend, a browser automation service or a remote sandbox. Choose per tool when you create a session.
- **Drive it from your app.** Send messages, stream progress, submit background work and collect
  results later through one session API.
- **Host it yourself, or don't.** Run the MIT-licensed server on your infrastructure, or let
  [Aex](https://aex.dev/docs) host it.

## Get started

The [quickstart](https://aex.dev/brain/docs/quickstart) runs an agent that checks an order through
a function in your app. You need Docker, Node.js 22 or newer and an OpenAI API key.

| I want to… | Read |
| --- | --- |
| Stream answers, submit work or reconnect | [Sessions](https://aex.dev/brain/docs/concepts/sessions) |
| Connect my functions or publish a tool | [Write a tool](https://aex.dev/brain/docs/guides/write-a-tool) |
| Change how the agent manages context and tools | [Write a loop](https://aex.dev/brain/docs/guides/write-a-loop) |
| Use browsers and remote execution | [Environments](https://aex.dev/brain/docs/concepts/environment) |
| Configure a self-hosted server | [Configuration](https://aex.dev/brain/docs/reference/configuration) |

Call `await brain.prepare(placed)` at startup to prepare loop and tool code once and reuse it
across sessions. See [Environment preparation](https://aex.dev/brain/docs/reference/environment-runtime#prepare-before-creating-sessions).

## When to use Brain

Brain is a service your application talks to. If one process can own the whole agent, an embedded
agent library may be simpler. Choose Brain when you want to swap the loop, spread tools across
systems and keep sessions behind their own API.

You don't need a sandbox: plain application functions work as tools. When a tool does need one,
the loop stays outside it, so sandbox failures reach your loop as observations it can act on.

Other frameworks and hosted services also offer extensions and persistent sessions. Brain pairs a
self-runnable engine with public interfaces for replacing both agent behavior and execution.

> **Early preview.** APIs may change before 1.0. Saved history survives a restart when storage is
> intact. Interrupted work is marked as failed, not resumed or retried, and history can't restore
> lost environment files or processes.

The application SDK supports TypeScript and JavaScript. Other clients can use the
[HTTP API](https://aex.dev/brain/docs/reference/api). Each extension guide lists its supported languages.

## Project

[Contributing](CONTRIBUTING.md) · [Design decisions](references/adrs/README.md) ·
[Benchmarks](BENCHMARKS.md) · [Security](SECURITY.md) · [MIT license](LICENSE)

Found a bug? [Open an issue](https://github.com/aexhq/brain/issues) or email
[support@aex.dev](mailto:support@aex.dev).
