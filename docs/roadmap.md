# Roadmap

Brain is in active development. This page lists what you can use today, what's coming next with today's workaround for each item, and how each integration has been verified. To ask about an item or suggest one, open an [issue](https://github.com/aexhq/brain/issues).

## Available today

- **[Sessions with the Pi harness](https://aexhq.github.io/brain/docs/guides/sessions/):** create sessions, stream events, interrupt and resume turns, and recover sessions after a restart.
- **SDKs:** the [TypeScript SDK](https://aexhq.github.io/brain/docs/sdks/typescript/) on npm and the [Python SDK](https://aexhq.github.io/brain/docs/sdks/python/) on PyPI.
- **Tools:** [HTTP and SSE tools](https://aexhq.github.io/brain/docs/tools/) in your own services, and [MCP tools](https://aexhq.github.io/brain/docs/tools/mcp/) from remote MCP servers.
- **[Persistence](https://aexhq.github.io/brain/docs/guides/persistence/):** checkpoint archives in a local directory, Amazon S3, Cloudflare R2, or Google Cloud Storage, and an optional PostgreSQL session index.
- **Tool services:** [sandboxes](https://aexhq.github.io/brain/docs/tools/sandboxes/) on Modal, E2B, and Daytona; [storage and web tools](https://aexhq.github.io/brain/docs/tools/resources/) for S3 or R2 storage, Brave or Tavily search, and public web fetch; and an [example tool server](https://aexhq.github.io/brain/docs/tools/local-tools/) with operator approvals.
- **[Platform examples](https://aexhq.github.io/brain/docs/guides/platforms/):** templates for Docker Compose, Fly.io, AWS ECS/Fargate, and a Cloudflare Worker tool service.
- **[Operations](https://aexhq.github.io/brain/docs/guides/operations/):** health checks, structured logs, OpenTelemetry traces and metrics, and a published container image.

## Next up

### Service integration readiness

Integrate services against one contract with Pi, Codex and OpenCode, server defaults and client-supplied model credentials, ordinary tool declarations, attached-tab and remote-browser tools, and retained model-usage evidence. Shared native servers keep their per-host caps; a common pool adds optional aggregate utilization controls without imposing another default limit. Platforms own session-inspection tools.

- **Status:** Requirements selected; implementation planned. [ADR 0004](https://github.com/aexhq/brain/blob/main/docs/adr/0004-service-integration-readiness.md) records the decisions, research and remaining SDK/browser/accounting choices. [Plan 12](https://github.com/aexhq/brain/blob/main/docs/plans/12-service-integration-readiness.md) orders the work.
- **Today:** Use the implemented Pi/backend surface below for experiments. Complete the new integration gates before dependent services cut over; the current client still exposes low-level binding fields and lacks private per-session model credentials.

### Application SDK and session-bound integrations

- **Status:** Available for Go application composition, authorized provider bindings, encrypted session credentials, TypeScript/Python runtime functions, direct HTTP/SSE, reverse SSE/HTTP, WebSocket RPC, MCP HTTP/stdio, and optional authorized session reads. [Plan 11](https://github.com/aexhq/brain/blob/main/docs/plans/11-application-sdk.md) records implementation evidence and remaining adapter gates.
- **Today:** Use the [Go application SDK](https://aexhq.github.io/brain/docs/sdks/application/) to install providers; declare runtime functions in a backend client and keep its callback connection alive. Browser authentication and automatic reconnection remain separate work.

### Local harness-host supervision

- **Status:** Available for Pi on Linux and Windows. Hosts have separate session occupancy and active-turn caps, private homes, session-local cancellation and eviction, and owned process trees. Tests cover eight concurrent native sessions with exact catalogs and separate context, bounded concurrent admission, detachment while peers continue, shared host crashes, and abrupt parent death. These are local correctness tests, not fleet capacity measurements.
- **Today:** Configure `max_shared`, `max_active_turns`, `max_hosts`, and `max_starting` alongside global limits. Other harnesses must pass the same recovery and isolation gates; each session still has one owning Brain instance.

### Dynamic session configuration

Change a session's model, instructions, reasoning effort, and tools between turns.

- **Status:** Planned. The [proposed design](https://github.com/aexhq/brain/blob/main/docs/plans/05-api-and-sdks.md#future-configuration-updates-proposed) applies a configuration event while the session is idle, and [plan 08](https://github.com/aexhq/brain/blob/main/docs/plans/08-integration-expansion.md) keeps it as follow-up work.
- **Today:** A session's configuration is fixed when you create it. To change it, create a new session with the new configuration.

### More harnesses: OpenCode and Codex

Choose OpenCode or Codex for each session through the same API you use for Pi. Each adapter ships once it passes the same tests as Pi: recovery without model or tool calls, an exact tool set, recovery after context compaction, and storage that grows only with new data.

- **Status:** Planned. Targeted local probes verified exclusion of the previously observed extra native tools; complete adapters still require recovery, persistence, and isolation qualification. The [harness probes](https://github.com/aexhq/brain/blob/main/docs/research/harness-probes.md) record that evidence and the remaining changes, and [plan 03](https://github.com/aexhq/brain/blob/main/docs/plans/03-harness-adapters.md) tracks the work.
- **Today:** Use the Pi harness, or run your own agent loop through the [harness process contract](https://aexhq.github.io/brain/docs/adapters/harness/).

### Tool catalog: browser and Fly Machines tools

Attach ready-made tools that work in an isolated browser, or that start, inspect, and stop Fly Machines for compute.

- **Status:** Planned. [Plan 08](https://github.com/aexhq/brain/blob/main/docs/plans/08-integration-expansion.md#i4-external-tool-catalog) describes both services and their safety boundaries, and [plan 04](https://github.com/aexhq/brain/blob/main/docs/plans/04-distributed-tools.md) tracks the tool catalog. Release stage [R3](#release-stages).
- **Today:** Serve your own tools over HTTP, SSE, or MCP. The [sandbox](https://aexhq.github.io/brain/docs/tools/sandboxes/) and [storage and web](https://aexhq.github.io/brain/docs/tools/resources/) tool services are available.

### Fleet-scale operations

Size a fleet from published evidence: multi-instance load and soak benchmarks with their hardware and workload details, plus handoff and disaster-recovery drills.

- **Status:** Planned. [Plan 08](https://github.com/aexhq/brain/blob/main/docs/plans/08-integration-expansion.md#i6-performance-and-operational-evidence) targets 1,000 retained sessions, then 1,000 active sessions across several servers. [Plan 06](https://github.com/aexhq/brain/blob/main/docs/plans/06-observability-and-operations.md) and [plan 07](https://github.com/aexhq/brain/blob/main/docs/plans/07-verification-and-release.md) own the operations work and release gates. Release stage [R4](#release-stages). Local tests don't establish thousand-session capacity, and the project doesn't claim it.
- **Today:** Load-test with your own workload before you size a deployment, and route each session to a single owner. [Operations](https://aexhq.github.io/brain/docs/guides/operations/) covers capacity limits and routing across servers.

### Steering a running turn

Add guidance to a turn while it runs. The API reserves `delivery: "steer"` for this.

- **Status:** Planned. No design document yet.
- **Today:** Queue your next message, which runs after the current turn, or interrupt the turn and send a new message.

### SDK auto-reconnect

SDK event streams reconnect on their own after a dropped connection.

- **Status:** Planned. No design document yet.
- **Today:** Reconnect by passing the cursor of the last event you processed. See [Session lifecycle](https://aexhq.github.io/brain/docs/guides/sessions/).

### Browser clients

Call Brain directly from browser apps, with cross-origin (CORS) support and browser-safe authentication.

- **Status:** Planned. No design document yet.
- **Today:** Proxy requests through your backend, so the Brain token stays on your server.

### OAuth for MCP servers and model providers

Connect MCP servers and model providers that require OAuth.

- **Status:** Planned. No design document yet.
- **Today:** Use API keys and tokens from environment variables. See [Models & providers](https://aexhq.github.io/brain/docs/reference/models/) and [MCP tools](https://aexhq.github.io/brain/docs/tools/mcp/).

## Integration status

Local tests and live runs are separate evidence. **Verified against live services** means the integration passed against a real provider account; **verified with local tests** means it passed with the provider's official SDK or local fixtures, without a live account. The [integration evidence](https://github.com/aexhq/brain/blob/main/docs/research/qualification.md) records exactly what each run covered.

| Integration | Status | Notes |
| --- | --- | --- |
| Amazon S3 archive | Verified against live services | Passed live cold-restore qualification in AWS eu-west-2, including immutable writes, restore into an empty directory, and older-checkpoint restore. This covers the tested standard bucket and IAM setup, not every S3-compatible service or policy. |
| Cloudflare R2 archive | Verified with local tests | The real AWS SDK transport passes the archive fixtures with R2 settings. Not yet run against a live R2 account. |
| Google Cloud Storage archive | Verified with local tests | The official SDK passes multipart, precondition, read, and shared archive contract tests. Not yet run against a live GCS account. |
| PostgreSQL session index | Verified with local tests | CI runs the index tests against a PostgreSQL 18 server. Hosted services such as Neon, Supabase, and RDS haven't been qualified separately. |
| MCP tools | Verified with local tests | Official SDK HTTP and owned-stdio fixtures cover explicit discovery, process loss, progress, structured output, tenant isolation, limits, cancellation, and no replay. No third-party MCP server has been tested live. |
| Modal, E2B, and Daytona sandboxes | Verified with local tests | Pinned SDK type checks, an HTTP tool journey, signed sandbox handles that stay valid across service restarts, and cancellation and cleanup fixtures. A live script, `npm --prefix services/sandbox run live -- PROVIDER`, needs your provider credentials. |
| Storage and web tools | Verified with local tests | An AWS SDK and HTTP registry journey covers authentication, tenant prefixes, read-only policy, and deletes, alongside Brave and Tavily request fixtures and public-network and HTML extraction tests. Live search and R2 tool accounts are unqualified. |
| Platform templates | Verified with local tests | `tests/platforms.test.mjs` validates the Docker Compose, Fly.io, AWS ECS/Fargate, and Cloudflare Worker templates. They aren't evidence of a live deployment on each platform. |
| Cloudflare Containers as a Brain runtime | Experimental | Cloudflare Containers run container images with Durable Objects for routing. This runtime stays experimental until container lifecycle and handoff tests pass. A standard Cloudflare Worker can host tool services, not Brain. |
| Google Cloud Run as a Brain runtime | Not validated | Turns keep running after the send-message request (`POST /sessions/{id}/events`) returns `202 Accepted`, so request-based CPU allocation throttles them. Instance-based CPU avoids that, but termination windows, request timeouts, and single-owner routing still need validation. Cloud Run's in-memory file system also needs conservative cache limits. |

## Release stages

Brain is built in stages: a solid local session server first, then portable session state, more harnesses, a catalog of ready-made tools, and evidence for running a fleet.

| Stage | Goal | Scope | Verification gate | Status |
| --- | --- | --- | --- | --- |
| **R0: Local preview** | Run and manage local Pi sessions through one server. | Session records in SQLite, append-only event and harness-state files, the session and streaming API, interrupt and resume, HTTP and SSE tools, approval and inspection examples, the TypeScript SDK, OpenTelemetry, and bounded queues. | End-to-end Pi lifecycle, restart recovery, separate tool and session errors, responsiveness during tool waits, tenant isolation, stream reconnects, and overload checks. | Available |
| **R1: Portable state** | Move or recover sessions across servers with external object storage. | S3, R2, and filesystem checkpoint stores, an optional PostgreSQL session index, background batching and retries, and explicit flush, sync, and restore. | PostgreSQL and S3/R2 contract suites, restore into an empty cache with the model's context intact, interrupted-upload tests, and backpressure tests during archive outages. | Available |
| **R2: Multi-harness beta** | Choose Pi, OpenCode, or Codex per session through the same API. | Pinned OpenCode and Codex adapters, the MCP tool transport, native resume and rewind where a harness supports them, and the Python SDK. | Conformance tests for next-turn context after restore, input queueing during tool waits, error boundaries, and fully disabling a harness's own local tools. | Partly available: MCP tools and the Python SDK. OpenCode and Codex are [next up](#more-harnesses-opencode-and-codex). |
| **R3: Tool catalog beta** | Attach ready-made storage, web research, browser, and compute tools. | External tool services for S3 and R2 buckets, Fly and Modal sandboxes, code execution, browser bridges, and web search and fetch. | Isolation, quota, approval, and cleanup tests for each service against real providers. | Partly available: storage, web search and fetch, and Modal, E2B, and Daytona sandboxes, verified with local tests. Browser and Fly Machines tools are [next up](#tool-catalog-browser-and-fly-machines-tools). |
| **R4: Operational release** | Run Brain as a predictable compute unit in a multi-tenant fleet. | Multi-instance load and soak benchmarks, long-session compaction and checkpoint measurements, and handoff and disaster-recovery drills. | Every advertised adapter passes credentialed CI, soak suites, and restore drills on fresh infrastructure, with published hardware and workload numbers. | Planned |

## Design documents

These documents are for contributors. They record how Brain is designed and how the work is split up.

- [ADR 0001: Local-first ephemeral session server](https://github.com/aexhq/brain/blob/main/docs/adr/0001-local-first-session-server.md) records the architecture, ownership boundaries, and design decisions.
- [ADR 0002: Subsystem contracts and published SDKs](https://github.com/aexhq/brain/blob/main/docs/adr/0002-subsystem-contracts-and-sdk-delivery.md) extends it with subsystem contracts and SDK delivery.
- [ADR 0003: Application SDK, session bindings, and harness integration](https://github.com/aexhq/brain/blob/main/docs/adr/0003-application-sdk-and-session-bindings.md) selects application composition, session-bound integrations and shared native hosts.
- [ADR 0004 Service integration readiness](https://github.com/aexhq/brain/blob/main/docs/adr/0004-service-integration-readiness.md) extends those decisions for dependent services and records the remaining choices.
- The [implementation plan](https://github.com/aexhq/brain/blob/main/docs/implementation-plan.md) breaks the work down across runtime, persistence, harnesses, tools, API, and operations.
- Research: [integration options](https://github.com/aexhq/brain/blob/main/docs/research/integration-options.md), [harness feasibility probes](https://github.com/aexhq/brain/blob/main/docs/research/harness-probes.md), [application SDK architecture spikes](https://github.com/aexhq/brain/blob/main/docs/research/application-sdk-spikes.md), and [integration evidence](https://github.com/aexhq/brain/blob/main/docs/research/qualification.md).

[Plan 10: Published clients and subsystem boundaries](https://github.com/aexhq/brain/blob/main/docs/plans/10-modernization.md) is implemented and deployed. [Plan 08: Integration expansion](https://github.com/aexhq/brain/blob/main/docs/plans/08-integration-expansion.md) and [plan 09: Delivery, documentation, and platform examples](https://github.com/aexhq/brain/blob/main/docs/plans/09-delivery-and-examples.md) have landed their first cuts. Their remaining work, including dynamic configuration, browser and Fly Machines tools, and fleet benchmarks, is listed under [Next up](#next-up).

| Area | Plan | Target stages |
| --- | --- | --- |
| Server runtime and local state | [01 Local runtime](https://github.com/aexhq/brain/blob/main/docs/plans/01-local-runtime.md) | R0 |
| Object checkpoints and PostgreSQL index | [02 External persistence](https://github.com/aexhq/brain/blob/main/docs/plans/02-external-persistence.md) | R1 |
| Pi, OpenCode, and Codex runners | [03 Harness adapters](https://github.com/aexhq/brain/blob/main/docs/plans/03-harness-adapters.md) | R0 (Pi), R2 (OpenCode, Codex) |
| Service integration prerequisites | [12 Service integration readiness](https://github.com/aexhq/brain/blob/main/docs/plans/12-service-integration-readiness.md) | Integration cutover gate |
| HTTP/SSE, MCP, and external tool catalog | [04 Distributed tools](https://github.com/aexhq/brain/blob/main/docs/plans/04-distributed-tools.md) | R0 (HTTP/SSE), R2 (MCP), R3 (catalog) |
| HTTP contract and TypeScript/Python SDKs | [05 API and SDKs](https://github.com/aexhq/brain/blob/main/docs/plans/05-api-and-sdks.md) | R0–R2 |
| Telemetry, limits, Docker, and capacity | [06 Observability and operations](https://github.com/aexhq/brain/blob/main/docs/plans/06-observability-and-operations.md) | R0–R4 |
| Test suites and release gates | [07 Verification and release](https://github.com/aexhq/brain/blob/main/docs/plans/07-verification-and-release.md) | R0–R4 |
| Cloud storage, MCP, tool catalog, harnesses, and capacity evidence | [08 Integration expansion](https://github.com/aexhq/brain/blob/main/docs/plans/08-integration-expansion.md) | R1–R4 |
