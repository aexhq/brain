# Brain roadmap

Status: Draft  
Date: 2026-10-15  
Architecture: [ADR 0001](adr/0001-local-first-session-server.md)

Brain will provide a standalone, ephemeral agent server that developers embed in wider platforms. Its hot path uses local state; external persistence and integrations are optional. A machine failure may lose the unsynchronized tail. The roadmap delivers a useful local server first, then proves portability, isolation, and capacity incrementally.

The release names below are planning gates, not calendar commitments. Implementation is underway: the local Pi server, clients, HTTP/SSE tools, and portable checkpoint path are implemented. Cross-platform CI and recording-provider tests cover the local core; cloud qualification, additional harnesses, catalog services, and capacity gates remain. Detailed work and acceptance checks live in the linked plans; this roadmap owns release scope and requirement coverage.

Current delivery scope is local development. Cloud deployment/qualification and production capacity claims are deferred. The complete roadmap remains below so the smaller cut does not silently remove the longer-term requirements.

| Release gate | Developer outcome | Required scope | Release evidence |
| --- | --- | --- | --- |
| R0 Local preview | Run and manage several local Pi sessions through one server. | Original milestones 1-3: local metadata/events/native state, authenticated session/event API, streaming, interrupt/resume, external HTTP tools, an external approval example, current-session inspection, TypeScript client, observability, bounded queues. | Real Pi lifecycle/restart, distinct tool/Brain errors, tool-wait responsiveness, scope, reconnect, and local load checks. Native tools cannot bypass distributed bindings. |
| R1 Portable state preview | Resume on a fresh server using external storage. | Original milestone 4: S3 and R2 transcript/checkpoint adapters, PostgreSQL metadata adapter, asynchronous batching/retries, explicit flush, synchronization progress, and external restore. | Real S3/R2/PostgreSQL contract tests; restore with empty caches; interrupted upload and storage-outage tests. The local preview still runs without those services. |
| R2 Harness integration beta | Choose Pi, OpenCode, or Codex at session creation through the same Brain API. | First part of milestone 5: pinned adapters, only necessary capability distinctions, MCP, native resume/rewind where proven, and Python client. | Conformance verifies the next request after restore, input timing during tool waits, error boundaries, and exclusion of bypassing tools. Each integration has an executable example. |
| R3 Tooling beta | Bind selected storage, research, browser, and compute tools to a session. | Remaining milestone 5: external catalog services/packages for S3/R2, Fly/Modal, code execution, browser access, search/fetch, and inspection. | Each service passes its provider/isolation tests and owns approval, job, and cleanup behavior. Packages can ship independently without adding core entities. |
| R4 Operational release | Operate Brain as a bounded compute unit in a larger multi-tenant platform. | Milestone 6: measured scale-out, long-session validation, shutdown/handoff/restore runbooks, and supported deployment artifacts. | All advertised features pass required CI, provider/isolation suites, load/soak tests, and fresh-infrastructure restore drills. Published limits identify hardware and workload. No cross-release compatibility promise is added. |

R0 is the first implementation target. Persistence, additional harnesses, and catalog packages can progress independently after the shared lifecycle/tool contracts exist. None should become a prerequisite for local execution. R4 requires evidence for the features advertised in the stable release, rather than treating integration code as proof of support.

Configuration stays frozen at launch. The [API plan](plans/05-api-and-sdks.md#future-configuration-updates-proposed) leaves room for later edits through the same session/configuration shape: model/effort at idle boundaries first, then instructions/tools and active-work boundaries where justified. This work is not a local-preview prerequisite or a committed release gate. Every adapter must support passive restore, isolated remote tools, and disabled cache warming; steering and live tool replacement can remain explicit optional operations.

| Requirement | Delivery owner | First delivery and completion |
| --- | --- | --- |
| Harness independence with frozen bindings | [Harness plan](plans/03-harness-adapters.md) | Pi at R0; all three harnesses at R2. |
| Language-neutral, distributed tools | [Tools plan](plans/04-distributed-tools.md) | HTTP/SSE at R0; MCP at R2; external browser/compute integrations at R3. |
| Transcript/context with local cache and cloud storage | [Persistence plan](plans/02-external-persistence.md) | Local at R0; S3 and R2 at R1. A third cloud provider is deferred until demanded. |
| Session metadata and SQL adapters | [Local plan](plans/01-local-runtime.md), [persistence plan](plans/02-external-persistence.md) | SQLite at R0; optional PostgreSQL at R1. |
| First-party tools and resources | [Tools plan](plans/04-distributed-tools.md) | External inspection/approval examples at R0; launch catalog at R3. No in-core execution services. |
| Multi-session and tenant/session scoping | [Local plan](plans/01-local-runtime.md), [tools plan](plans/04-distributed-tools.md) | Brain API/call scoping at R0; tool services enforce their resource authorization and execution isolation. |
| Interactive and resumable sessions | [API plan](plans/05-api-and-sdks.md), [harness plan](plans/03-harness-adapters.md) | R0 lifecycle; per-harness capability coverage at R2; portable restore at R1. |
| Low overhead and thousands of concurrent sessions | [Operations plan](plans/06-observability-and-operations.md) | Baseline at R0; measured capacity across independent instances at R4. |
| Horizontal deployment and disaster recovery | [Persistence plan](plans/02-external-persistence.md), [operations plan](plans/06-observability-and-operations.md) | Restore at R1; host-controlled handoff and recovery drills at R4. |
| Backpressure and bounded retries | [Local plan](plans/01-local-runtime.md), [persistence plan](plans/02-external-persistence.md) | Local admission limits at R0; external retry/backlog limits at R1. |
| Observability and maintainable contracts | [Operations plan](plans/06-observability-and-operations.md), [API plan](plans/05-api-and-sdks.md) | Instrumentation and one code-owned current contract begin at R0. Clean breaks apply at every release unless explicitly overridden. |

Brain owns session admission, scoped event/tool transport, persistence, and observability. Harnesses own inference/context/continuation; tool services own external execution, approvals, and cleanup. The host platform owns identity integration, placement, cross-instance ownership, autoscaling, global quotas, and regional failover. Brain needs no Environment framework, durable scheduler, broker, consensus service, or hosted control plane.

Dependencies are added with the feature that needs them. Start with Go standard-library services, one SQLite driver, the pinned Pi runner, and OpenTelemetry. Add cloud SDKs, PostgreSQL access, and MCP only with their adapters. Each addition must identify the supported use case and why an existing dependency or platform facility is insufficient.

Use the [implementation index](implementation-plan.md) to select the next task and the [verification plan](plans/07-verification-and-release.md) to determine release readiness. Performance numbers remain targets or measurements with stated conditions; the roadmap itself is not a capacity claim.
