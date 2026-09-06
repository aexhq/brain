# ADR-028: Expose independently created and shared Environment resources

- Decision date: 2026-09-04
- Status: Superseded
- Compiled: 2026-09-05

Superseded by: [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md).

## Context

An execution resource can outlive a conversation, and several sessions may logically need the same browser, service, or sandbox. Earlier APIs mixed declarations, attachment, and ownership.

## Decision

The September 4 implementation introduced independently created Environment IDs, attach-by-ID, explicit close, and an optional Brain-managed idle TTL. It refused closing an attached Environment and reported closure/unreachability as Events.

## Alternatives considered

Inline declarations alone did not express an independently owned shared resource. Automatic creation from an ID obscured lifecycle ownership. A managed flag tried to distinguish caller-owned and Brain-owned lifetimes.

## Consequences

[ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md) removes the standalone Environment API for the MVP because it could not truthfully promise cross-session attachment. Declarations now belong to one session, while providers may wrap externally owned resources. Lazy allocation and resource TTL belong to providers under [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md). Shared ownership remains a future contract, not an implied capability.

## Sources

- Brain implementation/history: [dc0ac55](https://github.com/aexhq/brain/commit/dc0ac55e3667ab6aa76d1d19fa2bab363ee3f79c), [bb7fb18](https://github.com/aexhq/brain/commit/bb7fb181ca5e977c0a06f2a9e9da64e2ab5108a9), [a21ae6e](https://github.com/aexhq/brain/commit/a21ae6e5c46ea2bffd9fe96aebb45471f6f1ac45).
- Current reference: [docs/concepts/environment.mdx](../../docs/concepts/environment.mdx).
- Current reference: [ROADMAP.md](../../ROADMAP.md).
- Original decision: “2026-09-04: Environments are resources with an optional managed lifecycle” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).

[Index](README.md) · [Source coverage and dating](SOURCES.md)
