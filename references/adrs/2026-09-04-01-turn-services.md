# ADR-023: Give the Agentloop one whole turn and asynchronous Brain services

- Decision date: 2026-09-04
- Status: Accepted
- Compiled: 2026-09-05

Supersedes: [ADR-002: Execute replaceable Agentloops through an isolated step contract](2026-08-21-01-step-loops.md); [ADR-017: Reuse per-session Wasm instances and resident turn context](2026-09-01-02-warm-instances.md).

## Context

A loop that only returned one decision at a time could not naturally choose sequential or parallel tool execution, skip a tool, perform its own retry policy, or call another API for compaction. Step-shaped execution also created repeated context transfers.

## Decision

One activation is one whole turn. The loop owns transcript content and calls Brain’s model, dispatch, emit, and telemetry services; the later events import pages committed observations. It returns its final transcript and JSON slots. Brain owns persistence and effect execution. Use asynchronous host imports so I/O suspends guest execution without consuming computation fuel.

## Alternatives considered

Retaining the step actor would keep policy in Brain. Adding many special compaction/model hooks would enlarge the learning surface. A CPU deadline that also counts suspended I/O would fail valid long model or tool waits.

## Consequences

Remove step observations/decisions, resident-context placeholders, decision caps, and actor model-request defaulting. Bound guest work with fuel and a turn independently with model-call and wall-time budgets. The loop controls retries as new explicit calls; the runtime never automatically retries an effect. The pre-1.0 contract remains named agentloop/v1.

## Sources

- Brain implementation/history: [6d20d59](https://github.com/aexhq/brain/commit/6d20d59421f1ba137eeea6c471f44afffc758ceb), [bb7fb18](https://github.com/aexhq/brain/commit/bb7fb181ca5e977c0a06f2a9e9da64e2ab5108a9), [e853046](https://github.com/aexhq/brain/commit/e853046e7b4a63f9fddafe3069f7e08140a4509f), [a21ae6e](https://github.com/aexhq/brain/commit/a21ae6e5c46ea2bffd9fe96aebb45471f6f1ac45), [682ce03](https://github.com/aexhq/brain/commit/682ce03104991f5815de0655165509510f07dbb6).
- Current reference: [docs/concepts/agent-loop.mdx](../../docs/concepts/agent-loop.mdx).
- Current reference: [crates/brain-loophost/wit/agentloop/agentloop.wit](../../crates/brain-loophost/wit/agentloop/agentloop.wit).
- Original decision: “2026-09-04: The agent loop drives the turn; Brain provides services” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- [Claude session `0fc0a00c-3776-4a6c-9383-c5a8fe9bd556`](SOURCES.md#session-0fc0a00c-3776-4a6c-9383-c5a8fe9bd556), 2026-09-03T16:40:17.639Z: User requests that the loop control dispatch and compaction while Brain provides services and records changes.
- [Claude session `0fc0a00c-3776-4a6c-9383-c5a8fe9bd556`](SOURCES.md#session-0fc0a00c-3776-4a6c-9383-c5a8fe9bd556), 2026-09-03T23:13:37.844Z: User accepts trying asynchronous imports and retains a Brain-understood transcript format.
- [Claude session `eb81e1ec-79e7-490f-9797-a836946f8d2f`](SOURCES.md#session-eb81e1ec-79e7-490f-9797-a836946f8d2f), 2026-09-04T13:28:22.843Z: User highlights compaction calls that can wait seconds or minutes.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
