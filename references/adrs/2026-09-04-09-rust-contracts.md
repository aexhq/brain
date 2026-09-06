# ADR-031: Generate public contracts from Rust types and route annotations

- Decision date: 2026-09-04
- Status: Accepted
- Compiled: 2026-09-05

## Context

The schema-to-Rust pipeline had become unused, leaving handwritten Rust and handwritten JSON Schema/OpenAPI in parallel. Contract digests had no admission consumer and did not solve that duplication.

## Decision

Make brain-protocol types and brain-http utoipa route annotations the handwritten source. Generate schemas, OpenAPI, and codes with brain-contracts, then generate SDK views. Keep WIT and contract examples hand-authored as documented exceptions. Regenerate and diff in CI.

## Alternatives considered

Generating Rust from JSON Schema had produced types without validation behavior already implemented in Rust. Letting each crate publish independent wire definitions would recreate duplicate boundary types. Hashing generated files without an actual consumer did not establish correctness.

## Consequences

Change the defining type or annotation, run npm run gen, and commit generated outputs together. Examples verify conformance. Runtime invariants stay in Rust rather than being inferred from examples alone. The old decision states that this conversion preserved the existing wire.

## Sources

- Brain implementation/history: [3437a0a](https://github.com/aexhq/brain/commit/3437a0a6b4da6f755ab769b940ffdfad65792af0), [09bb382](https://github.com/aexhq/brain/commit/09bb3822289d95f6f81aa97a14c9c2350c3c1412).
- Current reference: [AGENTS.md](../../AGENTS.md).
- Current reference: [CONTRIBUTING.md](../../CONTRIBUTING.md).
- Current reference: [crates/brain-protocol/src/bin/contract.rs](../../crates/brain-protocol/src/bin/contract.rs).
- Original decision: “2026-09-04: The Rust types are the source of the contracts” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- [Claude session `754faa6b-5689-4155-a619-39f11a52bf6e`](SOURCES.md#session-754faa6b-5689-4155-a619-39f11a52bf6e), 2026-09-04T12:17:22.335Z: User asks why handwritten protocol duplicates and unused digests exist.
- [Claude session `754faa6b-5689-4155-a619-39f11a52bf6e`](SOURCES.md#session-754faa6b-5689-4155-a619-39f11a52bf6e), 2026-09-04T12:36:50.480Z: User explicitly chooses Rust-first generation and concise AGENTS.md guidance.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
