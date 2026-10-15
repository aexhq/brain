# 07: Verification and release plan

- **Applies to:** All stages in the [roadmap](../roadmap.md)

Every advertised feature must be backed by automated tests in CI. Missing dependencies or credentials must fail the relevant test suite rather than silently skipping it.

## Test suites

| ID | Suite | What it runs |
| --- | --- | --- |
| **V1: Repo & contract checks** | `node scripts/check.mjs` | Builds `runners/pi` and `sdks/typescript`, verifies `go run ./cmd/generate -check`, runs `go test ./...` and `go vet ./...`, tests both SDKs, and builds/tests `website/`. |
| **V2: Adapter & storage contracts** | `internal/.../*_test.go`, Linux CI | Go unit and integration suites for SQLite, filesystem/S3/R2 checkpoint archives, PostgreSQL metadata, HTTP/SSE tools, and real Pi restore tests. |
| **V3: End-to-end journeys** | [`tests/journeys`](../../tests/journeys/README.md) | Spawns real Brain, Pi runner, and tool server processes on Linux and Windows to test conversations, approvals, interrupts, SSE reconnects, crashes, archive outages, corruption, overload, and OTLP export. |
| **V4: Container & cloud qualification** | `scripts/container-smoke.mjs` & protected CI | Docker smoke test (implemented in Linux CI), plus live S3/R2/provider qualification and multi-instance load/soak benchmarks before R4. |

## Core invariants verified by tests

| Scenario | Expected behavior |
| --- | --- |
| Process crash after `202 Accepted` | Committed local records survive; a torn final record is trimmed; interior file corruption fails loudly until an explicit `restore` is run. |
| Interrupted checkpoint upload | Incomplete uploads never update the `latest` pointer or PostgreSQL index; the previous complete checkpoint remains usable. |
| Flush during an active turn | `SyncStatus` reports `history_bytes` separately from `resume_offset` / `native_checkpoint`. |
| SSE disconnect or lost cursor | Disconnecting an observer leaves the turn running; a cursor lost after an external restore returns `409 cursor_unavailable`. |
| Tool error, denial, or timeout | Delivered to the model as a `tool.result` with `is_error: true` (and `unknown_outcome: true` on dropped/timed-out connections); never automatically retried. |
| Runner crash or local write failure | Aborts the turn and emits `session.error` (`brain_error`) rather than masquerading as a tool error. |
| All active slots waiting on tools | Interrupts, tool completions, `inspect_session` reads, and other sessions do not deadlock. |
| Idle sessions, keepalives, and cold restores | Zero background LLM calls; upstream cache warming (`cacheWarming: "off"`) and local bypass tools remain disabled. |
| Cold restore into an empty cache | The next Pi model request preserves the saved conversation prefix. |
| Overload or rejected request | Returns `429 overloaded` with `Retry-After: 1` without consuming the caller's idempotency `key`. |
