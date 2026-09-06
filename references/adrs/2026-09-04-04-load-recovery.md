# ADR-026: Recover saved data by loading it, without replaying the Agentloop

- Decision date: 2026-09-04
- Status: Accepted
- Compiled: 2026-09-05

## Context

Resident worker context and special restored-history delivery required the loop to reconstruct state on restart. That could lose context or tempt recovery to execute prior effects again.

## Decision

Persist the neutral transcript and returned slots. Inject the store, SessionRuntime, and SessionConfig when constructing or opening a session. Load recorded state directly. Mark an unfinished turn interrupted, and deliver the committed observations to a later explicit activation. Seed a new conversation with transcript messages instead of replayable historical events.

## Alternatives considered

A restored-history callback makes each loop responsible for recovery correctness. Re-executing steps cannot safely reconstruct effects that may already have happened. Whole guest memory snapshots expand the persistence contract unnecessarily.

## Consequences

The loop does not run merely because data was opened. [ADR-036: Preserve request claims and effect uncertainty across interruption](2026-09-05-05-interruption.md) covers incomplete creation/ending and uncertain effects; [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md) makes history reads independent from live execution and uses a disposable checkpoint for speed.

## Sources

- Brain implementation/history: [3262030](https://github.com/aexhq/brain/commit/3262030c45b338febfcc61c69960b47e8490fbd2), [bb7fb18](https://github.com/aexhq/brain/commit/bb7fb181ca5e977c0a06f2a9e9da64e2ab5108a9), [57caae4](https://github.com/aexhq/brain/commit/57caae4f1a0cfc4689462272d7659fd75f60c988), [682ce03](https://github.com/aexhq/brain/commit/682ce03104991f5815de0655165509510f07dbb6).
- Current reference: [docs/concepts/sessions.mdx](../../docs/concepts/sessions.mdx).
- Current reference: [docs/guides/embed.mdx](../../docs/guides/embed.mdx).
- Original decision: “2026-09-04: Every dependency of a session is injected, and recovery is load and construct” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- [Claude session `0fc0a00c-3776-4a6c-9383-c5a8fe9bd556`](SOURCES.md#session-0fc0a00c-3776-4a6c-9383-c5a8fe9bd556), 2026-09-03T16:40:17.639Z: User asks for recovery to be loading persisted data and constructing a session with injected dependencies.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
