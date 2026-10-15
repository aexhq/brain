# 01: Local runtime plan

- **Release:** R0 ([Roadmap](../roadmap.md))
- **Architecture:** [ADR 0001](../adr/0001-local-first-session-server.md)

Build a single Go server that runs local Pi sessions with SQLite metadata, append-only event and native state files, external HTTP/SSE tools, and OpenTelemetry instrumentation.

## Work items

| ID | Scope | Acceptance criteria |
| --- | --- | --- |
| **L1: Foundation** | Go module, CLI flags (`-config`, `-check`), strict config validation, SQLite schema setup, structured JSON logging, and OTLP setup. | Clean checkout builds and starts on Linux and Windows; `-check` validates config and exits; startup needs no external database or telemetry collector. |
| **L2: Local store** | Session `create`/`get`/`list`/`close`, tenant-scoped SQLite metadata, append-only event log with opaque cursors, and startup recovery. | Completed turns survive restarts; cross-tenant lookups return `not_found`; torn final records are trimmed while interior corruption fails loudly; reading a session does not start a runner. |
| **L3: Pi execution** | Node.js Pi runner over stdio (`open`, `prompt`, `steer`, `interrupt`, `checkpoint`), per-session input queue, SSE streaming, and completed-turn checkpoint recovery. | Multi-turn conversations run and resume across server restarts; disconnecting an SSE stream leaves execution running; `interrupt` cancels active and queued work. |
| **L4: Tools & inspection** | HTTP/SSE tool dispatch with `tenant_id`/`session_id`/`call_id`, short-lived `inspect_session` token delegation, and the standalone [`examples/tools`](../../examples/tools/README.md) service. | Invalid tool arguments fail before HTTP dispatch; tool errors return to the model as `is_error` tool results; runner/disk errors abort the turn; built-in local Pi tools are disabled. |
| **L5: Resource limits** | Bounded active turns, loaded sessions, per-session input queues, input/session byte caps, SSE subscriber buffers, and graceful shutdown drain. | Overload returns `429` with `Retry-After: 1` without starving in-flight tool completions or interrupts; slow SSE clients disconnect cleanly when their buffer fills. |

## Implementation notes

- **State transitions vs. I/O waits:** Hold session locks only during brief state transitions. Never hold a session or registry lock while waiting on model inference, tool HTTP calls, or disk/network I/O.
- **Write before acknowledging:** Write accepted `user.message` events and emitted turn events to the local log before returning `202` or broadcasting to SSE subscribers.
- **Torn tail vs. interior corruption:** A crash during an append can leave a partial record at the very end of the file, which startup recovery trims back to the last complete record. Damaged framing followed by later records is treated as corruption and fails the session until explicitly restored.
