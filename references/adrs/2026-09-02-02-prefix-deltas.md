# ADR-019: Record transcript changes as common-prefix deltas

- Decision date: 2026-09-02
- Status: Accepted
- Compiled: 2026-09-05

## Context

The old model-intent writer saved only messages beyond the previous count. A loop that rewrote, summarized, or reordered an earlier message left the actual request missing from the log; a request hash could detect disagreement without reconstructing the missing data.

## Decision

Compare the new transcript with the last recorded one, retain its unchanged prefix, and append the replacement tail. A reader applies “keep k, append rest” in journal order. Persist changes at model calls and turn end without rewriting older journal records.

## Alternatives considered

Full snapshots on every call preserve correctness but amplify writes. Append-count-only deltas handle only monotonically appended transcripts. A hash is not a substitute for the data needed to reconstruct a rewritten request.

## Consequences

Ordinary appends remain small; a rewrite near the beginning can legitimately record most of the transcript again. Linear growth applies to append-oriented workloads, not a promise that arbitrary full rewrites are free. [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md) places these changes in the single canonical record stream.

## Sources

- Brain implementation/history: [6dbf517](https://github.com/aexhq/brain/commit/6dbf517f319414bbec1145a3badb145db4b12a8f), [3262030](https://github.com/aexhq/brain/commit/3262030c45b338febfcc61c69960b47e8490fbd2), [a21ae6e](https://github.com/aexhq/brain/commit/a21ae6e5c46ea2bffd9fe96aebb45471f6f1ac45).
- Current reference: [docs/concepts/sessions.mdx](../../docs/concepts/sessions.mdx).
- Current reference: [crates/brain/tests/journal_growth.rs](../../crates/brain/tests/journal_growth.rs).
- Original decision: “2026-09-02: The journal records a model request as a diff against the last one” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- [Claude session `56115bda-1ba4-40dc-8c47-28b8e6273e24`](SOURCES.md#session-56115bda-1ba4-40dc-8c47-28b8e6273e24), 2026-09-02T16:10:09.940Z: User asks to write from the first changed position while preserving append-only history.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
