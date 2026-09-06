# ADR-021: Make the core runtime one session and let the server manage sessions

- Decision date: 2026-09-02
- Status: Accepted
- Compiled: 2026-09-05

## Context

The old Kernel combined a session registry, local journal construction, HTTP retry metadata, and per-session turn execution. That made embedding carry server-wide policy and state.

## Decision

Construct one Session from its ID, store, configuration, and injected executors. The core manages that session’s ordered execution. The server owns the session map, listing, loading, credentials, authorization, HTTP idempotency, and process lifecycle. Name the runtime object Session rather than Kernel.

## Alternatives considered

Keeping an all-session Kernel saves constructor plumbing but makes storage choice, admission, and HTTP policy implicit dependencies of every session. A separate ID-adoption/recovery protocol is unnecessary when dependencies can be supplied directly.

## Consequences

Sessions can be embedded and tested independently. The stock server still coordinates one mutable owner per local session and locks its data directory. Cross-server ownership is not supplied by this boundary; [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md) keeps it outside the local MVP.

## Sources

- Brain implementation/history: [6dbf517](https://github.com/aexhq/brain/commit/6dbf517f319414bbec1145a3badb145db4b12a8f), [3262030](https://github.com/aexhq/brain/commit/3262030c45b338febfcc61c69960b47e8490fbd2), [682ce03](https://github.com/aexhq/brain/commit/682ce03104991f5815de0655165509510f07dbb6).
- Current reference: [docs/guides/embed.mdx](../../docs/guides/embed.mdx).
- Original decision: “2026-09-02: The kernel is one session; the server manages sessions” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- [Claude session `56115bda-1ba4-40dc-8c47-28b8e6273e24`](SOURCES.md#session-56115bda-1ba4-40dc-8c47-28b8e6273e24), 2026-09-02T17:29:24.403Z: User defines the per-session/server split and requires dependency injection.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
