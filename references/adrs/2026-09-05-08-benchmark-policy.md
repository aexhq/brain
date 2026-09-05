# ADR-039: Keep performance claims historical until representative baselines are rebuilt

- Decision date: 2026-09-05
- Status: Accepted
- Compiled: 2026-09-05

## Context

The old benchmark numbers measured different execution and storage designs. A 10,000-request RSS guard was being described as a current performance gate, while turn-end passivation and the new worker/cache design changed the workload.

## Decision

Retain benchmark provenance with its measured revision and limitations, but remove stale headline claims and defer benchmark/leakage thresholds during pre-launch iteration. Keep correctness gates, including journal-growth shape, history without activation, lazy provider failures, native Tool capacity under parent load, and real SDK journeys. Run independent journey suites in parallel.

## Alternatives considered

Old enqueue latency cannot stand in for durable commit latency. An empty-server RSS limit cannot establish retained-session density. Refusals and unsupported measurements must not become successful results. Removing performance thresholds does not justify skipping correctness tests.

## Consequences

Measure admission, prepared creation, first/resumed activation, cold readiness, and history reads separately, including the server and worker. Historical comparisons against Pi, Codex, and OpenCode remain evidence for their exact runs only. BENCHMARKS.md is historical evidence; current policy is the workflow and docs/reference/benchmarks.mdx. A documentation-only ADR compilation does not re-run or validate those historical experiments.

## Sources

- Brain implementation/history: [04733b0](https://github.com/aexhq/brain/commit/04733b087e54f5e9e97ebba62f9ff58c6ffa4686), [a8557e0](https://github.com/aexhq/brain/commit/a8557e02ecfce84b578cbcd44d9af85aac7b6e24), [d897a1f](https://github.com/aexhq/brain/commit/d897a1f57d91692107a2314086712b78b019dd52), [074d02e](https://github.com/aexhq/brain/commit/074d02e345e1d29e22c7a11b20ff78881d4a5e80), [2da47f5](https://github.com/aexhq/brain/commit/2da47f5424299c31bfc87537703ef0c13af45f01), [c3c0dc5](https://github.com/aexhq/brain/commit/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170).
- Current reference: [BENCHMARKS.md](../../BENCHMARKS.md).
- Current reference: [tools/bench/README.md](../../tools/bench/README.md).
- Current reference: [docs/reference/benchmarks.mdx](../../docs/reference/benchmarks.mdx).
- Current reference: [tests/journeys/README.md](../../tests/journeys/README.md).
- Current reference: [.github/workflows/ci.yml](../../.github/workflows/ci.yml).
- [Claude session `f39792fc-dc4f-46cc-9a96-f985a092c48d`](SOURCES.md#session-f39792fc-dc4f-46cc-9a96-f985a092c48d), 2026-08-28T00:05:58.393Z: User requests competitor coverage across performance dimensions.
- [Claude session `b8149887-7a5f-45fc-a05a-d0a4816508d5`](SOURCES.md#session-b8149887-7a5f-45fc-a05a-d0a4816508d5), 2026-09-03T23:45:13.084Z: User requests real Pi/OpenCode/Codex comparisons.
- [Codex session `01a0705d-503e-7a53-a65f-a1e9ca13e23c`](SOURCES.md#session-01a0705d-503e-7a53-a65f-a1e9ca13e23c), 2026-09-05T11:24:41.570Z: User explicitly defers benchmark leakage checks during rapid iteration.
- [Codex session `01a0705d-503e-7a53-a65f-a1e9ca13e23c`](SOURCES.md#session-01a0705d-503e-7a53-a65f-a1e9ca13e23c), 2026-09-05T13:25:53.678Z: User requests regular-user SDK journeys with parallel CI execution.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
