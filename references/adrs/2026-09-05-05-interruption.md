# ADR-036: Preserve request claims and effect uncertainty across interruption

- Decision date: 2026-09-05
- Status: Accepted
- Compiled: 2026-09-05

Tool stop causes clarified by [ADR-047](2026-09-12-01-tool-outcomes.md): invocation deadlines and
explicit cancellation have known terminal causes; missing results after dispatch remain unknown.

Amended by: [ADR-040: Identify records by session and sequence, and everything inside a session by name](2026-09-05-09-session-names.md) and [ADR-041: Run every Tool in an Environment that implements one protocol, including Brain's own](2026-09-05-10-one-execution-model.md): model credentials are sealed under the session id rather than a binding identity, and host token hashes belong to the host env.

## Context

Restart could lose HTTP claims or resident-host identity, and cancellation/ending could race remote effects or teardown. Reissuing a claimed request or forgetting a started effect would break send-once semantics.

## Decision

Persist HTTP claims and resident token hashes in small server metadata logs. Keep lifecycle/effect outcomes in the session journal. A claimed request without a recorded answer is ambiguous and is not executed again with that key. Mark unclosed effects uncertain on recovery. Fence ending before detach; retain failed teardown resources for a later explicit delete attempt with a new idempotency key. Use session-owned model bindings.

## Alternatives considered

In-memory-only deduplication does not survive the failure boundary. Retrying unfinished claims or teardown automatically substitutes a scheduler for honest uncertainty. A database is not required merely to persist these small server-owned records.

## Consequences

A completed idempotency answer has a documented retention window; it is not permanent exactly-once delivery. Failed detach does not prevent ending, but failed teardown remains separately observable. Reopening incomplete creation/ending produces honest lifecycle outcomes without restarting the Agentloop. Metadata acknowledgements use the supported flush and torn-tail rules too.

## Sources

- Brain implementation/history: [57caae4](https://github.com/aexhq/brain/commit/57caae4f1a0cfc4689462272d7659fd75f60c988), [682ce03](https://github.com/aexhq/brain/commit/682ce03104991f5815de0655165509510f07dbb6).
- Current reference: [docs/concepts/sessions.mdx](../../docs/concepts/sessions.mdx).
- Original decision: “2026-09-05: Preserve effect identity and uncertainty across interruption” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- [Codex session `01a0705d-503e-7a53-a65f-a1e9ca13e23c`](SOURCES.md#session-01a0705d-503e-7a53-a65f-a1e9ca13e23c), 2026-09-05T08:52:25.865Z: User accepts interrupted-turn Events and leaves the next activation to the user.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
