# Brain roadmap

Next: [integration expansion](plans/08-integration-expansion.md) and [delivery, docs, and platform examples](plans/09-delivery-and-examples.md), informed by [current research](research/integration-options.md).

- **Status:** Draft
- **Date:** 2026-10-15
- **Architecture:** [ADR 0001](adr/0001-local-first-session-server.md)

Brain is built in stages: ship a solid local session server first, then prove external checkpoint portability, additional harnesses, external tool packages, and multi-instance scale.

Currently, the local Pi server, TypeScript and Python SDKs, HTTP/SSE tools, and checkpoint archive adapters (filesystem, S3, R2, and PostgreSQL) are implemented and covered by local CI and process-level journey tests. Live cloud qualification, Codex/OpenCode runners, MCP, the broader tool catalog, and load/soak benchmarks are next.

## Release stages

| Stage | Goal | Scope | Verification gate |
| --- | --- | --- | --- |
| **R0: Local preview** | Run and manage local Pi sessions through one server. | Local SQLite metadata, append-only event/native files, session & SSE API, interrupt/resume, HTTP/SSE tools, approval & inspection examples, TypeScript SDK, OpenTelemetry, bounded queues. | End-to-end Pi lifecycle, restart recovery, tool vs. session error separation, tool-wait responsiveness, tenant isolation, SSE reconnect, and overload checks. |
| **R1: Portable state** | Move or recover sessions across servers using external object storage. | S3, R2, and filesystem checkpoint stores, optional PostgreSQL session index, background batching/retries, explicit `flush`/`sync`/`restore`. | PostgreSQL and S3/R2 contract suites, empty-cache restore with preserved prompt context, interrupted upload tests, and archive-outage backpressure tests. |
| **R2: Multi-harness beta** | Choose Pi, OpenCode, or Codex per session through the same API. | Pinned OpenCode and Codex adapters, MCP tool transport, native resume/rewind where supported, and the Python SDK. | Conformance tests verifying next-turn context after restore, input queueing during tool waits, error boundaries, and complete disabling of local bypass tools. |
| **R3: Tool catalog beta** | Attach ready-made storage, web research, browser, and compute tools. | External tool services for S3/R2 buckets, Fly/Modal sandboxes, code execution, browser bridges, and web search/fetch. | Per-service isolation, quota, approval, and cleanup tests against real providers. |
| **R4: Operational release** | Run Brain as a predictable compute unit in a multi-tenant fleet. | Multi-instance load & soak benchmarks, long-session compaction/checkpoint measurements, and handoff/disaster-recovery drills. | All advertised adapters pass credentialed CI, soak suites, and fresh-infrastructure restore drills with published hardware/workload numbers. |

## Feature ownership

| Area | Plan | Target stages |
| --- | --- | --- |
| Server runtime & local state | [01 Local runtime](plans/01-local-runtime.md) | R0 |
| Object checkpoints & PostgreSQL index | [02 External persistence](plans/02-external-persistence.md) | R1 |
| Pi, OpenCode, and Codex runners | [03 Harness adapters](plans/03-harness-adapters.md) | R0 (Pi), R2 (OpenCode, Codex) |
| HTTP/SSE, MCP, and external tool catalog | [04 Distributed tools](plans/04-distributed-tools.md) | R0 (HTTP/SSE), R2 (MCP), R3 (catalog) |
| HTTP contract & TypeScript/Python SDKs | [05 API and SDKs](plans/05-api-and-sdks.md) | R0–R2 |
| Telemetry, limits, Docker, and capacity | [06 Observability and operations](plans/06-observability-and-operations.md) | R0–R4 |
| Test suites & release gates | [07 Verification and release](plans/07-verification-and-release.md) | R0–R4 |
