# ADR-009: Replace SQLite with a write-behind segment journal

- Decision date: 2026-08-28
- Status: Superseded
- Compiled: 2026-09-05

Superseded by: [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md).

## Context

The SQLite path scanned prior records for a maximum timestamp during each append, reread and hashed the database at startup, and serialized work through a shared connection. Brain’s access pattern was ordered append and range lookup rather than relational queries.

## Decision

At this stage, replace SQLite with an append-only framed segment log and a writer thread. Keep indexes and current state in memory, use frame-local integrity checks, and acknowledge queued appends without filesystem synchronization.

## Alternatives considered

Fjall and other databases were considered in the session, but changing database alone would not remove unnecessary scans and duplicated context. A simple log matched the access pattern. The explicit performance tradeoff at the time was to leave durability above Brain.

## Consequences

The log design persisted, but the no-fsync and effect-without-commit semantics are superseded by [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md) and [ADR-035: Send each effect once and expose failures to the Agentloop](2026-09-05-04-send-once.md). The throughput figures in the original commit measure enqueue/write-behind work and cannot describe current durable commit latency. Shared segments were also replaced by per-session storage.

## Sources

- Brain implementation/history: [f170ad6](https://github.com/aexhq/brain/commit/f170ad64ef6d0c1f08c27a42751fbd995677f783), [3b6038e](https://github.com/aexhq/brain/commit/3b6038e04eee124ce51fb757d4e04cb8ee319e03).
- Current reference: [BENCHMARKS.md](../../BENCHMARKS.md).
- Current reference: [docs/reference/benchmarks.mdx](../../docs/reference/benchmarks.mdx).
- [Claude session `f90a511b-6b7b-4e96-a762-5cb6a5101d0a`](SOURCES.md#session-f90a511b-6b7b-4e96-a762-5cb6a5101d0a), 2026-08-28T00:42:52.056Z: User explicitly chooses performance over the old durability fence during the append-only storage review.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
