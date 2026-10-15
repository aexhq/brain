# Brain

A standalone agent session server with distributed tools and local working state.

Brain owns session transport, tenant isolation, persistence, and bounded execution. Harnesses own model calls and context. Tools run in external services, including approvals, storage, browsers, and code execution.

Implementation is in progress. The current executable supports Pi, SQLite metadata, append-only event files, HTTP/SSE tools, authenticated sessions, bounded queues, interruption, completed-turn recovery, and event streaming. Optional filesystem/S3/R2 checkpoint storage and PostgreSQL discovery run outside the execution path. TypeScript and Python clients use generated wire types. The [roadmap](docs/roadmap.md) and [implementation plans](docs/implementation-plan.md) describe remaining integrations; they are not release claims.

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

Recovery loads saved native context without invoking a model. After interruption, explicitly resume before sending new input. Recovery can discard native work beyond the last completed checkpoint; observed history remains available. Remote side effects are not rolled back. Cache warming and native local tools are disabled. Read the [architecture](docs/adr/0001-local-first-session-server.md) for ownership and durability boundaries.

## Checks

```powershell
node scripts/check.mjs
```

The checks require both npm installations above and Python 3.11+. They build clients/runners, verify generated contracts, run Go tests/vet, exercise the actual Pi SDK against a recording model provider, and test both clients. Linux CI also runs Go's race detector and PostgreSQL contracts. Portable-state tests restore into an empty cache and verify the next real Pi request preserves the saved prefix. Live cloud/provider verification is separate from recording-provider and SDK transport tests and requires configured credentials.

Generate API and runner declarations with `go run ./cmd/generate`. [OpenAPI](api/openapi.json) is generated from Go types and route declarations. Clean breaks are the default; dependency pins and standard document metadata do not imply API compatibility.

Set `OTEL_EXPORTER_OTLP_ENDPOINT` to export traces and metrics over OTLP/HTTP. Exporters are bounded and optional. Logs use structured JSON without request bodies. Health endpoints are `/health/live` and `/health/ready`.

The [Dockerfile](Dockerfile) packages the server and Pi runner as an unprivileged process. Mount a configuration file at `/config/brain.json`; set its listen address to `0.0.0.0:8080` and data directory to `/data`. The surrounding platform owns routing, session placement, cross-instance ownership, and failover.

See [persistence and recovery](docs/running-persistence.md) for configuration, explicit flush, cache loss, and handoff.

