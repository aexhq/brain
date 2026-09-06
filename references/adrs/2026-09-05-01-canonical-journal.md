# ADR-032: Use one canonical journal and commit before exposing records or effects

- Decision date: 2026-09-05
- Status: Accepted
- Compiled: 2026-09-05

Supersedes: [ADR-009: Replace SQLite with a write-behind segment journal](2026-08-28-01-write-behind.md); [ADR-025: Store transcript state and public Events in two per-session logs](2026-09-04-03-two-logs.md).

## Context

Two canonical logs duplicated ordering/recovery, while the earlier write-behind path could lose an acknowledged effect record on a crash. The user separated transcript, complete journal, public Events, operational logs, and session configuration, and selected local durable commits.

## Decision

Record configuration, transcript deltas, slots, lifecycle, and effect starts/outcomes in one ordered append-only journal per session. Derive status, transcript, and public Events. One process-wide writer assigns sequence numbers and prioritizes ready synchronous work over asynchronous informational writes. A successful canonical commit means the supported local filesystem flush boundary has completed; projections become visible after that boundary.

## Alternatives considered

Two durable logs do not add a second source of truth. Enqueue acknowledgement or process-crash-only persistence can lose acknowledged data on OS/power failure. Requiring an external database or workflow service would undermine the self-contained MVP. A single FIFO would let queued background work delay latency-sensitive commits.

## Consequences

The store contract is SessionStore, implemented on local machine disk. Acknowledged records survive process and OS/power failure when storage honors flushes; device/node loss and replication are outside that promise. Priority applies to ready queued work and cannot preempt an in-flight disk operation. Canonical corruption fails loudly; only an incomplete final write is repaired. [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md) adds a disposable cache, not another durable truth.

## Sources

- Brain implementation/history: [a21ae6e](https://github.com/aexhq/brain/commit/a21ae6e5c46ea2bffd9fe96aebb45471f6f1ac45), [57caae4](https://github.com/aexhq/brain/commit/57caae4f1a0cfc4689462272d7659fd75f60c988), [682ce03](https://github.com/aexhq/brain/commit/682ce03104991f5815de0655165509510f07dbb6).
- Current reference: [docs/concepts/sessions.mdx](../../docs/concepts/sessions.mdx).
- Current reference: [docs/guides/embed.mdx](../../docs/guides/embed.mdx).
- Original decision: “2026-09-05: Standalone, ephemeral execution” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- Original decision: “2026-09-05: One canonical journal, with transcript and Events as projections” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- [Codex session `01a06cea-6715-7f33-826c-4ee3ed98b5eb`](SOURCES.md#session-01a06cea-6715-7f33-826c-4ee3ed98b5eb), 2026-09-04T19:17:52.232Z: User accepts effect-after-commit, one local store interface, and journal-derived transcript/Events.
- [Codex session `01a06cea-6715-7f33-826c-4ee3ed98b5eb`](SOURCES.md#session-01a06cea-6715-7f33-826c-4ee3ed98b5eb), 2026-09-04T20:13:40.128Z: User requires separate sync/async paths, one sequencer, and no duplicated transcript storage.
- [Codex session `01a0705d-503e-7a53-a65f-a1e9ca13e23c`](SOURCES.md#session-01a0705d-503e-7a53-a65f-a1e9ca13e23c), 2026-09-05T08:49:17.093Z: User accepts the proposed process-and-OS/power-failure local guarantee and defers external commit services.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
