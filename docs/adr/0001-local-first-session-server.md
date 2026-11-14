# ADR 0001: Distributed Brain services

- **Status:** Accepted; implementation and qualification are tracked on the roadmap.
- **Originally accepted:** 2026-10-15.
- **Consolidated:** 2026-11-14, following the product owner's architecture review.
- **Authority:** This record consolidates the agreed intent and supersedes conflicting service, runtime, storage, packaging and telemetry decisions in ADRs 0002–0005. Those records retain historical rationale. Code and tests establish implemented behavior; this ADR establishes the target.

## Product intent

Brain is a multiplexed, session-based compute service written in Go. It separates the agent's brain from its hands. Existing sandbox and tool providers supply the hands; Brain must not become a replacement for them. The surrounding platform supplies end-user authentication, routing, session placement, exclusive ownership, scaling and failover.

The product owner requested independently runnable services with low coupling and high cohesion, and a layout that makes ownership apparent. Internal wire types and service-call clients belong beside their implementation. Public interfaces may be shared. Do not introduce a shared internal protocol hierarchy or separate services for every cache tier or session data type.

The initial user interfaces are the TypeScript SDK and CLI. Pi is the only supported agent runtime in this architecture. Every active or warm session has its own isolated Pi environment, managed by the same Sandbox service that executes uploaded tools. Retaining a session does not require retaining a running process.

The [self-hosted Claude Managed Agents architecture](https://platform.claude.com/docs/en/managed-agents/self-hosted) is a useful service/sandbox/model analogy. Its documented server runs the agent loop; our design puts Pi's loop inside an isolated session environment and leaves Go Brain responsible for coordination.

## Running services

| Service | Owns | Calls |
| --- | --- | --- |
| `brain` | Public session API, admission, ordered queues, turn concurrency, Pi coordination, durable event accounting, streaming, interruption and recovery | Storage, Tools, Sandbox |
| `storage` | Session metadata/state, transcripts, snapshots, attachments, tiering, capacity management and replication | Configured storage adapters |
| `tools` | Bindings, authorization, schemas, canonical invocation, deadlines, progress/results and hosted/remote/browser transports | Sandbox, remote endpoints, connected user executors |
| `sandbox` | Packages, isolated environments, processes, streams, resource enforcement, pools and cleanup | Configured isolation runtime |

Each service has its own executable, configuration, health endpoints, container image, resource allocation and shutdown lifecycle. Services communicate through owned APIs, never another service's private database, filesystem layout or internal business implementation.

Brain's queueing belongs in `internal/sessions`: preserve per-session ordering and decide when a turn can start under admission limits. Sandbox independently enforces environment/process capacity. There is no Scheduler service.

Colocate Brain, Storage and Sandbox where locality is useful. Independent lifecycles do not require different machines. Each Storage instance exclusively owns its local volume. Tools can scale independently; browser connections remain associated with their serving instance until explicitly reattached.

## Identity, ownership and admission

Clients provide session IDs. The logical key is `(tenant_id, session_id)` throughout caches, persistence, tool scopes and environment bindings. IDs are bounded opaque values, never directly interpolated into filesystem paths. Equal IDs in different tenants must not collide.

One Brain owner serializes mutations and runs at most one turn per session. Different sessions execute concurrently under bounded limits. Submission idempotency keys return the original acceptance for identical input and reject changed input using the same key.

The host platform supplies ownership and an increasing epoch. Coordination is synchronous and separate from asynchronously replicated discovery metadata. Lost ownership stops admission and new dispatch. Stale owners cannot advance the current checkpoint. Already-issued external actions can outlive a connection or lease; fencing does not imply exactly-once external side effects.

## Pi and Sandbox

Pi owns model requests, context construction/compaction, tool selection and continuation. Brain owns session lifecycle and event accounting. Brain asks Sandbox to create an environment and start Pi, then exchanges runtime messages through its attached process stream.

Each running or warm session has a dedicated environment; Pi environments are not shared between sessions. Uploaded tools execute separately so their code cannot read Pi context or model credentials. The same Sandbox service manages both workloads with different lifetimes and resource profiles.

Sandbox manages environments and processes, without Pi-specific agent behavior. Pi's protocol adapter, state import/export and tool forwarding belong in `runtimes/pi`. Apply configured isolation and resource limits before execution; fail if unavailable. A subprocess or JavaScript VM context alone is not the customer-code boundary.

Idle eviction first exports a settled snapshot and stores it durably. Resumption creates an environment and restores state. Startup/recovery never makes model calls or automatically repeats tools; interrupted execution requires explicit resumption.

## Storage adapters and tiers

All backends implement one low-level adapter contract: scoped keys, get, conditional put, delete, pagination, standardized missing/conflict errors and explicit limits. Backends include memory, filesystem, SQLite, S3/R2, PostgreSQL/Supabase and DynamoDB. Adapters perform I/O; logical repositories, chunking, TTL, tiering and replication are implemented once above them.

| Data | Memory | Local disk | External |
| --- | --- | --- | --- |
| Transcripts | Recent pages and bounded active cache | Durable append state and cached segments | Immutable objects/manifests |
| Session state | Active working set | Durable snapshots and cached state | Immutable snapshots |
| Attachments | Bounded descriptors/content cache | Upload staging and cached objects | Finalized objects/references |
| Metadata | Resident coordination fields only | SQLite records and pending synchronization | PostgreSQL/Supabase, DynamoDB or configured equivalent |

Transcripts and session state read memory → disk → external. Metadata reads disk → external. Transcript reads are bounded ranges with stable cursors; recent history must not require restoring the entire runtime. Concurrent misses share a load. Lower tiers are consulted on misses, not corruption, permission failures or arbitrary I/O errors.

Memory contains disposable copies. Local disk contains both externally committed cached copies and the unsynced authoritative tail. Commit accepted writes durably locally before acknowledging them, then replicate asynchronously. Group commit may amortize fsync without weakening acknowledgement. Machine loss can lose the unsynced tail; the product accepts this tradeoff. Flush returns the externally committed snapshot and transcript boundary. A sync interval is not a hard recovery-point guarantee during outages.

Upload immutable data before publishing its manifest, then advance the metadata pointer. Conditional writes reject stale publication. Commit metadata and pending sync together locally, or recover pending work from the durable journal. Deletion leaves a tombstone so delayed uploads and restoration cannot resurrect deleted sessions.

Cache TTL is separate from permanent retention. Evict a local copy only after another durable copy is committed. Pin active readers for their operation and preserve unsynced state. Under disk pressure, prioritize sync and reject further admission before losing data.

Capacity management combines periodic sampling and allocation/write triggers, byte budgets, idle TTL and high/low watermarks. Account for machine/container memory limits, Go memory, runtime/sandbox allocations, disk reserve, unsynced bytes and sync lag. Queues, subscribers, workers and uploads are bounded.

## Snapshots and attachments

A snapshot is a settled recovery point: serialized Pi files/state, configuration references and matching transcript position. It is not a process-memory dump and does not roll back external actions. Storage owns persistence; Brain/Pi determine when capture is coherent. Materialize workspaces through the API rather than sharing storage paths.

Attachments are streamed, finalized objects referenced by messages, not large inline transcript values. Messages cannot reference unfinished uploads. External references have explicit identity and authorized resolution; mutable URLs are not silently represented as immutable snapshots. Large tool outputs can use artifact references.

## Tools and integrations

| Flavour | Definition | Execution |
| --- | --- | --- |
| `HostedTool` | Immutable uploaded package, entrypoint, schemas, capabilities and limits | Sandbox-managed isolated environment |
| `RemoteTool` | Authorized endpoint/provider binding and credential reference | Remote HTTP/SSE or MCP endpoint |
| `BrowserTool` | User descriptor and connected client executor | User's attached browser/client process |

All flavours use the same invocation, progress and result contract. Authenticated tenant/session/epoch/turn/call identity, deadline and artifact references come from Brain outside model arguments. Credentials never become model-visible arguments or transcript content.

Tools validates and dispatches; it does not run the agent loop or write transcripts. Pi requests a tool from Brain; Brain durably records intent, invokes Tools, persists the result and returns it to Pi. Cancellation is explicit. A lost response after dispatch can mean the action happened: preserve known failure versus unknown outcome and never automatically replay a model action.

Modal, E2B, browser automation and resource tools are ordinary supplied packages under `tools/`. They use the same SDK, registration, schemas, credentials, bindings and limits as user tools. Provider implementations do not live in the core services or receive a privileged path. Remote browser automation is a tool package; BrowserTool is the transport for user-defined client execution.

## Public telemetry

Telemetry is a public cross-component contract. Shared conventions define event/metric names, units, attributes, correlation and propagation. Go exposes `telemetry`; the TypeScript SDK exposes configuration, context and instrumentation APIs. The CLI uses the SDK instrumentation. Pi reports model usage through its runtime protocol; Brain emits the corresponding session telemetry. Domain emitters stay beside their implementations.

Use OpenTelemetry exporters and trace propagation. Record admission, queue, turn and tool timing, model usage events, storage/cache/sync behavior and sandbox lifecycle. Resource profiles enforce container limits; fleet utilization reporting remains on the roadmap. Session/call identifiers are trace/log correlation fields, not unbounded metric dimensions. Default telemetry excludes credentials and transcript/tool payloads. Export queues and shutdown are bounded; exporter failures are observable without turning a successful application operation into a replayable failure.

## Layout

```text
services/
  brain/
    cmd/{brain,generate}/
    api/                         # Public session types, authentication and generated OpenAPI
    internal/{http,config,sessions,storage,archive,credentialtests}/
    storage/                     # Storage client working copies and snapshots
    distributed.go               # Four-service composition
  storage/
    api/                         # Owned wire types, client and chunk streams
    internal/
      sessions/metadata/         # Heads, tombstones, ownership and publication
      transcripts/               # Bounded pages and recent history
      tiering/                   # Memory/disk/external cache and capacity
      adapters/{memory,filesystem,sqlite,s3,dynamodb,postgres}/
    cmd/storage/main.go
  tools/
    api/                         # Canonical invocation and browser executors
    registry/                    # Public Go tool authoring and binding API
    internal/{server,hosted}/
    cmd/tools/main.go
  sandbox/
    api/
    internal/{server,environments}/
    cmd/sandbox/main.go
runtimes/
  pi/                            # Pi protocol and isolated session lifecycle
  host/                          # Shared process protocol and host admission
  common/
  {codex,opencode}/               # Existing embedded application integrations
tools/                          # Ordinary supplied tool implementations
  {integrations,service,sandbox,browser,resources,mcp}/
telemetry/                       # Public Go contract and bounded exporters
credentials/                     # Public encrypted credential capability
sdks/typescript/
cli/
deploy/docker/
tests/{distributed,journeys}/
internal/dirlock/                 # OS directory ownership lock
```

Attachments use Storage objects and metadata links; their public streaming/resolution adapter lives beside Brain’s Storage client. Splitting that adapter into another running service would add a hop without separating a lifecycle.

Wire types and service-call wrappers live in the owning service's `api/`. Public session types may be shared by consumers. No shared internal `protocol/` hierarchy. Private business code stays in service-local `internal/`. Local tests sit beside implementation; cross-service tests belong under `tests/`.

## Delivery and verification

Docker is the deployment target; Kubernetes is outside this scope. Compose runs the four services and an OpenTelemetry Collector. Runtime/tool images are separate from supervisor images. The product owner authorized local Compose deployment plus existing CI publication of tested images, SDK artifacts and documentation.

CI gates release. Run all required checks; do not skip tests or treat missing live credentials as successful qualification. Contract changes update implementations, generated SDKs/specifications, examples and tests together. No compatibility layer, negotiation or migration is required unless separately requested.

Verify external tenant-scoped IDs, submission deduplication, ordered/replayable events, local durability, cold restoration, tier pressure/no dirty eviction, partial publication, metadata sync, adapter contracts, process/session isolation, all tool flavours, cancellation/unknown outcomes, snapshot recovery, telemetry propagation, Docker lifecycle and installed SDK/CLI journeys. Measure retained sessions, active turns, resident Pi environments and active tools independently.

## Error and recovery contract

- Invalid input fails admission without breaking healthy sessions.
- Tool errors become canonical results; ambiguous dispatch remains explicit.
- Brain/storage/runtime failures stop the affected execution with an observable error.
- Slow subscribers are disconnected at bounded limits and reconnect by durable cursor.
- Lost ownership stops new dispatch; controlled handoff flushes before transfer.
- Recovery is passive: no inference, repeated effects or silently empty sessions.

## References

- [Roadmap](../roadmap.md): implementation and integration qualification.
- [Claude self-hosted Managed Agents](https://platform.claude.com/docs/en/managed-agents/self-hosted): analogy, not implementation dependency.

## Implementation boundaries

The Docker composition runs Pi only. Existing embedded Go/Node application APIs and their Pi/Codex/OpenCode qualification tests remain available in their own package. They do not select the distributed runtime. The `harness` wire field retains its established meaning: agent runtime, not another running service. A checkpoint is a serialized settled snapshot plus transcript position; clients are the SDK/CLI or service-local API wrappers, not a shared internal layer.

Docker containers provide the implemented execution boundary: non-root customer code, read-only image, dropped capabilities, no new privileges, bounded CPU/memory/PIDs and private workspace. Only the Sandbox controller has the Docker socket. Hosted modules have no network; Pi has outbound provider access. This is container isolation, not a microVM guarantee. The service network and tokens are a trusted deployment boundary; expose only Brain to end users.

Each cache partition has one Storage writer. The platform routes a session to that partition and supplies an increasing owner epoch. Local metadata CAS and externally conditional publication fence stale heads. Async object storage is not a distributed lease service. Browser executors likewise require routing to their Tools instance until reattached.

The initial replica worker processes bounded batches serially. Recent transcript reads fetch only the requested tail and do not load native state. Historical pages walk immutable chain descriptors, while active sessions and SSE materialize their transcript/native working set under per-session and loaded-session limits. These are explicit performance tradeoffs, not measured fleet capacity claims.

Compose uses separate Docker volumes for local cache and external filesystem/SQLite data. Those volumes survive process replacement but share the Docker host failure domain. Configure S3/R2/DynamoDB/PostgreSQL/Supabase for off-host persistence. Real account qualification and sustained workload benchmarks are recorded separately from local protocol tests on the roadmap.
