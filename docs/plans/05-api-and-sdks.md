# API and SDK implementation plan

Status: Draft  
Releases: core API and TypeScript client at R0; Python client at R2; operational evidence at R4

Developers create sessions and exchange events. Start with familiar messages, text deltas, tool calls/results, status, and errors; keep harness and tool configuration inline. No Environment, approval, job, or turn resource is required. The host supplies identity and placement; Brain authenticates each request and enforces its scope.

| Work item | Implementation | Acceptance |
| --- | --- | --- |
| A1 Contract | Define the current request/response/event unions, error codes, pagination, and only necessary capability fields. Generate OpenAPI/schema artifacts from one source. Start with configured keys mapped to tenant/session grants. | Schemas match the running API and examples. Invalid input, unauthorized scope, unsupported behavior, and state conflicts have predictable responses. Payload fields cannot assign authority. |
| A2 Interaction | Implement send/observe events, supported follow-up/steering, interruption, JSON history and SSE. Define catch-up/live handoff, slow-client bounds, and cursor-loss responses. | Reconnect does not start model work. Retained events are not missed at the handoff. Input accepted while a tool waits is not falsely reported as already consumed. Client disconnect leaves execution alive. |
| A3 Clients and examples | Ship a small TypeScript client at R0 and Python at R2, generated wire types, and minimal streaming helpers. Add lifecycle, HTTP tool, external approval, reconnect, restore, and harness examples. | Examples pass CI. Clients preserve error/event identity. Automatic retries do not duplicate unsafe work. Ordinary HTTP remains sufficient. |
| A4 Clean breaks | Update server, adapter protocol, generated clients, examples, and tests together whenever contracts change. Document any deliberate compatibility exception before implementing it. | One current contract; no API/schema version fields, versioned routes, compatibility negotiation, deprecated aliases, or migration framework. Unreadable retained state fails clearly without implicit recreation or deletion. |

The following route shapes are proposed for A1, not implemented endpoints:

| Operation | Proposed route | Contract |
| --- | --- | --- |
| Create | `POST /sessions` | Validate configuration/capabilities, establish bindings frozen for launch, and return session identity. |
| Inspect/list | `GET /sessions/{id}` and `GET /sessions` | Authorize scope; list with bounded pagination. |
| Send | `POST /sessions/{id}/events` | Accept typed input events locally; support follow-up or steering only with defined harness behavior. |
| Observe | `GET /sessions/{id}/events` | Return paginated history or SSE according to the requested representation, using opaque event cursors. |
| Interrupt | `POST /sessions/{id}/events` with `user.interrupt` | Request interruption; report actual outcome through session events. |
| Resume | `POST /sessions/{id}/resume` | Open readable native state or explicitly restore a checkpoint. Do not implicitly resubmit lost work. |
| Rewind, when supported | `POST /sessions/{id}/rewind` | Select an earlier history boundary through the harness after quiescence; preserve prior records and do not undo external effects. |
| Flush | `POST /sessions/{id}/flush` | R1: report external synchronization and the usable checkpoint according to the persistence contract. |
| Close | `POST /sessions/{id}/close` | End execution and release resources; data deletion/retention is a separate documented operation. |
| Discover tools | `GET /catalog` | Return permitted descriptors/capabilities without credentials. |

Proposed event names are user.message, assistant.message, assistant.delta, tool.call, tool.result, session.status, and session.error. Finalize the smallest set in code during A1. Initial system instructions belong in session configuration; append-only system.message input is optional and requires privileged caller scope and tested native support. Do not simulate it by rewriting an earlier prompt. An approval service exchanges ordinary tool data; there is no Brain approval event family or decision endpoint.

## Future configuration updates (proposed)

Use one `config` object for creation and inspection, containing the selected `model`, `reasoning_effort`, `instructions`, and `tools`. Keep `harness` and authenticated tenant/session identity outside mutable configuration. Launch accepts these settings only at creation; do not implement an unused mutation endpoint, config revision, or negotiation framework. This shape leaves room to enable editing later without replacing the session/event interface. It is not a backward-compatibility commitment.

When implemented, use the existing event route for a configuration command, for example:

```json
{"type":"session.configure","config":{"reasoning_effort":"high"}}
```

Omitted fields stay unchanged; supplied fields replace their value, including the complete tool list. Avoid a patch language. Validate the whole change, model/effort combination, compatibility with retained native context, tool bindings, and caller authority before applying anything. Unknown or unsupported fields fail explicitly. Ordinary end-user messages and tool results cannot configure a session.

Start with updates only when execution has settled, with no in-flight model request or unresolved tool calls. A busy session rejects the update; the caller can wait or explicitly interrupt through the existing operation. Configuration changes do not themselves start inference. Serialize application with input admission, expose only the applied configuration, and emit `session.configured` correlated to the command after application. If an adapter partially applies a change and cannot restore the prior settings, stop execution rather than continue with a mixed configuration.

Later support updates during active work only when an adapter exposes a reliable next-request boundary. Keep the same command and acknowledgment; acceptance is not application. Each model request and the tool calls it produces must retain their captured definitions/bindings. Later changes affect subsequent requests. This prevents a tool call generated against one schema from executing a different binding. Credential rotation and authorization revocation remain live security controls, including for older calls.

Persist the initial configuration and small applied changes with existing event identities; save the effective configuration with the matching native checkpoint. Recovery must use that pair, not a newer metadata row or replay of configuration commands from normalized history. An explicit rewind changes history and retains current settings; reapply them through native controls before continuing, or reject the combination if the adapter cannot do so correctly. Preserve supported prompt prefixes, but apply requested changes correctly even when they lose cache reuse. No per-turn prompt hash or public configuration version is needed.

Implement model/effort updates first, then instruction/tool changes where demonstrated demand and adapter support justify them. The [harness plan](03-harness-adapters.md) owns native application and boundary checks. Until that work is scheduled and passes conformance, configuration remains frozen.

## Contract and interaction rules

The server's public types generate its API schemas and client models. One current runner schema generates Go and TypeScript wire types. SQL definitions remain their own source. No duplicate hand-maintained SDK models or historical contract variants. Dependency release identifiers are build metadata, not a compatibility promise.

Local acceptance returns the accepted event identity; it means recorded input, not model consumption, completed work, or remote durability. Observe status/error events correlated to that input. Idle alone is not success. Deduplicate requests only within retained state; a definite rejection does not consume the key, and reuse with different input fails. Cache loss removes that deduplication guarantee.

Order events within a session and give them immutable IDs. Separate provisional display updates from replayable retained records. After restoration, a cursor may point into the lost tail. Return a typed `cursor_unavailable` response with the available recovery boundary instead of fabricating continuity or reusing old IDs. The client can refresh history and inform its application of the reset. Do not automatically rerun the missing turn.

Use a bounded catch-up/live handoff so subscription races neither lose retained events nor buffer indefinitely. Duplicate delivery is acceptable when event identity makes it detectable. A slow SSE consumer may be disconnected and resume from retained history. Explicit interruption belongs to a separate request, not to closing the observation stream.

Keep error boundaries explicit:

| Error | Behavior |
| --- | --- |
| Invalid/forbidden request, unsupported operation, or admission rejection | Reject the request; do not claim acceptance or halt healthy work. |
| Tool failure, denied approval, timeout, or unknown remote outcome | Supply a tool.result with the failure details to the harness; it decides how the model proceeds. |
| Brain append failure, broken runner protocol, or invalid execution state | Stop affected execution and emit/report session.error; do not disguise it as a model-recoverable tool result. |
| Model-provider failure | Report the harness's failed work distinctly; Brain adds no second model retry loop. |

Assign envelope type and authority from the authenticated path, never from a tool's data. An end-user message cannot become system context; a tool credential can complete only its authorized call. Duplicate or stale results cannot satisfy a new call. Route a genuine late notification as new input only through a supported path. Include machine-readable codes and request IDs without promising cross-release compatibility.

Credential material, native harness files, and internal provider endpoints are not ordinary session response fields. SDKs propagate trace context where appropriate, support configurable deadlines, and avoid logging request bodies by default. Timeout/cancellation of a client wait must have documented semantics separate from cancelling the underlying session.
