# ADR-055: Give Agentloops one context and complete event batches on return

- Decision date: 2026-10-03
- Status: Accepted

Refines the authoring interface in [ADR-046](2026-09-09-01-minimal-extension-contract.md)
and supersedes the explicit processed-sequence service in
[ADR-048](2026-09-21-01-tool-completion-and-event-activation.md).

## Context

The JavaScript authoring path exposes raw Component imports, serialized input and an
explicit processing cursor. These require authors to reproduce binding code and manage
runtime bookkeeping. Agentloops already have durable KV for their own progress policies.

## Decision

Give the handler one context containing parsed input and scoped services. Use
`ctx.kv.get/set/delete`, the existing `ctx.model`, `ctx.emit` and `ctx.environments`
vocabulary, and `ctx.callTool/callTools` for individual and parallel tool invocation.
Keep serialization and Component imports in the runtime adapter. Generate payload types
from the Rust contracts. Successful writes remain individually durable.

Successful return completes the delivered contiguous event batch, including intentionally
ignored events. Include contiguous pages read during the activation, and record the
completed boundary with the successful turn record. Failure or cancellation leaves the
batch incomplete without undoing committed writes or automatically retrying effects.
Unseen events arriving during an activation remain pending. Deliver remaining pages of a
background burst through subsequent activations.

Remove the public acknowledgement service from native, Component and HTTP bindings.
The internal cursor is runtime bookkeeping. A tool return can overlap journal history;
the Agentloop owns skipping, remembered observations and finer checkpoints in ordinary KV.
Reading a later tool result cannot complete an earlier, unseen journal event.

## Alternatives and consequences

- Renaming acknowledgement retains the authoring burden. There is no separate public
  checkpoint or retry-policy API.
- Completing the journal head at return could silently consume concurrent observations.
  Only the delivered prefix is complete.
- Automatically remembering every individual tool observation would add a second
  tracking mechanism. Loops can retain their chosen progress in KV.

An activation that fails late can receive earlier observations again. The official loops
save their own observation position after saving selected conversation changes. Rebuild
Component loops against the updated WIT and deploy the runtime and loop packages together.

## Sources and verification

- Owner discussion, 2026-10-03: minimal context API, `ctx.kv.get/set/delete`, normal return
  as the batch boundary, and loop-owned skipping or remembered progress in KV.
- `platform/docs/code.md`: small cohesive interfaces, explicit persistence, least
  astonishment and runtime details hidden behind stable author operations.
- Verify success, failure, cancellation, concurrent arrivals, pagination, journal reopen,
  typed context bindings and official loop continuation through the real worker.
