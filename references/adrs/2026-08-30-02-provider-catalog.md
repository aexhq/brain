# ADR-013: Make compatible providers reviewed deployment data

- Decision date: 2026-08-30
- Status: Accepted
- Compiled: 2026-09-05

## Context

The registry, schema, and SDK duplicated a small fixed provider list. Each added provider required coordinated protocol edits even when it spoke an existing dialect.

## Decision

Vendor a models.dev snapshot and generate the supported registry and SDK names. Normalize catalog rows, curated defaults, operator provider files, and URL overrides into one provider definition. Refresh the catalog explicitly with releases. Accept syntactically valid unknown model IDs for registered providers; use known metadata when available.

## Alternatives considered

Runtime fetching would make behavior depend on an unreviewed external change. Enumerating provider names in the wire contract would couple catalog updates to protocol churn. Catalog-only model admission would block newly released or private models.

## Consequences

Adding a compatible endpoint is configuration; a new wire dialect still requires implementation. Server-side registry admission remains authoritative. Do not freeze the provider count in an ADR: it is snapshot data.

## Sources

- Brain implementation/history: [a19e4bc](https://github.com/aexhq/brain/commit/a19e4bca8c0ed923f8b92f7a9105eadbd458689e).
- Current reference: [docs/concepts/model.mdx](../../docs/concepts/model.mdx).
- Current reference: [crates/brain/src/bin/contract.rs](../../crates/brain/src/bin/contract.rs).
- Current reference: [tools/fetch-models-dev.mjs](../../tools/fetch-models-dev.mjs).
- [Claude session `b4871cce-1706-4bb3-8140-531592e60799`](SOURCES.md#session-b4871cce-1706-4bb3-8140-531592e60799), 2026-08-30T20:24:52.580Z: User asks for unified provider support primarily through configuration.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
