# ADR-020: Name effects by session and sequence, with one lifecycle vocabulary

- Decision date: 2026-09-02
- Status: Accepted
- Compiled: 2026-09-05

## Context

journal_id, operation_id, request_identity, and separate journal metadata named or checked information already available from a session and its ordered records. A hidden journal salt even made a restored session readable but unable to execute when the extra metadata was missing.

## Decision

Identify a record by `(session_id, sequence)` and an effect by its started record’s sequence. Use `started`, `ended`, and `failed` consistently across lifecycle/effects. Represent uncertainty in the outcome/failure rather than a separate family of ambiguous record names. Remove request_identity and the journal/operation allocator concepts.

## Alternatives considered

A second ID does not improve correlation where the existing sequence already names the effect. A request digest had no consumer that needed it once prefix deltas preserved complete data. HTTP idempotency still compares request bodies in the server store; it is a different concern.

## Consequences

Recovery can reconstruct record identity from the journal and session ID. This decision does not remove content-addressed artifact admission, credential token hashes, or storage-integrity checks, each of which crosses a different boundary. [ADR-024: Use one SessionConfig and keep hashes local to their actual purpose](2026-09-04-02-simple-config.md) and [ADR-036: Preserve request claims and effect uncertainty across interruption](2026-09-05-05-interruption.md) specify those remaining uses.

## Sources

- Brain implementation/history: [6dbf517](https://github.com/aexhq/brain/commit/6dbf517f319414bbec1145a3badb145db4b12a8f).
- Current reference: [docs/concepts/sessions.mdx](../../docs/concepts/sessions.mdx).
- Original decision: “2026-09-02: Effect records are named for what happened, not for intent” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- Original decision: “2026-09-02: No request identity” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- Original decision: “2026-09-02: A session has two ids, `session_id` and `sequence`” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- [Claude session `56115bda-1ba4-40dc-8c47-28b8e6273e24`](SOURCES.md#session-56115bda-1ba4-40dc-8c47-28b8e6273e24), 2026-09-02T16:16:24.467Z: User rejects intent/request-identity vocabulary and asks why another operation ID exists.
- [Claude session `56115bda-1ba4-40dc-8c47-28b8e6273e24`](SOURCES.md#session-56115bda-1ba4-40dc-8c47-28b8e6273e24), 2026-09-02T17:26:04.688Z: User selects session_id and sequence consistently and expects reconstruction from the journal.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
