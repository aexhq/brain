<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>A minimal and extensible agent server for building agent applications</strong></p>

<p align="center">
<a href="https://aexhq.github.io/brain/docs/quickstart/">Quickstart</a> ·
<a href="https://aexhq.github.io/brain/docs/">Docs</a> ·
<a href="https://aexhq.github.io/brain/docs/api/">API reference</a> ·
<a href="examples/">Examples</a> ·
<a href="https://aexhq.github.io/brain/docs/roadmap/">Roadmap</a>
</p>

Brain is a single application you deploy as a service to run the agents behind your products. It's an open-source alternative to Claude Managed Agents and the OpenAI Agents SDK, running on your own infrastructure with the models you choose.

Brain gives you session-oriented primitives for building agents. Your application starts sessions, streams what the agent does, and connects tools from application code and services. Brain runs the agent loop and records each session's history, so you can replay a session or recover it after a failure.

- **[Sessions](https://aexhq.github.io/brain/docs/guides/sessions/)**: agent conversations that outlive connections and restarts.
- **[Events](https://aexhq.github.io/brain/docs/reference/events/)**: an ordered record of what the agent does, streamed live or replayed.
- **[Harnesses](https://aexhq.github.io/brain/docs/adapters/harness/)**: pluggable agent loops that drive the model and manage context.
- **[Tools](https://aexhq.github.io/brain/docs/tools/)**: capabilities implemented in your application, clients, or services.
- **[Checkpoints](https://aexhq.github.io/brain/docs/guides/persistence/)**: portable session state for recovering and moving sessions.

```ts
import { Pi, BrainServer, Model } from "brainserver";

const brain = new BrainServer();
await brain.start();
try {
  const session = await brain.create({ harness: Pi(), model: Model("openai/gpt-4.1-mini"), tools: [] });
  const input = await brain.send(session.id, { type: "user.message", text: "Say hello." });
  for await (const event of brain.observe(session.id)) {
    if (event.type === "assistant.message") console.log(event.data.text);
    if (event.input_id !== input.id) continue;
    if (event.type === "session.error") throw new Error(event.data.message);
    if (event.type === "session.status" && event.data.status === "idle") break;
  }
} finally {
  await brain.stop();
}
```

Save this as `agent.mjs` and follow the setup below.

## Why Brain

- **Choose where tools run.** Compose functions into your application, attach client functions, or connect services in any language.
- **Decide what the agent can do.** The model sees exactly the tools you enable for each session.
- **Reconnect at any time.** Sessions keep running when clients disconnect and survive server restarts, so any client can reconnect and continue from the last event it read.
- **Move sessions between servers.** Checkpoints let another server recover a session after a failure or take it over in a planned handoff.
- **Isolate every customer.** Keep each customer's sessions and tools separate, and give each client only the access it needs.
- **Run it yourself.** Brain is open source and self-hosted, with structured logs and OpenTelemetry traces and metrics.

## Get started

You need Node.js 24+ and an OpenAI API key for the example above.

1. **Install Brain.** The package includes the server and harness runtimes:

   ```sh
   npm install brainserver
   ```

2. **Run your script.** Set the provider key, then start the example:

   ```sh
   export OPENAI_API_KEY="your-provider-key"
   node agent.mjs
   ```

The [Quickstart](https://aexhq.github.io/brain/docs/quickstart/) includes PowerShell setup. For a separately deployed server, use the [container image](https://aexhq.github.io/brain/docs/guides/operations/#run-brain-in-a-container) and connect with `@aexhq/brain` or `aex-brain`.

## Documentation

| I want to… | Read |
| --- | --- |
| Run my first session | [Quickstart](https://aexhq.github.io/brain/docs/quickstart/) |
| Understand how Brain runs a session | [Architecture](https://aexhq.github.io/brain/docs/architecture/) |
| Stream events, interrupt turns, and resume sessions | [Session lifecycle](https://aexhq.github.io/brain/docs/guides/sessions/) |
| Write a tool service | [Distributed tools](https://aexhq.github.io/brain/docs/tools/) |
| Connect a remote MCP server | [MCP tools](https://aexhq.github.io/brain/docs/tools/mcp/) |
| Issue tokens and isolate customers | [Authentication](https://aexhq.github.io/brain/docs/guides/authentication/) |
| Back up sessions and move them between servers | [Persistence & recovery](https://aexhq.github.io/brain/docs/guides/persistence/) |
| Deploy and monitor Brain | [Operations & observability](https://aexhq.github.io/brain/docs/guides/operations/) |
| Call Brain from my backend | [TypeScript SDK](https://aexhq.github.io/brain/docs/sdks/typescript/) or [Python SDK](https://aexhq.github.io/brain/docs/sdks/python/) |
| Look up routes and config fields | [HTTP API](https://aexhq.github.io/brain/docs/api/) and [Host configuration](https://aexhq.github.io/brain/docs/reference/configuration/) |
| See what's coming next | [Roadmap](https://aexhq.github.io/brain/docs/roadmap/) |

## Build with a coding agent

Give your coding agent Brain's docs as plain text: [llms-full.txt](https://aexhq.github.io/brain/llms-full.txt) has every page in one file, and [llms.txt](https://aexhq.github.io/brain/llms.txt) lists the pages.

## Contributing

Install the dependencies and run every local check before you open a pull request. The checks need Go 1.26+, Node.js 24+, and Python 3.11+.

```sh
node scripts/setup.mjs
node scripts/check.mjs
```

[Development & verification](https://aexhq.github.io/brain/docs/development/) covers what the checks run, how CI publishes releases, and how to preview the docs.

## License

Brain is released under the [MIT License](LICENSE).

<p align="center">
<a href="https://aexhq.github.io/brain/docs/">Docs</a> ·
<a href="https://aexhq.github.io/brain/docs/roadmap/">Roadmap</a> ·
<a href="https://aexhq.github.io/brain/docs/architecture/">Architecture</a> ·
<a href="LICENSE">License</a> ·
<a href="https://github.com/aexhq/brain/issues">Issues</a>
</p>
