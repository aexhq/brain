# ADR-012: Store provider-neutral messages and normalize model transports

- Decision date: 2026-08-30
- Status: Accepted
- Compiled: 2026-09-05

## Context

OpenAI-shaped JSON had leaked into the guest contract and journal, while the server was effectively tied to one gateway. Adding providers only at the last HTTP call would leave persisted history provider-specific.

## Decision

Use typed neutral messages and content blocks throughout the protocol and journal, and render provider wire formats at request construction. Normalize model results, stop reasons, stream events, errors, and optional usage counters. Preserve missing usage as missing; reject malformed tool JSON instead of coercing it.

## Alternatives considered

A generic string message or raw provider JSON keeps provider quirks in every loop. A registry of dozens of bespoke implementations duplicates compatible wire handling. Two implemented dialects plus data-driven provider definitions cover the supported catalog.

## Consequences

A model binding remains fixed for a session, while presentation is loop policy under [ADR-018: Let the Agentloop control model presentation within fixed authority](2026-09-02-01-presentation.md). The original normalization change also introduced retry/backoff for selected provider errors; that behavior is superseded by [ADR-035: Send each effect once and expose failures to the Agentloop](2026-09-05-04-send-once.md). Neutrality does not imply support for every provider dialect or modality.

## Sources

- Brain implementation/history: [e4664ef](https://github.com/aexhq/brain/commit/e4664ef0ed90112723dcbb18007feb7a914fb98a).
- Current reference: [docs/concepts/model.mdx](../../docs/concepts/model.mdx).
- Current reference: [crates/brain-protocol/src/model.rs](../../crates/brain-protocol/src/model.rs).
- [Claude session `7074c005-e236-4d1a-8671-1ddfbeb1a27c`](SOURCES.md#session-7074c005-e236-4d1a-8671-1ddfbeb1a27c), 2026-08-28T15:48:12.216Z: User requests provider normalization after comparing Brain with ZeroClaw.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
