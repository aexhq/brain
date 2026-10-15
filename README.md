# Brain

Brain is a standalone server for running agent sessions.

It handles HTTP APIs, tenant isolation, event streaming, local storage, and checkpoint syncing. An agent harness (currently **Pi**) runs the model loop and manages context in a child process. Tools run as external HTTP or SSE services—meaning sandboxes, browsers, storage, and human approvals live outside the Brain process.

## Quickstart

You need **Go 1.26+**, **Node.js 24+**, and an API key for your model provider.

From the repository root, build the Pi runner and start the server:

```powershell
npm --prefix runners/pi ci --ignore-scripts
npm --prefix runners/pi run build
$env:BRAIN_API_KEY = "replace-with-a-random-token-of-at-least-24-bytes"
$env:OPENAI_API_KEY = "your-provider-key"
go run ./cmd/brain -config examples/local.json
```

Copy `examples/local.json` to `brain.json` (gitignored) if you want to customize settings locally. Pass `-check` to validate your config file without starting the server.

In a second terminal, build the TypeScript SDK and run the example session:

```powershell
npm --prefix sdks/typescript ci --ignore-scripts
npm --prefix sdks/typescript run build
$env:BRAIN_API_KEY = "replace-with-a-random-token-of-at-least-24-bytes"
node examples/session.mjs
```

## Tools

Tools are configured on the host and invoked over HTTP or SSE. When a model calls a tool, Brain sends the tool's arguments along with the authenticated `tenant_id`, `session_id`, and `call_id`. Synchronous tools return JSON (`content` and optional `is_error`); streaming tools emit `progress` events before a final `result`.

See [`examples/tools`](examples/tools/README.md) for a standalone local tool server that implements `echo`, `inspect_session`, and an operator approval flow (`ask_approval`).

## Persistence and recovery

By default, Brain stores session metadata in SQLite and writes events and native harness state to local append-only files in `.brain/`. You can optionally configure background checkpoint uploads to a local directory, S3, or Cloudflare R2, plus an eventual session index in PostgreSQL.

See [Persistence and recovery](docs/running-persistence.md) for configuration, explicit flushes, and session handoff between servers.

## Observability and Docker

- **Health checks:** `GET /health/live` and `GET /health/ready`.
- **Logs:** Structured JSON on stdout/stderr (request bodies and secrets are omitted).
- **OpenTelemetry:** Set `OTEL_EXPORTER_OTLP_ENDPOINT` to export traces and metrics over OTLP/HTTP.

To build and smoke-test the container image locally (no model credentials needed):

```powershell
docker build --tag brain:local-dev .
node scripts/container-smoke.mjs
```

When running the container in production, mount your config at `/config/brain.json`, bind `listen` to `0.0.0.0:8080`, and mount `/data` for local state.

## Documentation

The [`website/`](website/README.md) directory contains the Fumadocs site covering the HTTP API, events, configuration, SDKs, model catalog, and adapter interfaces:

```powershell
npm --prefix website ci --ignore-scripts
npm --prefix website run dev
```

Then open `http://127.0.0.1:3000/docs`.

## Development and checks

Run the full test and verification suite (requires Go 1.26+, Node.js 24+, and Python 3.11+):

```powershell
node scripts/check.mjs
```

This builds the Pi runner and TypeScript SDK, checks generated wire types and OpenAPI specs (`go run ./cmd/generate -check`), runs Go tests and `go vet`, tests both SDKs, runs the [end-to-end journey suite](tests/journeys/README.md), and builds and verifies the docs site.

## Project docs

- [Architecture ADR](docs/adr/0001-local-first-session-server.md) — ownership boundaries and design decisions
- [Roadmap](docs/roadmap.md) — release milestones from local preview (R0) to operational release (R4)
- [Implementation plan](docs/implementation-plan.md) — work breakdown across runtime, persistence, harnesses, tools, API, and operations
