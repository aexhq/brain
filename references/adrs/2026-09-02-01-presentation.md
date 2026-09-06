# ADR-018: Let the Agentloop control model presentation within fixed authority

- Decision date: 2026-09-02
- Status: Accepted
- Compiled: 2026-09-05

## Context

A canonicalized ModelPresentation pinned the system prompt and tool presentation for a session. That restricted context selection and prompt-cache policy even though the Agentloop was intended to own policy.

## Decision

The creator supplies the initial system prompt, response format, and Tool catalogue. The Agentloop may replace the prompt, choose which admitted tools to present, and change response format per call. Remove presentation/config hashes as public equality shortcuts. Keep execution bindings and the admitted catalogue fixed at creation.

## Alternatives considered

A stable prefix may improve model caching, but that is a choice for the loop. Pinning presentation conflates what the model sees with what it is authorized to invoke. The actual journaled request provides the audit record.

## Consequences

The loop can compact or rewrite context without a new session. It cannot acquire new tools, credentials, destinations, or a different model binding merely by changing presentation. The turn-shaped interface in [ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md) replaces the earlier per-step defaulting machinery.

## Sources

- Brain implementation/history: [6dbf517](https://github.com/aexhq/brain/commit/6dbf517f319414bbec1145a3badb145db4b12a8f), [6d20d59](https://github.com/aexhq/brain/commit/6d20d59421f1ba137eeea6c471f44afffc758ceb).
- Current reference: [docs/concepts/model.mdx](../../docs/concepts/model.mdx).
- Current reference: [docs/concepts/agent-loop.mdx](../../docs/concepts/agent-loop.mdx).
- Original decision: “2026-09-02: The agent loop owns what the model sees” in [DECISIONS.md at compilation baseline](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md).
- [Claude session `56115bda-1ba4-40dc-8c47-28b8e6273e24`](SOURCES.md#session-56115bda-1ba4-40dc-8c47-28b8e6273e24), 2026-09-02T16:10:09.940Z: User gives the loop control over presentation and rejects precomputed configuration equality.
- [Claude session `56115bda-1ba4-40dc-8c47-28b8e6273e24`](SOURCES.md#session-56115bda-1ba4-40dc-8c47-28b8e6273e24), 2026-09-02T18:42:27.478Z: User clarifies that the creator still chooses the initial system prompt.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
