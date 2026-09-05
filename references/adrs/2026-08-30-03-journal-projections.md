# ADR-014: Rebuild session views from recorded history

- Decision date: 2026-08-30
- Status: Accepted
- Compiled: 2026-09-05

## Context

Separate rewritten session state and event storage duplicated information and allowed reconstruction to diverge from recorded execution. Full-context persistence also carried the growth costs in [ADR-010: Bound journal growth and measure the complete execution path](2026-08-28-02-growth.md).

## Decision

Make the journal the session source of truth, with state and event indexes reconstructed from it. Keep live token deltas transient and persist the completed model result. Avoid persisting another full transcript for each model call.

## Alternatives considered

A mutable state file is convenient for reads but creates a second persistence path. Recording complete context every time makes history unnecessarily large. Treating transient token delivery as durable history promises replay the feed cannot provide.

## Consequences

This early implementation used append-tail assumptions and did not yet satisfy the final context recovery contract. [ADR-019: Record transcript changes as common-prefix deltas](2026-09-02-02-prefix-deltas.md) handles arbitrary edits, [ADR-025: Store transcript state and public Events in two per-session logs](2026-09-04-03-two-logs.md) records a temporary split, and [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md) is the final single-journal design. The architectural intent here must not be read as proof that all August 30 recovery behavior was correct.

## Sources

- Brain implementation/history: [bc14d8a](https://github.com/aexhq/brain/commit/bc14d8a5cde36aaa95017abe7872a35042634dfd), [52cef5f](https://github.com/aexhq/brain/commit/52cef5fdaed4e5d655faeade757209cb8e360d18).
- Current reference: [docs/concepts/sessions.mdx](../../docs/concepts/sessions.mdx).
- Current reference: [BENCHMARKS.md](../../BENCHMARKS.md).
- [Codex session `01a055b3-dee6-7362-b8a0-e244cd46b3c4`](SOURCES.md#session-01a055b3-dee6-7362-b8a0-e244cd46b3c4), 2026-08-31T02:46:07.379Z: User asks to distinguish in-memory context, append-only journal, and asynchronous publication.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
