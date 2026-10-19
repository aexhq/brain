# End-to-end journey tests

The journey suites verify the compiled Brain server, real Pi/Codex/OpenCode runners, external HTTP/SSE tools and both installed SDKs against recording model providers. Browser journeys use real Chromium, the same-origin gateway and the remote browser service. These local tests need no external API keys, cloud accounts or Docker; setup installs pinned packages and Chromium.

## Run the suite

To run all repository checks including journeys:

```powershell
node scripts/check.mjs
```

To run only the journey suite (after building `runners/pi` and `sdks/typescript` and running `node scripts/package-sdks.mjs` to install the clients):

```powershell
node --test tests/journeys/journeys.test.mjs tests/journeys/adapters.test.mjs tests/journeys/browser.test.mjs
```

Each test compiles the Go binaries into an isolated temporary directory and starts separate processes with their own ports, tokens, and storage directories. Crash tests terminate the server and runner processes directly and verify cleanup and recovery on both Linux and Windows.

## What is covered

| Journey | What it verifies |
| --- | --- |
| Choose a native harness | Exact/empty catalogs, external callbacks, tenant context and keys, shared peers, cancellation, host failure, incremental checkpoints, automatic compaction and passive restart/cold restore on Codex and OpenCode. |
| Retain usage evidence | Acknowledged pre-dispatch starts, physical attempts where observable, cache/reasoning semantics, retries, rejection, partial streams, unknown outcomes and deduplicated crash reconciliation. Codex exposes explicit native-turn gaps. |
| Attach a browser | User gesture, same-origin SSE, scoped composition, duplicate and late replies, closed tabs, denied origins, expiry, revocation and explicit rebind without action replay. |
| Operate a remote browser | Real navigation, fill/click, read, screenshot and close; authenticated tenant/session context isolation and denied egress. Service tests cover redirects/subresources, capacity, idle expiry and cancellation. |
| Configure a local server | `-check` validates config without starting the server; invalid settings and external schema `$ref`s are rejected. |
| Start and continue a conversation | Create, read, list, send, and close; Unicode streaming, pagination, idempotency keys, and multi-turn model context. |
| Authenticate and delegate inspection | Tenant isolation across routes, tenant-filtered tool catalogs, read-only vs. management tokens, and revocable `inspect_session` tokens. |
| Use external tools and approvals | `echo`, `inspect_session`, and `ask_approval` (approve/deny, SSE progress, concurrent sessions, and duplicate-decision rejection). |
| Reconnect an observer | Resuming SSE streams via `?after=` and `Last-Event-ID`; client disconnects leave turns running; lost cursors return `409 cursor_unavailable`. |
| Queue, interrupt, and close | Queue limits (`429` with `Retry-After`), reusing keys after rejection, cancelling queued inputs on interrupt, and cleaning up pending approvals. |
| Handle tool failures | Denials, HTTP 500s, bad JSON, oversized outputs, dropped connections, timeouts, redirects, and incomplete SSE streams return `is_error` tool results to the model without automatic retries. |
| Restart after process death | Committed events survive SIGKILL; torn final records are trimmed; resuming is passive and never replays unfinished tool calls. |
| Recover into a fresh cache | Missing local sessions hydrate from external checkpoints; `history_bytes` and `resume_offset` stay distinct; restored sessions preserve prompt context on the next turn. |
| Recover from an archive outage | Local turns succeed while storage is down; full sync queues reject new input and mark readiness `503`; explicit `flush` clears the backlog once storage recovers. |
| Diagnose provider and Brain failures | Model provider errors and broken runner protocols fail the turn explicitly instead of masquerading as tool errors; credentials never leak into logs. |
| Repair corrupt local history | Interior file corruption fails loudly instead of silently truncating history; an explicit `restore` recovers the session from external storage. |
| Reject invalid API input | Unknown JSON fields, unsupported harnesses, unconfigured models or unauthorized tools, `steer` delivery, and oversized payloads return `400`/`403`/`413`/`422`. |
| Bound observer memory | Oversized events disconnect slow SSE observers without interrupting the turn or corrupting stored history. |
| Switch SDKs | A conversation started in TypeScript continues in Python across tool calls, streaming, flush/restore, and interrupt/resume. |
| Run concurrent tenants | Active execution limits hold under load, tenants stay isolated, and idle runner eviction preserves native context when reloaded. |
| Operate with telemetry | Trace IDs propagate from HTTP requests through tool calls; OTLP spans and metrics export cleanly; broken OTLP collectors do not block sessions. |
