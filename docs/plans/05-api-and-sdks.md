# 05: API and SDKs plan

- **Releases:** HTTP API, TypeScript SDK, and Python SDK implemented ([Roadmap](../roadmap.md))

Generate the OpenAPI spec and SDK wire types from a single source in Go (`internal/api/contract.go` and `internal/core/types.go` via `go run ./cmd/generate`).

## Work items

| ID | Scope | Acceptance criteria |
| --- | --- | --- |
| **A1: HTTP contract** | Define routes, request/response structs, error codes, pagination, and token grants; generate `api/openapi.json` and SDK types. | `go run ./cmd/generate -check` verifies zero drift; unknown JSON fields, bad scopes, and state conflicts return predictable status codes. |
| **A2: Streaming & cursors** | Implement `POST /sessions/{id}/events`, JSON pagination, and SSE streaming with replay-to-live handoff, slow-client buffer limits, and `409 cursor_unavailable`. | Reconnecting with `?after=` or `Last-Event-ID` neither drops nor fabricates events; client disconnects leave active turns running. |
| **A3: TypeScript & Python SDKs** | Ship `sdks/typescript` and `sdks/python` with generated wire types, SSE iterators, explicit timeouts, and `BrainError`. | Both SDKs pass unit tests and cross-SDK journey tests (`tests/journeys/journeys.test.mjs`). |
| **A4: Single current contract** | Update server, runner protocol, SDKs, docs, and tests together without versioned routes or legacy schema shims. | Unreadable state fails explicitly rather than silently resetting a session. |

## Implemented routes

| Operation | Route | Description |
| --- | --- | --- |
| Create | `POST /sessions` | Creates a session with fixed `harness` and `config`. |
| List / Get | `GET /sessions`, `GET /sessions/{id}` | Lists or fetches sessions within the caller's tenant scope. |
| Send / Interrupt | `POST /sessions/{id}/events` | Queues a `user.message` (with optional idempotency `key`) or sends `user.interrupt`. |
| History / Stream | `GET /sessions/{id}/events` | Returns paginated JSON events or streams SSE (`Accept: text/event-stream`). |
| Resume | `POST /sessions/{id}/resume` | Resumes an `interrupted` or `failed` session from its last completed checkpoint. |
| Flush / Sync / Restore | `POST /sessions/{id}/flush`, `GET /sessions/{id}/sync`, `POST /sessions/{id}/restore` | Forces an external checkpoint upload, checks sync status, or restores a specific checkpoint. |
| Close | `POST /sessions/{id}/close` | Permanently closes the session and stops active/queued work. |
| Tool catalog | `GET /catalog` | Lists tool descriptors enabled for the caller's tenant. |

## Future configuration updates (proposed)

Currently, `config` (`model`, `reasoning_effort`, `instructions`, and `tools`) is fixed when a session is created. When live configuration updates are added later, they will use the existing event endpoint rather than a separate versioning or patch API:

```json
{"type":"session.configure","config":{"reasoning_effort":"high"}}
```

Design rules for future config updates:
1. **Replace whole fields:** Omitted fields stay unchanged; provided fields (such as `tools`) replace the previous value in full.
2. **Idle boundary first:** Initially, accept `session.configure` only when the session is `idle` with no in-flight model call or pending tool results. Emit a `session.configured` event once applied and persist the updated config alongside the native checkpoint.
3. **Request-scoped tool bindings:** If active-turn config updates are added later, each in-flight model request and the tool calls it spawns must keep the tool definitions and bindings that were active when that request started.
