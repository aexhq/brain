# ADR-017: Reuse per-session Wasm instances and resident turn context

- Decision date: 2026-09-01
- Status: Superseded
- Compiled: 2026-09-05

Superseded by: [ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md).

## Context

Repeated instantiation, transcript serialization, and slot validation grew with conversation length on every decision. Measurements initially attributed to session count were actually measuring the increasingly long benchmark conversation.

## Decision

Keep a small per-session warm-instance cache and retain context in the worker between decision legs. Send the full context at turn entry and return it at turn completion. Preserve the guest’s own serialized state bytes to avoid defeating equality-based reuse.

## Alternatives considered

Fresh instances on every step repeated work. Increasing concurrency did not remove that cost. The chosen cache improved the then-current step execution path but introduced cache identity, eviction, and resident-state ownership concerns.

## Consequences

[ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md) removes step round trips and resident-context placeholders. [ADR-034: Run native Components in a separate capability-restricted Wasmtime worker](2026-09-05-03-wasm-worker.md) and [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md) reuse compiled/prelinked code with fresh invocation Stores and discard execution by default after a turn. The old benchmark improvements are evidence for the old design only.

## Sources

- Brain implementation/history: [7088f9c](https://github.com/aexhq/brain/commit/7088f9c500cf163b72fca7013d6760f26a56c1cf), [14a3be8](https://github.com/aexhq/brain/commit/14a3be8b686f97cc703046522ec1b37c0f41f04c).
- Current reference: [BENCHMARKS.md](../../BENCHMARKS.md).
- Current reference: [docs/reference/benchmarks.mdx](../../docs/reference/benchmarks.mdx).

[Index](README.md) · [Source coverage and dating](SOURCES.md)
