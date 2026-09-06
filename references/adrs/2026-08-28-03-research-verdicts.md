# ADR-011: Keep measured performance verdicts separate from hypotheses

- Decision date: 2026-08-28
- Status: Accepted
- Compiled: 2026-09-05

## Context

The independent Claude performance branch produced a source review and then a verdict scoreboard. Several intuitive optimizations were disproved, and Windows-only microbenchmarks could not establish real Linux worker behavior.

## Decision

Retain the distinction between confirmed, rejected, and unmeasured findings. Fix demonstrated repeated work: reuse HTTP clients, make SSE scanning linear, avoid repeated serialization/copies and lock-table sweeps, and remove context-growth amplification. Do not adopt an optimization merely because its mechanism sounds plausible.

## Alternatives considered

The reviewed two-pass replay was tried and reverted because it still needed earlier configuration/identity frames and added seeks. InstancePre/pooling did not improve the tested large JavaScript guest’s Windows instantiation; disabling telemetry and changing cancellation polling showed no useful throughput win. The predicted 1.5–2× fuel cost was not supported. These are bounded experimental conclusions, not permanent rejections of those mechanisms.

## Consequences

The branch-only PERF-REVIEW.md contains hypotheses; VERDICTS.md at b5e08ed contains their reconciliation, with explicit host and measurement limits. Neither is a current outstanding-bug list. Landed work is traced by [ADR-010: Bound journal growth and measure the complete execution path](2026-08-28-02-growth.md), later context ownership by [ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md), and current prelink caching by [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md). Raw results are not republished as current production numbers.

## Sources

- Brain implementation/history: [c1a7ac0](https://github.com/aexhq/brain/commit/c1a7ac080078fb8cd1b31fddf919f83d029faa73), [70262a7](https://github.com/aexhq/brain/commit/70262a7d7e6b7a0c07f1a2ab397baa5ce3e86578), [3b6038e](https://github.com/aexhq/brain/commit/3b6038e04eee124ce51fb757d4e04cb8ee319e03).
- Current reference: [docs/reference/benchmarks.mdx](../../docs/reference/benchmarks.mdx).
- [Claude session `e3e5d8ee-bc63-4389-b0bd-333c904870ba`](SOURCES.md#session-e3e5d8ee-bc63-4389-b0bd-333c904870ba), 2026-08-28T01:54:15.502Z: Claude independent performance investigation.
- [Claude session `e47f469f-637e-43e2-b35f-4acb7506f708`](SOURCES.md#session-e47f469f-637e-43e2-b35f-4acb7506f708), 2026-08-28T07:57:43.639Z: User requests reconciliation with concurrent Codex and journal optimization sessions.
- Historical branch source: [PERF-REVIEW.md at 374bae2](https://github.com/aexhq/brain/blob/374bae29cfd5a72d55db237d22d96acec520ff72/PERF-REVIEW.md); also recoverable with `git show 374bae2:PERF-REVIEW.md`.
- Historical branch source: [VERDICTS.md at b5e08ed](https://github.com/aexhq/brain/blob/b5e08ed4d6526e7a1eb45ecd11fee99b81f44ff2/VERDICTS.md); also recoverable with `git show b5e08ed:VERDICTS.md`.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
