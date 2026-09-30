<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>Build your agent the way you want. Working across your systems</strong></p>

Brain is an open-source agent runtime with pluggable loops, models, tools and execution
environments. Use a ready-made loop or write your own context management and tool-calling logic.
Run tools in your application, browser or remote environment, and interact with the agent through
one API.

[Quickstart](https://aex.dev/brain/docs/quickstart) · [Docs](https://aex.dev/brain/docs) ·
[Extensions](https://github.com/aexhq/extensions) · [中文](README.cn.md)

A support agent could use tools you provide to look up an order in your backend, check a delivery
page in a browser and update a ticket. Each tool runs where it can access the resources it needs.

- **Choose the behavior.** Use a supplied agent loop or define your own context selection,
  model calls and tool sequencing through the same public interfaces.
- **Choose the execution.** Place the loop and tools independently. Connect application
  functions, HTTP services, browsers and remote compute through supported extensions.
- **Use it from your app.** Send work, follow progress and retrieve results through Brain's API.
  Run Brain yourself or use [Aex](https://aex.dev/docs) for hosting.

## Get started

The [quickstart](https://aex.dev/brain/docs/quickstart) runs an agent that looks up an order through
a function in your application. It provides the server command, matching packages, complete
TypeScript example and expected output. You need Docker, Node.js 22 or newer and an OpenAI API key.

| Next task | Guide |
| --- | --- |
| Stream an answer, submit work or reconnect | [Sessions](https://aex.dev/brain/docs/concepts/sessions) |
| Connect your functions or package a tool | [Tools](https://aex.dev/brain/docs/guides/write-a-tool) |
| Customize context and tool calls | [Agent loops](https://aex.dev/brain/docs/guides/write-a-loop) |
| Connect browsers and remote execution | [Environments](https://aex.dev/brain/docs/concepts/environment) |
| Configure a server | [Configuration](https://aex.dev/brain/docs/reference/configuration) |

## Where Brain fits

Brain runs as a service your application connects to. An embedded agent library may be sufficient
when one application process can own the whole agent. Brain lets you choose the loop and tool
placements behind a separate session API.

A sandbox is an isolated place to run code, useful for tools that need it. It is optional:
application functions work without one. Keeping the loop outside a tool's sandbox lets it receive
that environment's failures as observations; your loop decides what to do with them.
Other frameworks and hosted services also offer extensibility and persistent sessions. Brain
combines an independently runnable engine with public interfaces for replacing its agent behavior
and execution integrations.

> **Early preview.** APIs may change before 1.0. Committed history survives a server restart
> with storage intact; interrupted work is reported as failed and is not automatically resumed
> or retried. History does not restore lost environment files or processes.

The application SDK supports TypeScript and JavaScript. Other clients can use the
[HTTP API](https://aex.dev/brain/docs/reference/api); extension guides show supported language and
runtime choices.

[Contributing](CONTRIBUTING.md) · [Design decisions](references/adrs/README.md) ·
[Benchmarks](BENCHMARKS.md) · [Security](SECURITY.md) · [MIT license](LICENSE)

[Report an issue](https://github.com/aexhq/brain/issues) or contact [support@aex.dev](mailto:support@aex.dev).

Prepare reusable Agentloop and Tool code before creating sessions with `await brain.prepare(placed)`.
[Environment preparation](https://aex.dev/brain/docs/reference/environment-runtime#prepare-before-creating-sessions)
explains startup configuration, shared runtimes and reuse across sessions.
