# ADR-007: Compose sessions with typed extension factories

- Decision date: 2026-08-27
- Status: Accepted
- Compiled: 2026-09-05

## Context

The first public composition API exposed many strings and required callers to track internal identifiers. The user wanted model configuration, a chosen Agentloop, and tools placed in Environments to read like ordinary application code.

## Decision

Provide typed factories and an SDK over the HTTP API. Parse extension options at the authoring boundary, keep model-visible schemas alongside implementations, and let the SDK perform artifact admission and transport bookkeeping. Maintain runnable examples for the public surface.

## Alternatives considered

Raw HTTP remains supported but is not the ergonomic default. Binding spellings such as bind/runIn/useIn were explored and shipped at different points; they are historical API choices, not parallel interfaces to preserve.

## Consequences

The current placed factory spelling is `{ env, ...options }`, and resident tools use `run` without an Environment. [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md) records that final distinction. Pre-1.0 changes replace the interface in place under [ADR-030: Replace pre-1.0 contracts and development data in place](2026-09-04-08-clean-break.md).

## Sources

- Brain implementation/history: [6009a5b](https://github.com/aexhq/brain/commit/6009a5b6fb73e11e8920cdd01fec53818563a048), [8f98ef2](https://github.com/aexhq/brain/commit/8f98ef27ba4c33b96c0381dab3ad7bcf046f9829), [b8f06c9](https://github.com/aexhq/brain/commit/b8f06c9587d5633eb6f3347211731f34d48a447b), [8ad9f2f](https://github.com/aexhq/brain/commit/8ad9f2f50e8d52b74f76ba8860008e3cec80a1b2), [9662144](https://github.com/aexhq/brain/commit/96621447223b2eee107b18d8814d2f91552faf88), [d473e76](https://github.com/aexhq/brain/commit/d473e76842c0e19f74067cefd5aefcab0596ac70), [a21ae6e](https://github.com/aexhq/brain/commit/a21ae6e5c46ea2bffd9fe96aebb45471f6f1ac45).
- Current reference: [packages/brain-sdk/README.md](../../packages/brain-sdk/README.md).
- Current reference: [examples/README.md](../../examples/README.md).
- Current reference: [docs/guides/write-a-tool.mdx](../../docs/guides/write-a-tool.mdx).
- [Codex session `01a03dd3-4217-7cc3-973c-8336841a6a26`](SOURCES.md#session-01a03dd3-4217-7cc3-973c-8336841a6a26), 2026-08-27T08:41:28.153Z: User supplies a typed session-composition sketch and rejects manual string bookkeeping.
- [Codex session `01a06cea-6715-7f33-826c-4ee3ed98b5eb`](SOURCES.md#session-01a06cea-6715-7f33-826c-4ee3ed98b5eb), 2026-09-04T16:30:42.764Z: User separates extension authoring from application configuration and asks for explicit env factory arguments.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
