# ADR-006: Make external event listeners the only session persistence

- Decision date: 2026-08-26
- Status: Rejected
- Compiled: 2026-09-05

## Context

An initial mental model described Brain as an ephemeral execution kernel whose external listeners persisted history and supplied state on reconstruction. Listeners were best effort and a crash could lose an unpersisted tail.

## Decision

Do not adopt that proposal as the complete standalone product. The same discussion explicitly restored a runnable Brain server with a journal on disk. External consumers may persist Events, but their queues are their own integration; Brain does not implement an external delivery service for them.

## Alternatives considered

External-only persistence minimizes local state but cannot make standalone history recovery independent of listener availability. Bounded best-effort delivery remains useful for telemetry, as recorded in [ADR-015: Keep telemetry bounded and best effort, and use cursors for durable consumers](2026-08-31-01-telemetry.md).

## Consequences

Ephemeral computation and local persistence are separate choices. The later [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md) decision releases execution while retaining [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md). This record must not be read as rejecting turn-end suspension, nor as claiming August 26 settled the current flush guarantee; [ADR-009: Replace SQLite with a write-behind segment journal](2026-08-28-01-write-behind.md) temporarily chose weaker local durability.

## Sources

- Brain implementation/history: [6081ebb](https://github.com/aexhq/brain/commit/6081ebbf94f4dd38a7dfbd97c641c18f58efb187).
- Current reference: [docs/concepts/sessions.mdx](../../docs/concepts/sessions.mdx).
- [Codex session `01a03dd3-4217-7cc3-973c-8336841a6a26`](SOURCES.md#session-01a03dd3-4217-7cc3-973c-8336841a6a26), 2026-08-26T15:06:43.272Z: User presents the external-persistence ephemeral-kernel proposal.
- [Codex session `01a03dd3-4217-7cc3-973c-8336841a6a26`](SOURCES.md#session-01a03dd3-4217-7cc3-973c-8336841a6a26), 2026-08-26T16:52:06.882Z: User clarifies that standalone Brain keeps its journal on disk.
- [Codex session `01a03dd3-4217-7cc3-973c-8336841a6a26`](SOURCES.md#session-01a03dd3-4217-7cc3-973c-8336841a6a26), 2026-08-27T11:16:14.075Z: User leaves external queue integration to clients and limits Brain telemetry to best effort.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
