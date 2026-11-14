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

Brain runs agent sessions on infrastructure you control. Your application sends messages and connects tools; Brain keeps the session history and runs Pi in an isolated environment for each session.

Four services divide the work:

- **Brain** accepts messages, orders turns and streams events.
- **Storage** keeps transcripts, attachments and session state across restarts.
- **Tools** calls uploaded code, remote services and connected client functions.
- **Sandbox** creates and cleans up isolated environments for Pi and uploaded tools.

Sessions can outlive client connections. Your application chooses each session’s model, instructions and tools, then reconnects to its event history whenever it needs to.

## Get started

Follow the [Docker quickstart](https://aexhq.github.io/brain/docs/quickstart/) to start the services and connect with the TypeScript SDK or CLI. The [embedded application guide](https://aexhq.github.io/brain/docs/sdks/application/) covers running the existing server package from Node.js.

```ts
import { Brain, Pi, Model, ApiKey } from '@aexhq/brain';

const brain = new Brain('http://127.0.0.1:8080', process.env.BRAIN_API_KEY);
const session = await brain.create({
  id: 'support/issue-42',
  harness: Pi(),
  model: Model('openai/gpt-4.1-mini', ApiKey(process.env.OPENAI_API_KEY)),
});
await brain.send(session.id, { type: 'user.message', text: 'Hello.' });
```

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
| Call Brain from my backend | [TypeScript SDK](https://aexhq.github.io/brain/docs/sdks/typescript/) |
| Look up routes and config fields | [HTTP API](https://aexhq.github.io/brain/docs/api/) and [Host configuration](https://aexhq.github.io/brain/docs/reference/configuration/) |
| See what's coming next | [Roadmap](https://aexhq.github.io/brain/docs/roadmap/) |

## Build with a coding agent

Give your coding agent Brain's docs as plain text: [llms-full.txt](https://aexhq.github.io/brain/llms-full.txt) has every page in one file, and [llms.txt](https://aexhq.github.io/brain/llms.txt) lists the pages.

## Contributing

Install the dependencies and run every local check before you open a pull request. The checks need Go 1.26+, Node.js 24+, and Python 3.11+ for the sandbox execution fixture.

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
