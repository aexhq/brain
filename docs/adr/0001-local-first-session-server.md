# ADR 0001: Local-first ephemeral session server

- **Status:** Accepted
- **Date:** 2026-10-15
- **Amended by:** [ADR 0003](0003-application-sdk-and-session-bindings.md), which adds application composition, managed and runtime tool bindings, encrypted private credentials, and shared native hosts.
- **Amended by:** [ADR 0005](0005-harness-profiles-and-pi-plugins.md), which adds typed harness profiles and trusted server-installed Pi plugins.

## Context

Brain is a standalone agent session server designed to be embedded into a larger platform. The surrounding platform handles user auth, session routing, single-owner placement, scaling, and failover. Brain focuses on fast local session execution, event streaming, external tool calls, and optional background checkpointing. If a machine dies before a background checkpoint finishes uploading, losing the unsynced tail is acceptable.

## Decisions

We are building a small Go server with a thin TypeScript runner for **Pi** first. These eight rules define its boundaries:

1. **Keep the public API small.** Clients create sessions, send inputs, and read or stream events (`user.message`, `assistant.message`, `assistant.delta`, `tool.call`, `tool.progress`, `tool.result`, `session.status`, `session.error`). We do not expose Environment, job, effect, approval, or worker resources in the core API. One OpenAPI contract is generated directly from the Go code.
2. **Clear execution ownership.** Brain owns HTTP admission, auth, tool dispatch, and storage. The harness owns model requests, prompt construction, context compaction, and continuation after tool calls. Each session allows one active turn at a time on a single owning instance. Harness, tenant, model, reasoning effort, instructions, and enabled tools are fixed at session creation (see [proposed future config updates](../plans/05-api-and-sdks.md#future-configuration-updates-proposed)).
3. **Local working state and passive recovery.** SQLite holds session metadata; append-only files hold the event log and native harness state. Resuming a session loads local state first, falling back to external storage on a local cache miss. Missing or corrupted state returns an error rather than silently starting a fresh session. Restoring a session loads native harness context without calling the model or re-running tools.
4. **Asynchronous external checkpoints.** When configured, a background worker uploads immutable 64 KiB transcript blocks and native checkpoints to a local directory, S3, or Cloudflare R2, and updates an optional PostgreSQL session index. `POST /sessions/{id}/flush` forces an immediate upload and returns an immutable checkpoint reference; `GET /sessions/{id}/sync` reports sync progress.
5. **External HTTP/SSE tools.** All agent tools—including sandboxes, browsers, storage, and human approvals—run outside Brain over HTTP or SSE (with MCP planned). Brain authenticates requests, passes `tenant_id`, `session_id`, and `call_id` outside model arguments, and enforces timeouts and output size caps. Tools own their own jobs, retries, and cleanup; Brain never retries tool calls automatically.
6. **Three distinct error layers.**
   - *Client request errors* (`4xx`) reject bad inputs without breaking a healthy session.
   - *Tool failures* (denials, HTTP errors, timeouts, dropped connections) become `tool.result` events with `is_error: true` so the model can react.
   - *Execution failures* (provider errors, broken runner stdio, failed local disk writes) stop the active turn and emit `session.error`.
7. **Bounded queues and zero background inference.** In-process queues, SSE subscriber buffers, and storage retry loops are strictly bounded. Health checks and SSE keepalives never touch model context or trigger LLM calls, and upstream harness cache warming is disabled. Structured JSON logs and OpenTelemetry traces/metrics are built in without requiring an external collector to start.
8. **Clean breaks by default.** We do not maintain API versions, wire-protocol negotiation, or storage schema migrations across releases. When contracts change, we update the server, runner, generated SDKs, examples, and tests together.

## References

- [Roadmap](../roadmap.md) — release milestones (R0–R4)
- [Implementation plan](../implementation-plan.md) — work breakdown across subsystems
- [Harness integration notes](../harness-integration-notes.md) — research on Pi, Codex, OpenCode, and prompt caching
