# ADR-003: Replace the default sandbox with explicit Environment bindings

- Decision date: 2026-08-24
- Status: Accepted
- Compiled: 2026-09-05

Amended by: [ADR-041: Run every Tool in an Environment that implements one protocol, including Brain's own](2026-09-05-10-one-execution-model.md), which places Tools held by the application in the host env rather than treating them as a second execution path.

## Context

Tools were coupled to a default Hand/sandbox and the core carried product-specific tools and inventory. Browsers, remote services, and other execution providers did not fit that assumption.

## Decision

Treat execution as an Environment contract. Require explicit composition, leave resource inventory and physical execution to adapters, and remove product-specific package surfaces and the built-in subagent tool from Brain.

## Alternatives considered

Keeping a default sandbox or treating every Environment as a Hand would make one execution provider special. Keeping built-in product tools would bypass the extension boundary.

## Consequences

Brain checks and routes tools without owning their implementation. Current resident tools require no synthetic Environment; placed tools keep fixed bindings under [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md). Child sessions remain the composition model, while native parent/child support is still roadmap work.

## Sources

- Brain implementation/history: [6a5c506](https://github.com/aexhq/brain/commit/6a5c5063f532f85c90f0bb9b6473ecc081086a79), [8c007aa](https://github.com/aexhq/brain/commit/8c007aa94344516e1975c488a4e7b110d9e01aee), [2017073](https://github.com/aexhq/brain/commit/2017073cfc097477c63d13216cdc84468d00d1d9).
- Current reference: [docs/concepts/environment.mdx](../../docs/concepts/environment.mdx).
- Current reference: [docs/guides/subagents.mdx](../../docs/guides/subagents.mdx).
- Current reference: [ROADMAP.md](../../ROADMAP.md).
- [Codex session `01a02b95-b874-7a80-adcb-8ada7196900c`](SOURCES.md#session-01a02b95-b874-7a80-adcb-8ada7196900c), 2026-08-23T15:47:25.295Z: Brain-related part of the architecture discussion replaces Hand terminology with Environment extensions.
- [Codex session `01a035ae-d35e-7491-aec2-32a225a91722`](SOURCES.md#session-01a035ae-d35e-7491-aec2-32a225a91722), 2026-08-24T21:31:50.297Z: User clarifies that Brain is independently deployable and tools execute in their chosen Environment.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
