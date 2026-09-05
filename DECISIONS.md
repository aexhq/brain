# Design decisions

Brain’s architecture decisions now live in [references/adrs](references/adrs/README.md),
with date-sortable filenames, decision status, alternatives, consequences, and source evidence.
Start there for the current architecture and chronological history. Future decisions belong
there; add a new ADR when reversing an accepted decision and link its predecessor.

The sections below preserve links to the former decision log. Historical wording remains
in Git; each link identifies whether the decision still stands or was superseded.
See the [complete source map](references/adrs/SOURCES.md).

## 2026-09-05: Standalone, ephemeral execution

[ADR-037: Release execution at turn boundaries and prepare artifacts before creation](references/adrs/2026-09-05-06-ephemeral.md); [ADR-038: Defer dynamic placement, workflow durability, and advanced tenancy policy](references/adrs/2026-09-05-07-roadmap-boundary.md); [ADR-032: Use one canonical journal and commit before exposing records or effects](references/adrs/2026-09-05-01-canonical-journal.md).

## 2026-09-02: The agent loop owns what the model sees

[ADR-018: Let the Agentloop control model presentation within fixed authority](references/adrs/2026-09-02-01-presentation.md).

## 2026-09-02: The journal records a model request as a diff against the last one

[ADR-019: Record transcript changes as common-prefix deltas](references/adrs/2026-09-02-02-prefix-deltas.md).

## 2026-09-02: Effect records are named for what happened, not for intent

[ADR-020: Name effects by session and sequence, with one lifecycle vocabulary](references/adrs/2026-09-02-03-record-identity.md).

## 2026-09-02: No request identity

[ADR-020: Name effects by session and sequence, with one lifecycle vocabulary](references/adrs/2026-09-02-03-record-identity.md).

## 2026-09-02: A session has two ids, `session_id` and `sequence`

[ADR-020: Name effects by session and sequence, with one lifecycle vocabulary](references/adrs/2026-09-02-03-record-identity.md).

## 2026-09-02: The kernel is one session; the server manages sessions

[ADR-021: Make the core runtime one session and let the server manage sessions](references/adrs/2026-09-02-04-session-runtime.md).

## 2026-09-04: The agent loop drives the turn; Brain provides services

[ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](references/adrs/2026-09-04-01-turn-services.md).

## 2026-09-04: One session configuration, and nothing is sealed

[ADR-024: Use one SessionConfig and keep hashes local to their actual purpose](references/adrs/2026-09-04-02-simple-config.md).

## 2026-09-04: Identity is an idempotency key and nothing else

[ADR-024: Use one SessionConfig and keep hashes local to their actual purpose](references/adrs/2026-09-04-02-simple-config.md).

## 2026-09-04: One directory per session, two append-only logs, one sequence

[ADR-025: Store transcript state and public Events in two per-session logs](references/adrs/2026-09-04-03-two-logs.md).

## 2026-09-04: Every dependency of a session is injected, and recovery is load and construct

[ADR-026: Recover saved data by loading it, without replaying the Agentloop](references/adrs/2026-09-04-04-load-recovery.md).

## 2026-09-04: Idle sessions are suspended and rebuilt on demand

[ADR-027: Suspend idle session actors after an idle TTL](references/adrs/2026-09-04-05-idle-ttl.md).

## 2026-09-04: Environments are resources with an optional managed lifecycle

[ADR-028: Expose independently created and shared Environment resources](references/adrs/2026-09-04-06-shared-environments.md).

## 2026-09-04: One catalogue of codes

[ADR-029: Declare machine-readable Events and failures once](references/adrs/2026-09-04-07-codes.md).

## 2026-09-04: The data directory is not migrated

[ADR-030: Replace pre-1.0 contracts and development data in place](references/adrs/2026-09-04-08-clean-break.md).

## 2026-09-04: The Rust types are the source of the contracts

[ADR-031: Generate public contracts from Rust types and route annotations](references/adrs/2026-09-04-09-rust-contracts.md).

## 2026-09-05: One canonical journal, with transcript and Events as projections

[ADR-032: Use one canonical journal and commit before exposing records or effects](references/adrs/2026-09-05-01-canonical-journal.md).

## 2026-09-05: Placement is explicit and execution has two forms

[ADR-033: Distinguish resident Tools from explicitly placed extensions](references/adrs/2026-09-05-02-placement.md).

## 2026-09-05: Brain's native Environment runs Components in one worker process

[ADR-034: Run native Components in a separate capability-restricted Wasmtime worker](references/adrs/2026-09-05-03-wasm-worker.md).

## 2026-09-05: Brain sends each effect once and reports the outcome

[ADR-035: Send each effect once and expose failures to the Agentloop](references/adrs/2026-09-05-04-send-once.md).

## 2026-09-05: Preserve effect identity and uncertainty across interruption

[ADR-036: Preserve request claims and effect uncertainty across interruption](references/adrs/2026-09-05-05-interruption.md).
