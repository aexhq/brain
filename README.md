# Brain

A standalone agent session server with distributed tools and local working state.

Brain owns session transport, tenant isolation, persistence, and bounded execution. Harnesses own model calls and context. Tools run in external services, including approvals, storage, browsers, and code execution.

This cut targets local development. The current executable supports Pi, SQLite metadata, append-only event files, HTTP/SSE tools, authenticated sessions, bounded queues, interruption, completed-turn recovery, and event streaming. Optional filesystem/S3/R2 checkpoint storage and PostgreSQL discovery run outside the execution path. TypeScript and Python clients use generated wire types. Cloud qualification, Codex/OpenCode adapters, MCP, the broader tool catalog, and production capacity work remain on the [roadmap](docs/roadmap.md).

## Run locally

Requires Go 1.26+, Node 24+, and an API key for the selected model provider. From the repository root:

```powershell
npm --prefix runners/pi ci --ignore-scripts
npm --prefix runners/pi run build
$env:BRAIN_API_KEY = "replace-with-a-random-token-of-at-least-24-bytes"
$env:OPENAI_API_KEY = "your-provider-key"
go run ./cmd/brain -config examples/local.json
```

Keep credentials in environment variables. Copy the example to `brain.json` for local changes; that file is ignored by Git. `-check` validates configuration without starting the server. Configuration paths are relative to the process working directory.

In another terminal, build the client and run the [session example](examples/session.mjs):

```powershell
npm --prefix sdks/typescript ci --ignore-scripts
npm --prefix sdks/typescript run build
node examples/session.mjs
```

Session settings are frozen at creation. Input is queued; unsupported steering fails explicitly. Local acceptance means the event was recorded, not that the model consumed it. Reconnect to the event stream using the last retained event cursor. Deltas and progress are provisional. Disconnecting an observer does not cancel execution.

Tool bindings are host configuration: each descriptor supplies its schema, endpoint, allowed tenants, and optional credential environment variable. Only selected tools enter a session. Tool HTTP requests carry authenticated session/call scope outside model arguments. Responses contain `content` and optional `is_error`; SSE tools emit `progress` events followed by one `result`. Tools own approvals and remote jobs. Brain never automatically retries their actions.

The [local tool service](examples/tools/README.md) includes echo, scoped inspection, and an operator approval example. Its configuration is generated from the actual descriptors. It runs in a separate process and requires no cloud services.

Recovery loads saved native context without invoking a model. After interruption, explicitly resume before sending new input. Recovery can discard native work beyond the last completed checkpoint; observed history remains available. Remote side effects are not rolled back. Cache warming and native local tools are disabled. Read the [architecture](docs/adr/0001-local-first-session-server.md) for ownership and durability boundaries.

## Documentation

The [Fumadocs site](website/README.md) includes quickstart and usage guides, HTTP API reference, events, configuration, SDKs, model inventory, and adapter interfaces. To browse it locally:

```powershell
npm --prefix website ci --ignore-scripts
npm --prefix website run dev
```

Open `http://127.0.0.1:3000/docs`. The build generates API pages from the Go-owned OpenAPI contract, interfaces from Go declarations, and model inventory from the pinned Pi runtime. It publishes static files without adding a Brain runtime dependency.

## Checks

```powershell
node scripts/check.mjs
```

The checks require all three npm installations above and Python 3.11+. They build clients/runners and documentation, verify generated contracts and site links, run Go tests/vet, exercise the actual Pi SDK against a recording model provider, and test both clients. The [user journey suite](tests/journeys/README.md) runs actual server processes and both SDKs through conversations, tools/approvals, interruption, streaming, tenant isolation, process death, storage recovery, overload, and telemetry failures. Linux and Windows CI require these journeys. Linux CI also runs Go's race detector and PostgreSQL contracts. Portable-state tests restore into an empty cache and verify the next real Pi request preserves the saved prefix. Live cloud/provider verification is separate from recording-provider and SDK transport tests and requires configured credentials.

Generate API and runner declarations with `go run ./cmd/generate`. [OpenAPI](api/openapi.json) is generated from Go types and route declarations. Clean breaks are the default; dependency pins and standard document metadata do not imply API compatibility.

Set `OTEL_EXPORTER_OTLP_ENDPOINT` to export traces and metrics over OTLP/HTTP. Exporters are bounded and optional. Logs use structured JSON without request bodies. Health endpoints are `/health/live` and `/health/ready`.

Metrics cover request duration/errors, queued-input delay, first assistant output, runner startup, active execution, tool duration/outcomes, transcript/native byte growth, and synchronization backlog/duration/failures. They avoid tenant/session labels; traces identify individual sessions. These are measurements, not a claim of tested production capacity.

The [Dockerfile](Dockerfile) packages the server and Pi runner as an unprivileged process. Mount a configuration file at `/config/brain.json`; set its listen address to `0.0.0.0:8080` and data directory to `/data`. The surrounding platform owns routing, session placement, cross-instance ownership, and failover.

With Docker running, `docker build --tag brain:local-dev .` followed by `node scripts/container-smoke.mjs` checks the packaged server and real Pi runner using a local recording provider. It needs no model credentials and removes its temporary container afterward. Linux CI runs the same smoke test.

See [persistence and recovery](docs/running-persistence.md) for configuration, explicit flush, cache loss, and handoff.

