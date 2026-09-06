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
  without inventing a large number. Trust-boundary limits keep a non-zero default. The three
  queue capacities (live backlog, host command queue, telemetry queue) are preallocated
  channels, not ceilings; they must be at least 1 and the server refuses to start otherwise.
- **Derived numbers are derived.** The loophost's 125-second worker liveness bound is the
  ceiling on a native Component's outbound HTTP request plus slack. It is computed from
  that injected ceiling, not kept as a separate constant that drifts.
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
| brain | `max_model_calls` | 128 | exposed already |
| brain | `max_turn_secs` | 1800 | exposed already |
| brain | `max_tool_secs` | 120 | was a constant |
| brain | `max_emitted_bytes` | 1 MiB | documented |
| brain | `max_model_output_bytes` | 192 KiB, now 4 MiB | was too small for 64k-token outputs |
| brain | `max_model_delta_bytes` | 64 KiB | |
| brain | `max_model_stream_bytes` / `max_model_frame_bytes` / `max_model_error_bytes` | 32 MiB / 256 KiB / 16 KiB | |
| brain | `max_model_secs` / `max_model_connect_secs` | 120 / 10 | |
| brain | `max_journal_queue_bytes` / `max_session_queue_bytes` / `max_journal_open_files` | 64 MiB / 8 MiB / 256 | `Writer::spawn_with` takes them |
| brain | `max_live_backlog` | 1,024 records | `Feed::with_limits` takes it; at least 1 |
| brain-loophost | `max_package_bytes` / `max_turn_input_bytes` / `max_turn_output_bytes` | 32 MiB each | `LoopLimits` |
| brain-loophost | `max_linear_memory_bytes` / `max_fuel` / `max_concurrent_turns` / `max_core_instances` | 128 MiB / 10^10 / 8 / 8 | `LoopLimits` |
| brain-loophost | `max_native_http_secs` | 120 | was a constant; the worker liveness bound derives from it |
| brain-telemetry | `max_telemetry_records` / `max_telemetry_bytes` / `telemetry_retry_secs` | 4,096 / 8 MiB / 30 | `telemetry_channel_with` takes them; the two capacities at least 1 |
| brain-http | `max_request_bytes` | 32 MiB | trust boundary; the 2 MiB session and message caps in `brain` collapse into this one |
| brain-server | `max_environment_response_bytes` | 2 MiB, now 32 MiB | would have failed the first Tool returning a file |
| brain-server | `max_environment_secs` / `max_environment_connect_secs` | 120 / 5 | |
| brain-server | `max_hosts` / `max_host_commands` / `host_unconnected_secs` | 4,096 / 128 / 60 | `max_hosts` is a trust boundary; the command queue at least 1 |
| brain-server | `request_retention_secs` | 86,400 | |
| brain-server | `session_idle_ttl_secs` | none | exposed already; absent means release after each turn |

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

Each limits struct is a clap `Args` with its `BRAIN_*` name, flag, default, and help on the
field, and `ServerConfig` flattens the five of them. The library crates depend on clap for the
derive but never call it; only the server parses. The configuration reference table is
rendered from that clap definition by `cargo run -p brain-server --bin contract`, part of
`npm run gen` and the CI diff guard, so a row cannot drift from the field it describes.

`Writer::spawn_with`, `Feed::with_limits`, and `telemetry_channel_with` take limits; the
zero-argument constructors remain and use the defaults. `LoopLimits` is constructed from
config and handed to the worker on its command line. The embed guide shows the structs being
filled. Embedders who relied on constants exported from `brain` switch to the `Default`
impls. Turn and Tool bounds are whole seconds now, so a test that wants a sub-second bound
sets one second.

`MAX_TRANSCRIPT_ITEMS` leaves the JSON Schema and the generated SDK types, a pre-1.0 rewrite
in place. A turn's transcript is bounded by turn input and output bytes instead.

The two limits already failing real workloads, assistant output at 192 KiB and environment
responses at 2 MiB, get larger defaults in the same change: 4 MiB covers a 64k-token answer
several times over, and 32 MiB matches the request cap so a Tool can return what a caller
can send. The reasoning is written next to each `Default`.

The roadmap item added when the kv cap was deleted, configurable kv bounds, is subsumed:
kv is bounded by turn output bytes and journal queue bytes, both deployment limits under this
record. It is replaced by an entry pointing here.

The supervisor and the worker no longer construct `LoopLimits::default()` independently. Before
this record they agreed only by coincidence; now the worker parses what the supervisor rendered.

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
