# 06: Observability and operations plan

- **Releases:** Telemetry, resource limits, and Docker packaging at R0–R1; fleet benchmarks and recovery drills at R4 ([Roadmap](../roadmap.md))

Run Brain as a predictable, bounded process with built-in structured logging, OpenTelemetry traces and metrics, health endpoints, and explicit handoff steps.

## Work items

| ID | Scope | Acceptance criteria |
| --- | --- | --- |
| **O1: Telemetry** | Structured Go JSON logs (`log/slog`) and OpenTelemetry (`OTEL_EXPORTER_OTLP_ENDPOINT`) traces and metrics across HTTP, session lifecycle, runners, tools, and storage sync. | Request and trace IDs propagate through tool calls; metrics avoid high-cardinality session/tenant labels; broken OTLP collectors never stall turns; logs never include secrets or request bodies. |
| **O2: Resource bounds** | Enforce `active`, `loaded`, `queue`, `input_bytes`, `session_bytes`, `subscribers`, `subscriber_events`, and `subscriber_bytes` limits, plus `/health/live` and `/health/ready`. | Saturation returns `429` (`Retry-After: 1`) while reserving capacity for in-flight tool completions and interrupts; full sync queues mark `/health/ready` as `503`. |
| **O3: Packaging** | Reproducible Go + Pi runner container (`Dockerfile`) running as an unprivileged user, verified by `scripts/container-smoke.mjs`. | Container builds and passes the end-to-end smoke test in CI without external model credentials. |
| **O4: Capacity benchmarks** | Measure coordinator overhead, runner memory footprint, tool throughput, SSE fan-out, and checkpoint sync across long sessions and multi-instance fleets. | Published capacity reports state exact hardware, workload shape, queue/disk settings, and latency/memory percentiles. |
| **O5: Handoff & recovery drills** | Document and test graceful shutdown, planned `flush`/`restore` handoffs, empty-cache recovery, and archive outage recovery. | See [Persistence and recovery](../running-persistence.md) for the operator runbook. |
