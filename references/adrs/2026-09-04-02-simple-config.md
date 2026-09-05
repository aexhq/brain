# ADR-024: Use one SessionConfig and keep hashes local to their actual purpose

- Decision date: 2026-09-04
- Status: Accepted
- Compiled: 2026-09-05

## Context

Requested/resolved/sealed variants duplicated nearly identical configuration types. Fingerprints and attachment digests were sent or compared without a meaningful independent consumer.

## Decision

Use one SessionConfig for creation, persistence, and runtime access. Keep the fixed catalogue/binding invariant in the mutation API rather than duplicate sealed types. Call shared executors and limits SessionRuntime. Keep Identity a plain typed value, with HTTP idempotency hashing in the server; remove unused environment fingerprints and hashed attachment IDs.

## Alternatives considered

Two types can express a real state transition, but here they doubled change cost without enforcing an additional invariant. Precomputed equality hashes were not a replacement for comparing the configuration where comparison was actually needed.

## Consequences

Boundary admission and immutable-after-create behavior remain required. Random attachment identifiers, content identities for artifact admission, and durable server claims retain their own meanings. This is not a blanket ban on hashes or validation.

## Sources

- Brain implementation/history: [24aa6af](https://github.com/aexhq/brain/commit/24aa6af9d2605ae84a8a5270fcf2d7b3a944ed67), [bb7fb18](https://github.com/aexhq/brain/commit/bb7fb181ca5e977c0a06f2a9e9da64e2ab5108a9).
- Current reference: [AGENTS.md](../../AGENTS.md).
- Current reference: [docs/guides/embed.mdx](../../docs/guides/embed.mdx).
- Original decision: “2026-09-04: One session configuration, and nothing is sealed” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- Original decision: “2026-09-04: Identity is an idempotency key and nothing else” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- [Claude session `0fc0a00c-3776-4a6c-9383-c5a8fe9bd556`](SOURCES.md#session-0fc0a00c-3776-4a6c-9383-c5a8fe9bd556), 2026-09-03T17:29:42.378Z: User removes duplicate sealed configuration types and unused digests while retaining idempotency as a valid hashing use.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
