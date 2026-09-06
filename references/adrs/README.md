# Brain architecture decision records

This is the decision history for Brain, compiled on 2026-09-05 from this repository,
its Git history, and relevant local Codex and Claude discussions. Other repositories
are outside this compilation’s scope. [SOURCES.md](SOURCES.md) records coverage,
session locators, and how dates and status were assigned.

Files sort chronologically as `YYYY-MM-DD-NN-topic.md`. `NN` is a stable ordering
within a date, not a claim about the exact time of the decision. ADR numbers identify
records; use the full filename in links. New decisions get new records. Update an
old record’s status and successor link when it is reversed; preserve its historical decision.

**Accepted** means the decision or explicit scope deferral stands at the compilation
baseline. It does not make a deferred feature implemented. **Superseded** means a
historically adopted design was replaced. **Rejected** records a proposal not adopted
in that form. New unresolved choices should use **Proposed** until settled.

For current behavior, start with the records below. Exact APIs and configuration
remain in [the contracts](../../contracts), [user docs](../../docs), and
[ROADMAP.md](../../ROADMAP.md); ADRs explain why they take that shape.

## Current architecture

- [ADR-001: Make Brain an independent runtime with injected execution ports](2026-08-20-01-standalone.md)
- [ADR-021: Make the core runtime one session and let the server manage sessions](2026-09-02-04-session-runtime.md)
- [ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md)
- [ADR-018: Let the Agentloop control model presentation within fixed authority](2026-09-02-01-presentation.md)
- [ADR-019: Record transcript changes as common-prefix deltas](2026-09-02-02-prefix-deltas.md)
- [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md)
- [ADR-035: Send each effect once and expose failures to the Agentloop](2026-09-05-04-send-once.md)
- [ADR-036: Preserve request claims and effect uncertainty across interruption](2026-09-05-05-interruption.md)
- [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md)
- [ADR-034: Run native Components in a separate capability-restricted Wasmtime worker](2026-09-05-03-wasm-worker.md)
- [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md)
- [ADR-031: Generate public contracts from Rust types and route annotations](2026-09-04-09-rust-contracts.md)
- [ADR-038: Defer dynamic placement, workflow durability, and advanced tenancy policy](2026-09-05-07-roadmap-boundary.md)
- [ADR-039: Keep performance claims historical until representative baselines are rebuilt](2026-09-05-08-benchmark-policy.md)
- [ADR-040: Identify records by session and sequence, and everything inside a session by name](2026-09-05-09-session-names.md)
- [ADR-041: Run every Tool in an Environment that implements one protocol, including Brain's own](2026-09-05-10-one-execution-model.md)
- [ADR-042: Each crate renders its own contract into its own generated directory](2026-09-06-01-per-crate-contracts.md)
- [ADR-043: Make every limit a deployment default injected at server start](2026-09-06-02-deployment-limits.md)

## Chronological index

| Date | Record | Status |
| --- | --- | --- |
| 2026-08-20 | [ADR-001: Make Brain an independent runtime with injected execution ports](2026-08-20-01-standalone.md) | Accepted |
| 2026-08-21 | [ADR-002: Execute replaceable Agentloops through an isolated step contract](2026-08-21-01-step-loops.md) | Superseded |
| 2026-08-24 | [ADR-003: Replace the default sandbox with explicit Environment bindings](2026-08-24-01-environment-neutral.md) | Accepted |
| 2026-08-25 | [ADR-004: Route all four extension kinds through Component worlds](2026-08-25-01-four-worlds.md) | Superseded |
| 2026-08-26 | [ADR-005: Keep WIT Components and use Wasmtime for the native runtime](2026-08-26-01-runtime-choice.md) | Accepted |
| 2026-08-26 | [ADR-006: Make external event listeners the only session persistence](2026-08-26-02-external-history.md) | Rejected |
| 2026-08-27 | [ADR-007: Compose sessions with typed extension factories](2026-08-27-01-typed-authoring.md) | Accepted |
| 2026-08-27 | [ADR-008: Keep Brain documentation and release verification with the product](2026-08-27-02-repo-gates.md) | Accepted |
| 2026-08-28 | [ADR-009: Replace SQLite with a write-behind segment journal](2026-08-28-01-write-behind.md) | Superseded |
| 2026-08-28 | [ADR-010: Bound journal growth and measure the complete execution path](2026-08-28-02-growth.md) | Accepted |
| 2026-08-28 | [ADR-011: Keep measured performance verdicts separate from hypotheses](2026-08-28-03-research-verdicts.md) | Accepted |
| 2026-08-30 | [ADR-012: Store provider-neutral messages and normalize model transports](2026-08-30-01-provider-model.md) | Accepted |
| 2026-08-30 | [ADR-013: Make compatible providers reviewed deployment data](2026-08-30-02-provider-catalog.md) | Accepted |
| 2026-08-30 | [ADR-014: Rebuild session views from recorded history](2026-08-30-03-journal-projections.md) | Accepted |
| 2026-08-31 | [ADR-015: Keep telemetry bounded and best effort, and use cursors for durable consumers](2026-08-31-01-telemetry.md) | Accepted |
| 2026-09-01 | [ADR-016: Terminate application Tool channels in Brain](2026-09-01-01-client-channels.md) | Superseded |
| 2026-09-01 | [ADR-017: Reuse per-session Wasm instances and resident turn context](2026-09-01-02-warm-instances.md) | Superseded |
| 2026-09-02 | [ADR-018: Let the Agentloop control model presentation within fixed authority](2026-09-02-01-presentation.md) | Accepted |
| 2026-09-02 | [ADR-019: Record transcript changes as common-prefix deltas](2026-09-02-02-prefix-deltas.md) | Accepted |
| 2026-09-02 | [ADR-020: Name effects by session and sequence, with one lifecycle vocabulary](2026-09-02-03-record-identity.md) | Accepted |
| 2026-09-02 | [ADR-021: Make the core runtime one session and let the server manage sessions](2026-09-02-04-session-runtime.md) | Accepted |
| 2026-09-02 | [ADR-022: Let Environments execute implementations using their own platform APIs](2026-09-02-05-resources.md) | Accepted |
| 2026-09-04 | [ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md) | Accepted |
| 2026-09-04 | [ADR-024: Use one SessionConfig and keep hashes local to their actual purpose](2026-09-04-02-simple-config.md) | Accepted |
| 2026-09-04 | [ADR-025: Store transcript state and public Events in two per-session logs](2026-09-04-03-two-logs.md) | Superseded |
| 2026-09-04 | [ADR-026: Recover saved data by loading it, without replaying the Agentloop](2026-09-04-04-load-recovery.md) | Accepted |
| 2026-09-04 | [ADR-027: Suspend idle session actors after an idle TTL](2026-09-04-05-idle-ttl.md) | Superseded |
| 2026-09-04 | [ADR-028: Expose independently created and shared Environment resources](2026-09-04-06-shared-environments.md) | Superseded |
| 2026-09-04 | [ADR-029: Declare machine-readable Events and failures once](2026-09-04-07-codes.md) | Accepted |
| 2026-09-04 | [ADR-030: Replace pre-1.0 contracts and development data in place](2026-09-04-08-clean-break.md) | Accepted |
| 2026-09-04 | [ADR-031: Generate public contracts from Rust types and route annotations](2026-09-04-09-rust-contracts.md) | Accepted |
| 2026-09-05 | [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md) | Accepted |
| 2026-09-05 | [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md) | Accepted |
| 2026-09-05 | [ADR-034: Run native Components in a separate capability-restricted Wasmtime worker](2026-09-05-03-wasm-worker.md) | Accepted |
| 2026-09-05 | [ADR-035: Send each effect once and expose failures to the Agentloop](2026-09-05-04-send-once.md) | Accepted |
| 2026-09-05 | [ADR-036: Preserve request claims and effect uncertainty across interruption](2026-09-05-05-interruption.md) | Accepted |
| 2026-09-05 | [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md) | Accepted |
| 2026-09-05 | [ADR-038: Defer dynamic placement, workflow durability, and advanced tenancy policy](2026-09-05-07-roadmap-boundary.md) | Accepted |
| 2026-09-05 | [ADR-039: Keep performance claims historical until representative baselines are rebuilt](2026-09-05-08-benchmark-policy.md) | Accepted |
| 2026-09-05 | [ADR-040: Identify records by session and sequence, and everything inside a session by name](2026-09-05-09-session-names.md) | Accepted |
| 2026-09-05 | [ADR-041: Run every Tool in an Environment that implements one protocol, including Brain's own](2026-09-05-10-one-execution-model.md) | Accepted |
| 2026-09-06 | [ADR-042: Each crate renders its own contract into its own generated directory](2026-09-06-01-per-crate-contracts.md) | Accepted |
| 2026-09-06 | [ADR-043: Make every limit a deployment default injected at server start](2026-09-06-02-deployment-limits.md) | Accepted |

## Important reversals

- [ADR-031: Generate public contracts from Rust types and route annotations](2026-09-04-09-rust-contracts.md), its central brain-contracts renderer and top-level contracts/ → [ADR-042: Each crate renders its own contract into its own generated directory](2026-09-06-01-per-crate-contracts.md)
- [ADR-022: Let Environments execute implementations using their own platform APIs](2026-09-02-05-resources.md), its create-time check of Tool needs against declared resources → [ADR-041: Run every Tool in an Environment that implements one protocol, including Brain's own](2026-09-05-10-one-execution-model.md)
- [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md), its two Tool forms → [ADR-041: Run every Tool in an Environment that implements one protocol, including Brain's own](2026-09-05-10-one-execution-model.md)
- [ADR-024: Use one SessionConfig and keep hashes local to their actual purpose](2026-09-04-02-simple-config.md), its random attachment identifiers → [ADR-040: Identify records by session and sequence, and everything inside a session by name](2026-09-05-09-session-names.md)

- [ADR-009: Replace SQLite with a write-behind segment journal](2026-08-28-01-write-behind.md) → [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md)
- [ADR-025: Store transcript state and public Events in two per-session logs](2026-09-04-03-two-logs.md) → [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md)
- [ADR-002: Execute replaceable Agentloops through an isolated step contract](2026-08-21-01-step-loops.md) → [ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md)
- [ADR-017: Reuse per-session Wasm instances and resident turn context](2026-09-01-02-warm-instances.md) → [ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md) → [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md)
- [ADR-028: Expose independently created and shared Environment resources](2026-09-04-06-shared-environments.md) → [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md)
- [ADR-016: Terminate application Tool channels in Brain](2026-09-01-01-client-channels.md) → [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md)
- [ADR-027: Suspend idle session actors after an idle TTL](2026-09-04-05-idle-ttl.md) → [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md)

The external-only persistence proposal in [ADR-006: Make external event listeners the only session persistence](2026-08-26-02-external-history.md) is distinct from today’s ephemeral execution with a durable local journal.
