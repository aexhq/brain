# ADR-043: Make every limit a deployment default injected at server start

- Decision date: 2026-09-06
- Status: Accepted
- Compiled: 2026-09-06

## Context

Brain's crates hold about fifty fixed numbers. Some are shape rules a message must satisfy to
be valid. Some are budgets for memory, disk, wall time, or concurrency. Some are round-number
counts that nobody measured and nothing depends on. Today they all look the same: a `const`
in the crate that enforces it, unreachable from the process that starts the server.

The kv key cap made the problem concrete. A loop returning more than 128 keys failed its turn
because PR #163 chose 128 for every count it introduced. The number guarded nothing a byte
budget did not already guard, was documented nowhere, and Brain has no idea how large the
machine hosting it is, so it had no basis for the value. The cap was deleted and the sweep
that followed found the same shape twenty more times.

Three limits are already exposed (`BRAIN_MAX_MODEL_CALLS`, `BRAIN_MAX_TURN_SECS`,
`BRAIN_SESSION_IDLE_TTL_SECS`) and show the pattern working: the server parses a flag with an
environment fallback, and `SessionRuntime` receives the value as a plain field. Everything else
is a constant. `LoopLimits` is a struct with a `Default`, but `main.rs` constructs it with
`LoopLimits::default()` and offers no way to change a field. A deployment that wants a larger
Wasm memory, a longer tool deadline, or a bigger journal queue has to fork.

## Decision

Every number in Brain is one of four things, and the code says which.

**Invariants** are shape rules of the contract or the on-disk format: identifier syntax and
length, the journal frame header, crypto key and nonce sizes, the idempotency key length,
SSE separator handling. They are constants in the crate that owns the format, because changing
one changes what a valid message or file is. They are not configuration.

**Deployment limits** are budgets a machine or an operator sets: bytes a request, a turn, a
package, a Wasm instance, a model stream, or a journal queue may hold; seconds a turn, a Tool,
or a model call may run; how many turns a worker runs at once; how long idle state, request
claims, or unconnected hosts are kept. Brain ships a default for each and never treats the
default as a fact about the machine. Each limit is one field on the limits struct of the crate
that enforces it, with its default in that struct's `Default`. The crate reads no process
environment. One component in `brain-server`, the clap `ServerConfig` and the `compose`
function beside it, is the only place that reads flags and environment variables. It maps
every field of every limits struct onto one `BRAIN_*` variable and flag, constructs each
crate's struct once, and injects it by value into the crate that enforces it. Embedders
construct the same structs and fill the same fields, from wherever their own configuration
comes.

**Counts with no budget behind them** are deleted. A count cap on tools, environments, emits
per turn, tool calls per dispatch, transcript items, content blocks, or tool calls per model
message bounds nothing that the byte budgets around it do not already bound, and it fails
real workloads at a number nobody chose. The byte budgets stay and become deployment limits.

**Model limits** are not Brain's. Output token ceilings, context windows, and whatever else
a provider enforces are facts about the model, known to the provider catalog and chosen per
session. Brain neither sets nor overrides them, so they are not deployment limits and get no
`BRAIN_*` variable.

Rules that follow:

- **One place per limit.** The default lives in the crate's `Default` impl. The server's
  clap definition uses that value as its default, so the number is written once. Docs that
  print the number are checked against that source.
- **One naming scheme.** `BRAIN_MAX_<THING>_<UNIT>` for ceilings (`_BYTES`, `_SECS`),
  `BRAIN_<THING>_SECS` for retention and idle durations, matching the three that exist.
  Flags win over environment. Parse failures and out-of-range values fail startup.
- **Zero means no bound.** `BRAIN_MAX_TURN_SECS=0` already means unbounded. The same reading
  applies to every ceiling, so a deployment that trusts its workload can switch a bound off
  without inventing a large number. Trust-boundary limits keep a non-zero default.
- **Derived numbers are derived.** The loophost's 120-second host-call ceiling and the
  125-second worker backstop are the tool deadline plus slack. They are computed from the
  injected tool deadline, not kept as separate constants that drift.
- **Trust-boundary limits stay on.** The HTTP request size cap and the host registration cap
  face untrusted callers. They remain deployment limits with a non-zero default and are
  never deleted.
- **Brain does not cap the operator.** The startup check that refuses
  `BRAIN_MAX_MODEL_CALLS` above 1,024 is the same mistake one level up and goes.
- **The loop worker is injected the same way.** `brain-loop-worker` is a separate process
  the supervisor spawns. It receives its `LoopLimits` as command-line arguments at spawn,
  parsed by the one clap definition in `brain-loophost` that the supervisor also renders
  from, so both sides hold the same values by construction. Wire frame ceilings derive from
  them on both ends. The argument list is an internal detail between two binaries built
  together and carries no version.

### Inventory

Deployment limits, to be exposed. Defaults are today's values unless marked.

| Crate | Field | Today | Note |
| --- | --- | --- | --- |
| brain | `max_model_calls_per_turn` | 128 | exposed already |
| brain | `max_turn_secs` | 1800 | exposed already |
| brain | `tool_deadline_secs` | 120 | constant, not exposed |
| brain | `max_emitted_bytes_per_turn` | 1 MiB | documented |
| brain | `max_provider_assistant_bytes` | 192 KiB | too small for 64k-token outputs; default rises |
| brain | `max_provider_delta_bytes` | 64 KiB | |
| brain | `max_model_stream_bytes` / `max_sse_frame_bytes` / `max_error_bytes` | 32 MiB / 256 KiB / 16 KiB | |
| brain | model HTTP timeout / connect timeout | 120 s / 10 s | |
| brain | journal `max_queued_bytes` / `owner_queue_bytes` / `open_files` | 64 MiB / 8 MiB / 256 | `Writer::spawn` takes them |
| brain | `live_backlog` | 1,024 records | `Feed::new` takes it |
| brain-loophost | `package_bytes` / `turn_input_bytes` / `turn_output_bytes` | 32 MiB each | `LoopLimits` fields exist |
| brain-loophost | `linear_memory_bytes` / `fuel` / `concurrent_turns_per_worker` | 128 MiB / 10^10 / 8 | `LoopLimits` fields exist |
| brain-telemetry | `max_queue_records` / `max_queue_bytes` / `max_retry_age` | 4,096 / 8 MiB / 30 s | `telemetry_channel` takes them |
| brain-http | `max_request_bytes` | 32 MiB | trust boundary; the 2 MiB session and message caps in `brain` collapse into this one |
| brain-server | `max_environment_response_bytes` | 2 MiB | will bite the first Tool returning a file; default rises |
| brain-server | environment HTTP timeout / connect timeout | 120 s / 5 s | |
| brain-server | `max_hosts` / `host_command_capacity` / `unconnected_host_ttl_secs` | 4,096 / 128 / 60 | `max_hosts` is a trust boundary |
| brain-server | `idempotency_retention_secs` | 86,400 | |
| brain-server | `session_idle_ttl_secs` | none | exposed already |

Deleted:

| Limit | Where |
| --- | --- |
| 128 emits per turn | `session/actor.rs` |
| 128 tool calls per dispatch | `session/actor.rs` |
| 128 tools, 128 environments, 8 KiB tool description, 256-byte model name, 128 KiB system prompt (checked twice) | `session/mod.rs`, `session/actor.rs` |
| 4,096 transcript items | `brain-protocol` `MAX_TRANSCRIPT_ITEMS`, a contract rewrite in place under ADR-030 |
| 64 content blocks, 32 tool calls per model message | `model/accumulator.rs` |
| `BRAIN_MAX_MODEL_CALLS` ceiling of 1,024 | `main.rs` validate |

Invariants, unchanged: identifier rules, journal segment and header sizes, `KEY_BYTES`,
`NONCE_BYTES`, `MAX_IDEMPOTENCY_KEY_BYTES`, SSE separator overlap, sweep amortization
minimums, fuel yield interval, and the event page size, which has a documented fallback and
never rejects.

Model limits, out of scope here: the Anthropic dialect's default of 8,192 output tokens is
a model request default. It belongs with the provider catalog, which knows each model's
output ceiling, and with per-session configuration. It is tracked separately.

## Alternatives considered

**One global `Limits` struct in `brain-server` passed everywhere.** Simpler to read, but it
makes every crate depend on the server's type and puts the journal's queue size next to the
Wasm fuel budget in a struct no single crate owns. ADR-042 settled that a crate owns its own
contract; the same holds for its limits. The server composes, it does not define. What is
shared is the reader: one component parses the environment and builds every struct, so no
crate reads the environment and no two places disagree about a variable's name or default.

**A configuration file.** Brain already commits to flags and environment variables, the
Docker image already sets them, and nothing here needs structure a flat key cannot express.
A file would be a second source of the same values.

**Keep count caps but make them configurable.** Exposing a knob for the number of tools per
session still asks the operator to choose a number that means nothing to them. A count only
matters through the bytes it costs, and those are already bounded.

**Let each crate read its own environment variables.** Shorter, but then the library crates
behave differently depending on the process environment, embedders lose control, and the
configuration reference has no single place to be true from.

## Consequences

`ServerConfig` grows by roughly twenty-five fields and the configuration reference by the same
number of rows. Every row must exist in exactly one place, so the docs table is generated or
checked from the clap definitions in `npm run gen` and the CI diff guard.

`Writer::spawn`, `Feed::new`, and `telemetry_channel` take limits arguments; `LoopLimits`
is constructed from config; the embed guide shows the structs being filled. Embedders who
relied on constants exported from `brain` switch to the `Default` impls.

`MAX_TRANSCRIPT_ITEMS` leaves the JSON Schema and the generated SDK types, a pre-1.0 rewrite
in place. A turn's transcript is bounded by turn input and output bytes instead.

The two limits already failing real workloads, assistant output at 192 KiB and environment
responses at 2 MiB, get larger defaults in the same change. Their new defaults are chosen
from current model output ceilings and measured Tool responses, not from a round number, and
the reasoning is written next to the `Default`.

The roadmap item added when the kv cap was deleted, configurable kv bounds, is subsumed:
kv is bounded by turn output bytes and journal queue bytes, both deployment limits under this
record. It is replaced by an entry pointing here.

Fair scheduling, per-tenant budgets, and admission policy for mutually untrusted extensions
remain deferred under ADR-038. This record decides where a limit lives and who sets it, not
how a multi-tenant deployment shares it.

## Sources

- Current reference: [crates/brain-server/src/config.rs](../../crates/brain-server/src/config.rs).
- Current reference: [crates/brain-loophost/src/limits.rs](../../crates/brain-loophost/src/limits.rs).
- Current reference: [crates/brain/src/session/config.rs](../../crates/brain/src/session/config.rs).
- Current reference: [docs/reference/configuration.mdx](../../docs/reference/configuration.mdx).
- Builds on: [ADR-001: Make Brain an independent runtime with injected execution ports](2026-08-20-01-standalone.md), [ADR-024: Use one SessionConfig and keep hashes local to their actual purpose](2026-09-04-02-simple-config.md), [ADR-042: Each crate renders its own contract into its own generated directory](2026-09-06-01-per-crate-contracts.md).
- Bounded by: [ADR-038: Defer dynamic placement, workflow durability, and advanced tenancy policy](2026-09-05-07-roadmap-boundary.md).
- [Claude session `9bed54a9-c1f9-48eb-aba8-de8bcbcbe55f`](SOURCES.md#session-9bed54a9-c1f9-48eb-aba8-de8bcbcbe55f), 2026-09-06T11:42:37.985Z: User deletes the kv key cap because Brain does not know the machine it runs on, and asks for a sweep of similar limits.
- [Claude session `18c350ff-58c3-400d-b851-0877627b8a8d`](SOURCES.md#session-18c350ff-58c3-400d-b851-0877627b8a8d), 2026-09-06: User asks that every limit become an injected default overridable by environment variable before the server starts, and requests this record.
- [Claude session `18c350ff-58c3-400d-b851-0877627b8a8d`](SOURCES.md#session-18c350ff-58c3-400d-b851-0877627b8a8d), 2026-09-06: User chooses one shared argument definition for the loop worker, with no version, as an internal detail.
- [Claude session `18c350ff-58c3-400d-b851-0877627b8a8d`](SOURCES.md#session-18c350ff-58c3-400d-b851-0877627b8a8d), 2026-09-06: User confirms zero as no bound, per-crate structs with one shared component that reads the environment and injects them, deleting the operator ceiling, and model limits as outside Brain's control.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
