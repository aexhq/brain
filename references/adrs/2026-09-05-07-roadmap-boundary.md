# ADR-038: Defer dynamic placement, workflow durability, and advanced tenancy policy

- Decision date: 2026-09-05
- Status: Accepted
- Compiled: 2026-09-05

## Context

The user’s north star includes more capable agent-directed recovery and future hosted platforms, but shipping every policy in the runtime would undermine the minimal MVP.

## Decision

Keep fixed explicit Tool/Environment bindings in the MVP. Defer the entire official tool-env tool, mutable bindings, tools whose placement Brain chooses, between-call suspension, external commit services, clustered ownership/recovery, configurable admission/cache budgets and fairness, and optional stronger worker isolation. Put future work in ROADMAP.md.

## Alternatives considered

A heavy workflow engine or mandatory cloud dependency would constrain every consumer. Treating all future features as rejected would lose the agreed direction. Shipping inspection alone as the tool-env MVP would contradict the final scope correction.

## Consequences

The deferral is accepted; the future APIs are not implemented contracts. The agreed direction for mutable bindings is that committed changes affect subsequent calls, including in the same turn, while in-flight calls keep their original target and all changes remain within granted authority. Existing correctness and capability checks stay in force. Other roadmap items, including multimodal input, native subagent links, export/import, file sync, and stable v1 publication, remain proposals until separately decided.

## Sources

- Brain implementation/history: [682ce03](https://github.com/aexhq/brain/commit/682ce03104991f5815de0655165509510f07dbb6), [c3c0dc5](https://github.com/aexhq/brain/commit/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170).
- Current reference: [ROADMAP.md](../../ROADMAP.md).
- Original decision: “2026-09-05: Standalone, ephemeral execution” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- [Codex session `01a0705d-503e-7a53-a65f-a1e9ca13e23c`](SOURCES.md#session-01a0705d-503e-7a53-a65f-a1e9ca13e23c), 2026-09-05T09:01:13.831Z: User accepts dynamic bindings and optional automatic placement as future work.
- [Codex session `01a0705d-503e-7a53-a65f-a1e9ca13e23c`](SOURCES.md#session-01a0705d-503e-7a53-a65f-a1e9ca13e23c), 2026-09-05T09:05:01.771Z: User explicitly defers the whole tool-env tool and retains required MVP bindings.
- [Codex session `01a0705d-503e-7a53-a65f-a1e9ca13e23c`](SOURCES.md#session-01a0705d-503e-7a53-a65f-a1e9ca13e23c), 2026-09-05T14:17:10.872Z: User moves roadmap content to ROADMAP.md and selects the current product headline.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
