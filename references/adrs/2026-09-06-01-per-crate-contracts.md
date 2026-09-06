# ADR-042: Each crate renders its own contract into its own generated directory

- Decision date: 2026-09-06
- Status: Accepted
- Compiled: 2026-09-06

## Context

ADR-031 made the Rust types the source of the contracts but rendered them through one central
crate, `brain-contracts`, into one top-level `contracts/` directory. That left three names for
one idea (`brain-protocol`, `brain-contracts`, `contracts/`), a renderer crate that had to
depend on every crate with something to publish, a top-level directory that mixed generated
files with hand-written WIT and examples, and a provider catalog rendered by a Python script
in `tools/` while everything else was rendered by Rust.

## Decision

Every crate that publishes a contract renders it itself, with `cargo run -p <crate> --bin
contract`, into its own `generated/contract/` directory. brain-protocol writes the JSON
Schemas and the code catalogue, brain-http writes the OpenAPI document, brain writes the
provider list from the vendored `catalog/` snapshot alongside its Rust table. The SDK's
generator reads those directories. The hand-written WIT moves to `crates/brain-loophost/wit/`,
the crate that implements those worlds; the hand-written examples move to
`crates/brain-protocol/tests/examples/`. The central crate, the Python script, and the
top-level `contracts/` directory are deleted.

## Alternatives considered

Keeping one renderer crate and moving it under `tools/` changed nothing about the coupling.
Folding the renderer into brain-http kept a crate writing another crate's contract. Keeping a
top-level `contracts/` as the published set was a single browsing location at the cost of a
directory nobody owned.

## Consequences

A contract sits beside the type that defines it, and the directory name says it is output.
Adding a contract means adding a `contract` binary to the crate that owns it and a line to
`npm run gen` and the CI diff guard. The `$id` of every schema follows the new path. Anything
outside this repository that read `contracts/` by path, such as the documentation site's
OpenAPI import, must read the crate path instead.

## Sources

- Current reference: [crates/brain-protocol/src/bin/contract.rs](../../crates/brain-protocol/src/bin/contract.rs).
- Current reference: [crates/brain-http/src/bin/contract.rs](../../crates/brain-http/src/bin/contract.rs).
- Current reference: [crates/brain/src/bin/contract.rs](../../crates/brain/src/bin/contract.rs).
- Current reference: [packages/brain-sdk/scripts/gen.mjs](../../packages/brain-sdk/scripts/gen.mjs).
- Refines: [ADR-031: Generate public contracts from Rust types and route annotations](2026-09-04-09-rust-contracts.md).

[Index](README.md) · [Source coverage and dating](SOURCES.md)
