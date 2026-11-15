# Implementation plan

The service-integration work is recorded in [12 Service integration readiness](plans/12-service-integration-readiness.md), extending [11 Application SDK and session-bound execution](plans/11-application-sdk.md) and [10 Published clients and subsystem boundaries](plans/10-modernization.md). Earlier work is in [08 Integration expansion](plans/08-integration-expansion.md) and [09 Delivery, documentation, and examples](plans/09-delivery-and-examples.md).

- **Status:** In progress (local runtime, SDKs, HTTP/SSE tools, and archive adapters implemented)
- **Architecture:** [ADR 0001](adr/0001-local-first-session-server.md)
- **Release stages:** [Roadmap](roadmap.md)

This index links the subsystem plans and tracks how they fit together.

| Plan | Focus |
| --- | --- |
| [01 Local runtime](plans/01-local-runtime.md) | Go server, SQLite + event log storage, single-owner session loop, Pi integration, admission limits. |
| [02 External persistence](plans/02-external-persistence.md) | Background 64 KiB block uploads to filesystem/S3/R2/GCS, optional PostgreSQL discovery index, `flush`/`sync`/`restore`. |
| [03 Harness adapters](plans/03-harness-adapters.md) | Runner interface, JSON-over-stdio protocol, Pi runner, and planned OpenCode and Codex adapters. |
| [04 Distributed tools](plans/04-distributed-tools.md) | HTTP/SSE tool dispatch, scoped `inspect_session` tokens, external approval flow, planned MCP and tool catalog. |
| [05 API and SDKs](plans/05-api-and-sdks.md) | HTTP endpoints, SSE cursor semantics, code-generated OpenAPI/SDK types, TypeScript client. |
| [06 Observability and operations](plans/06-observability-and-operations.md) | Structured logs, OpenTelemetry traces/metrics, health checks, container packaging, handoff and load testing. |
| [07 Verification and release](plans/07-verification-and-release.md) | Unit/contract suites, end-to-end journey tests, failure injection, and release criteria. |
| [10 Modernization](plans/10-modernization.md) | Published SDK artifacts, consumer-owned runtime interfaces, tool transports, and local database adapters. |
| [11 Application SDK](plans/11-application-sdk.md) | Application composition, managed/runtime tools, encrypted credentials and shared Pi hosts. |
| [12 Service integration readiness](plans/12-service-integration-readiness.md) | Three harnesses, shared host-pool ownership, server/client model credentials and tool authoring, browser integrations and usage evidence. |

```mermaid
flowchart LR
    Local[R0 Local preview] --> Storage[R1 Portable state]
    Local --> Harnesses[R2 Harness integration]
    Local --> Tools[R3 Tooling]
    Storage --> Stable[R4 Operational release]
    Harnesses --> Stable
    Tools --> Stable
```

## Working rules

- **Code owns implemented behavior.** Once a slice lands, the Go types, SQL schema, and tests are the source of truth; the plan files track remaining work.
- **Ship complete vertical slices.** Update the server, runner, generated SDKs, docs, and tests together, and verify with `node scripts/check.mjs`.
- **Add dependencies only when needed.** Don't prebuild abstractions or pull in SDKs for unimplemented milestones.

## Repository layout discussion (2026-11-14)

Status: implemented after authorization on 2026-11-14. CI gates merge and release.

- Move the generated Brain HTTP contract from `api/openapi.json` to `services/brain/api/openapi.json`, beside the public Go types owned by that service.
- Keep Go route declarations and wire types as the source of truth, with Huma schema generation, openapi-typescript declarations and CI drift checks. Update generator output paths, website consumers, tests and documentation together when implementing the move.
- Document the contract's source files, regeneration command and consumers beside the artifact so its role is apparent from the directory.
- The placement is an ownership choice for this repository, not a Go or OpenAPI requirement. A single generated OpenAPI document remains appropriate.

Use service-local `cmd/<program>/` consistently for Go executable entry points, root `scripts/` for repository-wide automation, and component-local `scripts/` for automation owned by one component. Keep executable entry points focused on flags, configuration, startup and shutdown, with business logic in ordinary packages. Service entry points now use this layout.

Implementation preference: use popular, established libraries for standard infrastructure and protocol tooling where they fit, rather than maintaining equivalent custom implementations. Prefer the standard library when it already meets the need. The accepted tooling directions below are being implemented.

### SDK and tooling direction

- Remove the Python SDK from the supported implementation scope. Remove its generated types, packaging, publishing, consumer tests, examples and documentation references together. Update the remaining verification suite to match the supported TypeScript SDK; do not leave Python SDK checks disabled or silently skipped. Python used for unrelated repository tasks is a separate concern. This does not request deletion of already published PyPI artifacts.
- Do not adopt `eventsource-parser`: the user does not consider the functionality sufficient to justify this dependency. Retain owned SSE parsing and its protocol tests.
- Use Commander.js for the Node CLI, `openapi-typescript` plus `openapi-fetch` beneath the public TypeScript SDK, and retain Next.js/Fumadocs for the documentation website.
- Replace the custom schema/code-generation machinery with established tooling. Huma (Go-first) and oapi-codegen (contract-first) are alternative approaches, not a combined pipeline. Selected Huma schema generation to retain Go as the source of truth. Existing HTTP handlers retain request validation and transport lifecycles; this does not migrate handlers to Huma middleware. Runtime protocol types outside the HTTP contract still need an owned source and generation path.

### Tooling limits and integration responsibilities

| Tool | Limits and implications for Brain |
| --- | --- |
| [Commander.js](https://github.com/tj/commander.js) | Provides argument parsing, command dispatch and help, not Brain operations. Keep actions on the public SDK. Use `parseAsync()` for asynchronous actions. Configure output/exit handling to preserve the exported CLI entry point's testability and embedding behavior. |
| [openapi-typescript](https://openapi-ts.dev/introduction) | Generates compile-time types, not runtime response validation or business logic. Only describes what the contract represents; it does not replace non-HTTP runtime protocol declarations. |
| [openapi-fetch](https://openapi-ts.dev/openapi-fetch/) | Supplies typed HTTP calls, not the complete ergonomic `Brain` SDK or executor lifecycle. Retain SDK authoring helpers, error translation, telemetry and cancellation semantics. Streaming event parsing and executor WebSocket behavior need explicit integration; do not assume ordinary JSON operations cover them. Preserve existing redirect, timeout and no-automatic-action-replay behavior when configuring the client. |
| [Huma](https://huma.rocks/features/openapi-generation/) | Used for Go-to-schema generation. The existing HTTP handlers retain strict decoding, authentication, error envelopes, binary transfers, SSE and WebSocket lifecycles. Adopting Huma request middleware later would require explicit migration and qualification of those behaviors. Schema generation does not replace tenant and operation authorization. |
| [oapi-codegen](https://github.com/oapi-codegen/oapi-codegen) | A contract-first alternative requiring a source-of-truth change. Generated handlers do not supply complete request validation or authentication by themselves; use the supported middleware and owned authorization logic. Pin and verify a released version's OpenAPI 3.1/schema support: the development README may describe unreleased functionality. Generated Go types cannot express every JSON Schema union precisely. |
| Next.js / Fumadocs | Retain the existing documentation stack. Rendering the OpenAPI document does not validate server conformance or implement SDK behavior. Keep contract generation, endpoint pages and the published download synchronized. Check the emitted contract against the pinned `fumadocs-openapi` version when changing generators. |

These are integration constraints to address during implementation, not findings that the existing behavior is broken. Preserve and run all applicable checks when implementing the agreed scope.

### Sandbox and agent runtime ownership

Agreed: keep Sandbox a generic, separately runnable service. It owns isolated environments, resource limits, process streams, file transfer and environment cleanup. Agent runtimes remain separate from Sandbox's implementation; running inside a Sandbox-managed environment does not make the agent integration part of that service.

Brain owns agent coordination and the Pi-specific protocol adapter, prompt/steering behavior, tool forwarding and coherent checkpoint orchestration. Sandbox executes the installed runtime without implementing those agent semantics. Do not move the complete `runtimes/` tree into Sandbox or add Pi-specific session operations to its generic API.

Keep agent adapters and runnable payloads under `runtimes/`, separately from the generic Sandbox service. The private OS directory lock helper was renamed to `internal/dirlock`; its ownership and placement are superseded by the storage separation decision below.

### Storage ownership and three-tier separation (2026-11-15)

Status: implemented in the working tree; full verification, merge and deployment pending. The product owner authorized implementation on 2026-11-15. [ADR 0001](adr/0001-local-first-session-server.md#storage-adapters-and-tiers) owns the architecture.

Storage owns generic storage operations across memory, local disk and external backends. The in-memory storage tier is entirely Storage-owned, including cached session data and transcripts, budgets, load coalescing, pinning and eviction. Brain owns orchestration and invokes Storage primitives; disposable copies and staging do not justify a second storage implementation in Brain. Runtime execution files remain Sandbox-owned.

Implementation: `services/storage/client` calls Storage-owned transcript and snapshot primitives without retaining local files. Storage validates and stages streamed native state; Pi transfers it through Sandbox. Embedded persistence, archives and synchronization live under `services/storage/{local,archive,replication}`; the directory lock is private to Storage. Distributed Brain no longer opens a data directory, cleans a workspace, or mounts a storage volume. Shared telemetry setup lives under `services/shared/telemetry`; Brain instruments remain service-local.

Refactor and verification sequence:

1. Inventory Brain's filesystem operations and storage caches, including embedded storage, transcript/SSE working sets, snapshot staging and restore cleanup. Separate storage state from Brain's execution queues and Sandbox's runtime files.
2. Extend existing Storage-owned primitives only where needed for bounded reads/appends, state/object streaming, snapshot publication, restore and release. Keep tenant/session scope, owner fencing, cancellation, limits and explicit errors at the API boundary; do not expose Storage paths or add Pi-specific behavior to generic operations.
3. Move working-copy lifecycle, staging, cleanup and storage caches behind Storage's API. Keep memory → disk → external reads and disk → external metadata reads inside Storage. Preserve durable local acknowledgement, asynchronous replication and protection of unsynced data during eviction.
4. Replace Brain's local storage mechanics with API calls. Brain/Pi determine coherent capture and restore timing; Storage handles persistence and Sandbox handles runtime file transfer. Preserve embedded application behavior through the same Storage-owned implementation, without retaining duplicate storage logic in Brain.
5. Remove Brain's storage-directory lock and startup workspace deletion after their responsibilities have moved. Place the OS directory lock inside Storage's private implementation once all callers have moved; do not merely relocate the helper while leaving ownership unchanged. Update configuration, Compose volumes, layout documentation and affected contracts together.
6. Verify tier hits/misses, bounded transcript reads, concurrent loads, dirty eviction protection, durable acknowledgement, snapshot/restore, crash cleanup, tenant isolation and stale-owner rejection. Verify that restarting Brain cannot delete another process's storage working set. Run the complete required checks (`node scripts/check.mjs`) and all existing embedded/distributed tests; CI remains the release gate.

Completion means Brain needs no storage-owned local data directory or independent storage cache, all three tiers and their resource lifecycle are Storage-owned, and orchestration still accesses them through the owning API. This plan does not introduce a separate service for each tier.

### Credential ownership proposal

Provider credential ownership entirely within Tools, with transparent encryption behind a private scoped storage interface, remains proposed. The existing Brain admission / Tools decryption flow remains implemented and documented in `credentials/README.md`. Individual tool authors receive only their authorized binding credential and do not implement storage encryption. No public decrypt endpoint is planned.

### Public tool naming

Client-executed tools use `BrowserTool`, `BrowserContext` and `BrowserTransport` in the TypeScript API, with `browser_tools` / `browser_transport` for low-level session options. BrowserTool includes attached user client processes. Hosted, remote and browser remain the three tool flavours; agent runtimes are separate. Internal executor wire fields retain their protocol meaning.
