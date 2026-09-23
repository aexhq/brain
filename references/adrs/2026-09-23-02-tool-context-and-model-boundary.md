# ADR-050: Let Tools present results while one Agentloop owns the conversation

- Decision date: 2026-09-23
- Status: Accepted
- Implementation: Optional result text and invocation-owned model services implemented across SDK, HTTP, and Wasm.

## Context

Tools know how to present their results, but concurrent transcript edits would
introduce conflicts and distributed compaction. Authors need familiar inputs,
results, and model calls without learning a coordination protocol.

## Decision

Keep Tool and Agentloop as distinct extension types with consistent authoring and
placement conventions. [ADR-049](2026-09-23-01-tool-authoring-and-packaging.md)
covers those conventions. One Agentloop owns the session's shared transcript,
compaction, and scheduling of conversational model turns. Tools own their work and
the meaning and presentation of their results.

**Let Tools supply model-facing content with ordinary results.** By default,
Agentloop presents the result. A Tool may provide a suitable representation alongside
its structured output, including for intermediate results. Use a common,
language-neutral content contract so Agentloops need no Tool-specific event handlers.

The SDK keeps `ctx.finish(result)` and adds
`ctx.finish(document, { content: summary })`, also available on `emitResult`.
Content represents that result; it grants no transcript editing or permanent context
entry. Output validation and failure status retain their meanings.

**Agentloop remains the only conversation writer.** It places Tool content in valid
model messages and may select, defer, or compact it. Concurrent results retain their
invocation identity and journal order. Conflicting information remains observations
for Agentloop to handle. Tools cannot rewrite the transcript or perform compaction.

**Allow independent model calls inside Tools.** Provide the model-call primitive,
`ctx.model(...)`, with explicit messages and a returned response.
The response belongs to that invocation without editing the shared transcript or
advancing its conversation. Tools publish outcomes through normal results.
Independent requests may run concurrently; developers need not avoid combining
model-calling Tools.

Tool model services use the invocation's lifetime and the session's fixed authority,
with existing cancellation, budget, durable effect recording, and send-once rules.
They must not retain a finished Agentloop's services. Direct provider HTTP calls
remain subject to Environment permissions but outside Brain's model guarantees.
Developers coordinate dependencies in their work and shared external resources.

**Keep attention separate from inference.** Preserve
[ADR-048](2026-09-21-01-tool-completion-and-event-activation.md): ordinary progress
events can wait; results and terminal outcomes make Agentloop eligible to run.
Agentloop may handle them in code, call a model, or defer. Errors need no Tool-side
model call. Events arriving during a model request cannot alter that request;
preempting it is explicit Agentloop policy. No public priority or conflict-resolution
API is added.

## Alternatives considered

- Merge Tool and Agentloop authority: obscures who owns conversation changes.
- Let Tools edit shared context directly: requires coordination and distributed
  compaction beyond this scope.
- Restrict all model requests to Agentloop, or require developers to prevent their
  overlap: makes independent model-backed Tools unnecessarily difficult to compose.

## Consequences and verification

Ordinary Tools retain a small interface. The additional work is a standard optional
result representation and invocation-owned model services across the protocol, SDK,
and execution bindings.

Implementation must verify that concurrent Tool model calls preserve result identity
without modifying the shared transcript; supplied content preserves structured
output and failure semantics; and completion, cancellation, and wakeups respect the
existing lifecycle without forcing another model call. Required CI gates still apply.

## Sources

- Owner discussion, 2026-09-23: minimal interfaces, Tool-owned result presentation,
  independent model calls, and one Agentloop with no distributed compaction.
- [ADR-018](2026-09-02-01-presentation.md): Agentloop presentation ownership within
  fixed authority; this decision preserves that final ownership.
- Current [execution services](../../crates/brain-sessions/src/execution.rs) and
  [host Tool interface](../../packages/brain-sdk/src/host.ts).

[Index](README.md)
