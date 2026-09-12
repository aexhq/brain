# ADR-047: Preserve known Tool stop causes and accept host outcomes directly

- Decision date: 2026-09-12
- Status: Accepted

## Context

Host functions could only return successful values or throw an unstructured exception. Returning
an existing Outcome was wrapped as success, so adapters could not preserve structured failures or
report a lost remote result. Several deadline and cancellation paths also returned unknown despite
knowing why the invocation stopped.

## Decision

Tool invocation deadlines produce `timeout`, explicit cancellation produces `cancelled`, and known
failures produce `error`. Unknown is the last resort for a possibly dispatched effect with no reliable
terminal result. A caught transport exception does not prove whether the remote effect completed.
Timeout and cancellation describe the caller's stop cause and do not promise rollback. Preserve the
first terminal cause and existing bounded cleanup. Every non-success outcome is a failed Tool result.

Host functions return ordinary output or an existing Outcome directly. Reserve the top-level status
discriminators `ok`, `error`, `timeout`, `cancelled` and `unknown`; validate their envelope before
posting. Only successful values undergo output-schema validation. Preserve structured error fields.
An explicit success envelope can carry business data that uses a reserved status; do not recursively
interpret its value. Derive non-success SDK types from the generated protocol types.

An explicit Environment unknown receipt is an Outcome, not evidence that the Environment is
unreachable. Actual transport ambiguity retains the unreachable journal event. Environment timeout
and cancellation use existing failure receipt codes. No wire shape or transcript status is added.

## Consequences

Host adapters need no result helper, symbol brand or separate authoring mode. Applications returning
business data with a reserved status must put it in an explicit success value. Ordinary thrown
exceptions remain `tool_error`; adapters return structured Outcomes when they have more information.

The [send-once decision](2026-09-05-04-send-once.md) still applies. This clarifies Tool stop causes
without changing [recovery of uncertain effects](2026-09-05-05-interruption.md), model-call outcomes,
or whole-turn results. User-facing behavior and examples live in
[Write a Tool](../../docs/guides/write-a-tool.mdx).
