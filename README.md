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
<a href="examples/platforms/README.md">Platform examples</a> ·
<a href="docs/research/qualification.md">Integration evidence</a>
</p>

Brain is an open-source server that runs AI agent sessions behind an authenticated HTTP API. It has a small set of primitives:

- **Sessions**, one agent conversation owned by one tenant, with its model and tools fixed at creation.
- **Events**, the ordered record of a session, which you page through or stream with server-sent events (SSE).
- **Harnesses**, the agent loop that calls the model. Brain ships the Pi harness and runs it as a child process.
- **Tools**, HTTP, SSE, or MCP endpoints in your own services that Brain calls when the model asks.
- **Checkpoints**, saved session state that Brain can upload to external storage to recover or move a session.

```ts
import { Brain } from "@aexhq/brain";

const brain = new Brain("http://127.0.0.1:8080", process.env.BRAIN_API_KEY!);
const session = await brain.create({ harness: "pi", config: { model: "openai/gpt-4.1-mini", tools: ["echo"] } });
const input = await brain.send(session.id, { type: "user.message", text: "Use echo to say hello." });

for await (const event of brain.observe(session.id)) {
  const data = event.data as { text?: string; message?: string; status?: string };
  if (event.type === "assistant.message") console.log(data.text);
  if (event.input_id !== input.id) continue;
  if (event.type === "session.error") throw new Error(data.message);
  if (event.type === "session.status" && data.status === "idle") break;
}
```

Brain runs the Pi harness in a child process on its own host. `echo` runs in a tool service you deploy, such as the [example tool server](examples/tools/README.md), and Brain calls it over HTTP with the authenticated tenant and session IDs.

## Why Brain

- **Keep tools in your own services.** Each tool is an HTTP, SSE, or MCP endpoint you write in any language. Sandboxes, storage, browsers, and human approvals stay outside the Brain process, and Brain enforces a timeout and an output limit on every call.
- **Reconnect without losing work.** Brain keeps each session's event history on disk. Clients stream over SSE, reconnect from the last cursor, interrupt a turn, and resume an interrupted session.
- **Recover on another server.** Sessions survive restarts. With a checkpoint store configured, Brain uploads checkpoints in the background so another instance can restore a session.
- **Separate tenants.** Every API token is bound to one tenant and can be read-only or limited to one session. Each tool lists the tenants allowed to call it.
- **Run it on your infrastructure.** Brain is one Go binary plus the Pi runner, MIT licensed, with a container image, health checks, JSON logs, and OpenTelemetry traces and metrics.

## Get started

The JavaScript/TypeScript SDK installs from npm (`@aexhq/brain`); the Python SDK installs from PyPI (`aex-brain`). Client applications do not need a checkout or Go. To build the server yourself, you need Go 1.26+, Node.js 24+, and a model provider key. You can also run the published [container image](https://github.com/aexhq/brain/pkgs/container/brain).

Build the Pi runner and start Brain:

```sh
git clone https://github.com/aexhq/brain.git
cd brain
npm --prefix runners/pi ci --ignore-scripts
npm --prefix runners/pi run build
export BRAIN_API_KEY="replace-with-a-random-token-of-at-least-24-bytes"
export OPENAI_API_KEY="your-provider-key"
go run ./cmd/brain -config examples/local.json
```

In a second terminal, install the JavaScript/TypeScript SDK and run the example session:

```sh
export BRAIN_API_KEY="replace-with-a-random-token-of-at-least-24-bytes"
npm install --no-save --package-lock=false @aexhq/brain@1.0.0
node examples/session.mjs "Explain what makes a good agent tool."
```

The [quickstart](https://aexhq.github.io/brain/docs/quickstart/) has PowerShell commands, the Python example, and cURL calls. To customize settings, copy `examples/local.json` to `brain.json`, which is gitignored and loaded when you omit `-config`. Add `-check` to validate a config without starting the server.

| I want to… | Read |
| --- | --- |
| Run my first session | [Quickstart](https://aexhq.github.io/brain/docs/quickstart/) |
| Understand how Brain runs a session | [Architecture](https://aexhq.github.io/brain/docs/architecture/) |
| Stream events, interrupt turns, and resume sessions | [Session lifecycle](https://aexhq.github.io/brain/docs/guides/sessions/) |
| Write a tool service | [Distributed tools](https://aexhq.github.io/brain/docs/tools/) |
| Connect a remote MCP server | [MCP tools](https://aexhq.github.io/brain/docs/tools/mcp/) |
| Issue tokens and separate tenants | [Authentication & tenants](https://aexhq.github.io/brain/docs/guides/authentication/) |
| Back up sessions and move them between servers | [Persistence & recovery](https://aexhq.github.io/brain/docs/guides/persistence/) |
| Deploy and monitor Brain | [Operations & observability](https://aexhq.github.io/brain/docs/guides/operations/) |
| Call Brain from my backend | [TypeScript SDK](https://aexhq.github.io/brain/docs/sdks/typescript/) or [Python SDK](https://aexhq.github.io/brain/docs/sdks/python/) |
| Look up routes and config fields | [HTTP API](https://aexhq.github.io/brain/docs/api/) and [Host configuration](https://aexhq.github.io/brain/docs/reference/configuration/) |

## When to use Brain

- **Long-running conversations:** Session history lives on disk, so clients can disconnect, reconnect from the last event cursor, and find their sessions after a restart.
- **Multi-tenant products:** Tokens are bound to tenants, and tools receive the authenticated tenant and session IDs outside the model's arguments.
- **Tools in other systems:** Sandboxes, browsers, storage, and approvals run as separate services that you deploy and scale on their own.

### Brain or an embedded agent library?

**Use an agent library** when one process can run the whole agent: your application owns the loop, the tools, and the conversation state, and nothing else needs to reach the session.

**Use Brain** when the agent needs to run as its own service: several clients or tenants reach sessions over HTTP, tools live in other systems, and sessions must outlast client connections and server restarts.

Brain has no built-in tools, agent definitions, or multi-agent handoffs. The model sees only the tools you deploy and enable for its session.

## Tools

You declare tools in the host config, and Brain calls them over HTTP, SSE, or MCP. Each call carries the model's arguments plus the authenticated `tenant_id`, `session_id`, and `call_id`. A synchronous tool returns JSON `content` and an optional `is_error`; a streaming tool sends `progress` events, then one `result`.

The repository includes tool services that you deploy separately from Brain:

- [`examples/tools`](examples/tools/README.md): a local tool server with `echo`, `inspect_session`, and an operator approval tool, `ask_approval`.
- [`services/sandbox`](services/sandbox/README.md): Modal, E2B, and Daytona sandboxes, published as `ghcr.io/aexhq/brain-sandbox-tools`.
- [`services/resources`](services/resources/README.md): S3 or R2 storage, Brave or Tavily search, and public web fetch, built from `cmd/resource-tools` and included in the Brain image as `/app/resource-tools`.
- [MCP tools](https://aexhq.github.io/brain/docs/tools/mcp/): discovery and calls for remote MCP servers over Streamable HTTP.

Provider keys are ordinary service configuration. Brain doesn't manage your cloud accounts.

## Persistence and recovery

Brain stores session metadata in SQLite and writes events and native harness state to append-only files in `data_dir` (`.brain/` by default). Optionally, it uploads checkpoints in the background to a local directory, Amazon S3, Cloudflare R2, or Google Cloud Storage, and keeps an eventually consistent session index in PostgreSQL. [Persistence & recovery](https://aexhq.github.io/brain/docs/guides/persistence/) covers explicit flushes and moving sessions between servers.

## Operations

- **Health:** `GET /health/live` and `GET /health/ready`.
- **Logs:** Structured JSON on stderr, without request bodies or credentials.
- **Telemetry:** Set `OTEL_EXPORTER_OTLP_ENDPOINT` to export traces and metrics over OTLP/HTTP.
- **Container:** Build the image and smoke-test it locally, without model credentials:

  ```sh
  docker build --tag brain:local-dev .
  node scripts/container-smoke.mjs
  ```

  In production, mount your config at `/config/brain.json`, set `listen` to `0.0.0.0:8080` and `data_dir` to `/data`, and mount a volume at `/data`.

## Development

Install dependencies and run every local check (needs Go 1.26+, Node.js 24+, and Python 3.11+):

```sh
node scripts/setup.mjs
node scripts/check.mjs
```

`check.mjs` builds the Pi runner and TypeScript SDK, checks generated wire types and the OpenAPI document (`go run ./cmd/generate -check`), runs Go tests and `go vet`, tests the Pi runner, both SDKs, and the sandbox service, runs the [end-to-end journeys](tests/journeys/README.md) and the platform example tests, and builds and tests the [docs site](website/README.md). To preview the docs site after setup, run `npm --prefix website run dev` and open `http://127.0.0.1:3000/docs`.

CI runs `check.mjs` on Linux and Windows, plus Linux race detection, tests against a real PostgreSQL, container smoke tests for both images, and CodeQL for Go, JavaScript/TypeScript, Python, and GitHub Actions. Nothing publishes unless every job passes. Each push to `main` publishes the tested images to `ghcr.io/aexhq/brain` and `ghcr.io/aexhq/brain-sandbox-tools`, tagged `sha-<commit>` and `main` with build provenance attestations, and deploys the tested docs site to GitHub Pages. Brain has no versioned server API or compatibility layer, so pin production images by digest. A successful main CI run also publishes the tested SDK archives through registry trusted publishing. SDK 1.0.0 replaces the incompatible predecessor 0.x API; pin client versions and the matching server commit. See [SDK delivery](docs/sdk-releases.md).

## Project

> [!NOTE]
> Pi is the supported harness. The [Codex and OpenCode feasibility results](docs/research/harness-probes.md) list the controls those adapters still need before they can offer the same recovery and tool guarantees. The [integration evidence](docs/research/qualification.md) separates integrations tested against live cloud services from those covered by local fixtures. Local tests don't establish thousand-session fleet capacity, and the project doesn't claim it.

The [architecture ADR](docs/adr/0001-local-first-session-server.md) records ownership boundaries and design decisions. The [roadmap](docs/roadmap.md) tracks release stages from local preview (R0) to operational release (R4), and the [implementation plan](docs/implementation-plan.md) breaks the work down across runtime, persistence, harnesses, tools, API, and operations.

<p align="center">
<a href="docs/adr/0001-local-first-session-server.md">Architecture ADR</a> ·
<a href="docs/roadmap.md">Roadmap</a> ·
<a href="docs/implementation-plan.md">Implementation plan</a> ·
<a href="docs/research/qualification.md">Integration evidence</a> ·
<a href="LICENSE">MIT license</a> ·
<a href="https://github.com/aexhq/brain/issues">Issues</a>
</p>
