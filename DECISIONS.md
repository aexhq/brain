# Design decisions

Decisions that change how Brain behaves, recorded before the code changes. Each entry says what
we decided and why. When a decision is reversed, add a new entry rather than editing the old one.
Every entry below was implemented in the pull request that added it.

## 2026-09-05: Standalone, ephemeral execution

These decisions supersede earlier lifecycle and residency choices below.

- Brain is a minimal, language-neutral runtime. Aex consumes its public contracts like any third
  party. Placement, workflow durability, tool policy, and cloud provisioning belong in extensions
  or platforms. No heavy workflow dependency is required.
- Prepare and admit Wasm artifacts before session creation. Cache compiled and prelinked code;
  activate each invocation in a fresh store. Parent turns and their nested tools have separate
  execution permits, so saturated parents cannot prevent child execution.
- Release the session actor and cached journal state after each turn by default. An explicit
  session idle TTL can keep execution warm; zero means no automatic suspension. History and
  transcript reads need no activation. Startup does not reopen every session journal.
- A checksummed, disposable checkpoint accelerates reopening. The canonical journal remains the
  source of truth. This avoids decoding unchanged history, not its growing index or transcript.
- Acknowledge local commits only after the supported filesystem flush boundary. Recover an
  unfinished turn as interrupted and expose that Event without automatically running the loop.
  Never retry tool or environment effects automatically; uncertainty stays unknown. A caller or
  model may explicitly choose a new action. External commit services remain possible through
  the store interface and are post-MVP.
- Agentloops can page Events and append their own observations. Official loops expose interruption
  and environment failures to the model. Live subscriptions are scoped to one session; durable
  Events come from the journal, while historical telemetry requires a configured sink.
- Tools keep fixed environment bindings for the MVP. Environment providers can allocate on first
  invoke and own TTL, restart, and externally attached resource policy. Brain does not restore a
  browser or sandbox's lost state. Reloaded environment metadata starts with unknown status.
- Named environment routes and their credentials are deployment grants. A session cannot choose
  an arbitrary destination for a server credential. Wasm capabilities remain restricted; stronger
  process or VM isolation and tenant resource fairness are post-MVP platform choices.
- The entire official `tool-env` tool, mutable bindings, tools without bindings, automatic placement,
  configurable resource bounds, and suspension inside model/tool waits remain post-MVP. The
  roadmap includes inspection and explicit lifecycle operations for model-driven recovery.
- This is a pre-launch clean break: the local data format is `brain-data/2`, and contracts and
  official consumers move together. No compatibility shims or old-data migration are provided.

## 2026-09-02: The agent loop owns what the model sees

**Decided.** The creator sets the system prompt, the response format, and the tool catalogue at
session create. The agent loop sees the prompt and the tools and may change what the model is
told on any call: a different system prompt, a subset of the tools, a different response format.
Nothing about the prompt is pinned.

**Today.** `ModelPresentation` (system prompt, tool definitions, response format) is supplied by
the application at create, canonicalised and hashed into a session-lifetime identity, and rendered
into every provider request. The loop controls the messages and the per-call response format only.
It cannot change the system prompt, offer a subset of tools, or reorder tools for prompt caching.

**Change.**

- `ModelRequest` may carry `system` and the list of tools to offer alongside `messages`. What
  the loop leaves out, the session fills in from what it was created with, so a loop that says
  nothing sends exactly what the creator asked for, and a loop that wants to differ can.
- The application still supplies the tool catalogue at create: name, schemas, program, `needs`,
  and environment binding. That set is admitted, validated, and provisioned into environments at
  attach. It is a dispatch and trust boundary, not presentation. The loop offers the model a subset
  of that catalogue by name; a name outside it is a rejected decision.
- The sealed `Presentation` bytes and identity go away. `Session.config_hash` goes away with them.
  Comparing two sessions' configuration is the caller's job and is done by reading and comparing
  the configuration, not a hash Brain precomputed. A precomputed hash is an optimisation nobody
  asked for, and it invites callers to trust equality they never checked.
- The activation identity, which hashes the presentation identity with the turn context and
  observation, is recomputed from the sealed configuration and the request the loop made instead.

**Why.** The loop is the policy. Brain's promise is that it does the I/O the loop asks for and
records it; deciding what a model is told is policy. Fixing the prompt at create was defended as
drift prevention, but drift is prevented by the journal, which records every request as sent.
A loop that wants a stable prompt keeps one. A loop that wants to swap prompts for a subtask,
offer fewer tools, or manage a cache prefix can now do so without a second session.

## 2026-09-02: The journal records a model request as a diff against the last one

**Decided.** A `model_intent` record carries the part of the request that differs from the
request recorded before it, starting at the first message that changed. The log stays append-only.

**Today.** `model_intent` records `messages[journalled_messages..]`, the messages past the count
already written. It assumes the loop only appends. `request_identity` still covers the whole
request, so a reader that rebuilds by concatenation can detect a rewrite, but the rewritten
messages themselves are not in the log.

**Change.** Compare the new request against the last recorded request from the front. Find the
first index where a message differs, including the system prompt as position zero. Record from that
index to the end, with `messages_from` naming the index. In the common case the loop appended and
the record is the tail, as today. When the loop rewrote history, summarised, or changed the system
prompt, the record restarts from the changed position and carries everything after it. Nothing
already written is touched. A reader rebuilds the request by taking the previous rebuilt request up
to `messages_from` and appending the record's messages.

**Why.** The delta is what keeps the journal linear in the number of turns, which we measured and
want to keep. The prefix rule keeps that property for every loop behaviour rather than only for
append-only ones, and it makes the recorded log the complete truth of what was sent, so replay
needs no identity check to trust it.

## 2026-09-02: Effect records are named for what happened, not for intent

**Decided.** Every lifecycle on the feed uses the same three words: `started`, `ended`,
`failed`. `model_intent` and `model_result` become `model_call_started` and `model_call_ended`,
and a model call the session could not complete is `model_call_failed`. The same for tool calls,
cancellations, activations, and environment operations. `turn_finished` becomes `turn_ended`, a
turn cut off by a restart is `turn_failed` with code `interrupted`, and `session_created` becomes
`session_creation_ended` beside `session_creation_started` and `session_creation_failed`. The
`*_ambiguous` kinds go: a failed record carries `ambiguous: true` when the effect may have
happened anyway. The write-ahead behaviour is unchanged: the started record lands before the
effect, the ended or failed record before the next activation.

**Why.** "Intent" describes the mechanism from the inside, and "finished" and "created" were two
more words for the same thing. A reader of the event feed wants to know what happened, in one
vocabulary.

## 2026-09-02: No request identity

**Decided.** `request_identity` is removed from journal records, from `ModelExecutor::execute`,
and from the environment wire (`invoke`, `cancel`, and their receipts).

**Why.** It was a hash of the request body doing three jobs. Detecting a rewritten history is
unnecessary once the journal records the prefix diff and holds the whole truth. Correlating a
receipt to its operation is done by the operation id, which is already deterministic and already
echoed; the same id with a different body can only be a Brain bug. Rejecting an idempotency key
reused with a different body stays, but as a comparison inside the store, not a named concept.

## 2026-09-02: A session has two ids, `session_id` and `sequence`

**Decided.** A record is named by `(session_id, sequence)`. The event id `evt_{session}_{seq}` is
that pair spelled out. A model call or tool call is named by the sequence of its `*_started`
record; a result, a cancel, or a client-posted tool outcome refers to it by that sequence. There is
no `journal_id` and no `operation_id`. `operation_id(journal_id, position)`, `OperationAllocator`,
the server's journal metadata file, and `Kernel::adopt_journal_ids` are removed.

**Why.** A journal is always one session's journal, so it needs no id of its own. `journal_id`
existed only to salt the operation id so it could not be guessed, and nothing needs that: an
environment is an authenticated peer, and a client answering a tool call already holds the
session's share key. Because the salt was kept out of the log on purpose, a restart could not
rebuild it from the log; it lived in a second store and `adopt_journal_ids` handed it back, with a
failure mode in which a restored session was readable but could not take another turn. With the
salt gone, a restart rebuilds everything from the log given the session id.

**Rule.** Do not introduce a new identifier or term when an existing one names the thing. Use
`session_id` and `sequence` consistently on the wire, in the journal, and in the API.

## 2026-09-02: The kernel is one session; the server manages sessions

**Decided.** The `brain` crate is a per-session runtime. The server creates a session by handing
it a session id, a journal store, the three executors, and the sealed configuration. The session
exposes `start`, `message`, `cancel`, and `end`, and hooks the server can attach to observe it. It
increments its own sequence and journals its own records. It knows nothing about other sessions.

The server owns everything across sessions: the map of sessions running in memory,
what to restore after a restart, credentials and their encryption, HTTP idempotency, listing, and
authorisation.

**Today the line holds for** credentials (the kernel sees a `binding_id`, the server resolves it
to a key at call time), executors (injected through `LoopExecutor`, `ModelExecutor`,
`ToolExecutor`), and per-session work (the turn driver, sequencing, write-ahead records, cancel,
parked client-hosted tool calls).

**Today the line leaks in** three places, each to be moved to the server:

- `Kernel` holds the map of sessions running in memory, rebuilds every session's index from
  disk at open, and lists them. The server holds the map and decides what to load and when.
- `Kernel::open` creates the segment journal itself. The store is injected; the crate ships
  `SegmentJournal` as an implementation the server may choose.
- `Kernel::idempotency_get` and `idempotency_put` dedupe HTTP retries inside the journal. That
  table moves to the server's own store, beside the credentials.

**Why.** A runtime that owns one session is easy to reason about, test, and embed: everything it
needs arrives through its constructor and everything it does is on its own log. Placement,
what to keep in memory, recovery policy, and credentials are product decisions, and the embed guide already
promises they stay with the host. The kernel stopped short of that promise by keeping a registry
and a store of its own, and the idempotency table is the visible cost: an HTTP concern stored in
the session log because that was the only durable place.

**Naming.** The word "kernel" goes. `Kernel` today names the registry plus its store, which is
what this decision removes; what remains is a session, and the crate calls it that. The
`brain-protocol` struct that the API returns as a summary keeps a name that says it is a summary.
The session may mint its own id as long as the server records and manages it.

## 2026-09-04: The agent loop drives the turn; Brain provides services

**Decided.** One activation is one whole turn. The loop receives the input, the transcript, and
the session's records since it last ran, and calls back into Brain through host imports for as
long as the turn lasts: `model` makes one model call, `dispatch` runs one or many tool calls
together, `emit` writes the loop's own Event, `telemetry` is fire and forget. Brain performs
each call, journals it first, and hands the result back. The loop returns the transcript and its
slots when it decides the turn is over.

Brain owns the transcript's persistence and the loop owns its content. The transcript is the
neutral `Vec<Message>` the protocol already had; the loop edits it however it likes and hands it
over at every model call and at the end of the turn, and Brain journals the difference from the
last one it recorded. Compaction is the loop replacing the transcript with a shorter one.

**What went away.** The decision loop in the session actor, the `step` activation with its
observation and decision types, the resident context in the Loophost, the decision cap and
`BRAIN_MAX_DECISIONS`, model request defaulting in the actor, and the step-shaped `agentloop/v1` contract, replaced in
place by the turn-shaped one under the same name (contracts are rewritten, not versioned, until
Brain 1.0.0).
The turn is bounded instead by a model-call budget (`BRAIN_MAX_MODEL_CALLS`, failure code
`model_call_limit`) and a wall-time budget (`BRAIN_MAX_TURN_SECS`, which cancels the turn).

**Why.** The loop is the policy, and a policy that can only answer one question at a time
cannot run tool calls sequentially, skip a call the model asked for, or retry a model call on
its own terms without a second activation per step. Brain keeps authority because effects are
reachable only through its services, each of which journals before it acts. Cancellation
reaches the loop as an error from the next service call, which the loop propagates.

**Host imports are asynchronous.** The SDK presents them as promises and Wasmtime suspends the
guest while Brain or WASI performs I/O. A fixed 10 billion fuel units replace the former
two-second epoch deadline: fuel bounds guest instructions while suspended model, Tool, HTTP, and
filesystem waits consume none. Fuel is a work allowance for this Wasmtime version, not a promise
of an exact CPU duration. `BRAIN_MAX_TURN_SECS` remains the independent whole-turn wall deadline.

## 2026-09-04: One session configuration, and nothing is sealed

**Decided.** A session has one configuration type, `SessionConfig`: the request carries it,
the store records it, and the runtime reads it. `ResolvedSessionRequest`,
`SealedSessionConfig`, `RequestedToolBinding` against `ToolBinding`, `ResolvedEnvironment`
against `EnvironmentAttachment`, the `EnvironmentView` trait, and the second contract
validation at `complete` are gone. The host-wide struct that carries executors and limits is
`SessionRuntime`.

**Why.** Two types for the same thing at two moments of its life cost every change twice and
protected nothing: the invariant they were meant to express is that a session's tool
catalogue and bindings do not change after create, and that holds because nothing after
create writes them. The word "sealed" goes with the type; the invariant stays and is stated
in `AGENTS.md`.

## 2026-09-04: Identity is an idempotency key and nothing else

**Decided.** `Identity` is a plain 64-hex newtype. Hashing lives in the server's digest
module and is used for HTTP idempotency keys and the configuration digest an idempotent
replay is checked against. The environment configuration check at attach, the binding
fingerprints sent to environments, and the hashed attachment id are gone; an attachment id is
random.

**Why.** Configuration belongs to whoever creates an environment, and attach names an id, so
there is nothing to compare. A fingerprint the other side never verified was a promise
without a check.

## 2026-09-04: One directory per session, two append-only logs, one sequence

**Decided.** Every session owns `sessions/{id}/` under the data directory, holding its
creation configuration and two append-only logs written by one process-wide writer thread with
a per-session backlog budget. The **events** log holds lifecycle records, effect records, and
the loop's own records; it is the audit trail and the client feed. The **journal** holds
the transcript as prefix deltas (`keep k, append rest` against the last recorded transcript),
the loop's state slots as last-write-wins values, and checkpoints holding the whole transcript
and every slot. A checkpoint is written when the bytes appended since the last one exceed the
transcript's size. One sequence counter numbers the records of both logs. No file is rewritten
in place; delete is directory removal.

**What went away.** The shared segment log, cross-session segment rotation and reclamation,
`ObservedJournal`, the in-memory-only context, and the per-session state file the old module
documentation described but no code wrote.

**Why.** A context that is only in memory is what forced replaying records into the loop on
restart. Deltas keep the journal linear in the conversation's length for every loop behaviour,
not only append-only ones; checkpoints bound recovery to about twice the transcript; one
sequence keeps the order between a transcript change and the effect that followed it. One
writer is enough as long as sessions do not block each other, which the per-session budget
guarantees; sharding by session id is a measurement away if a single thread ever saturates.

## 2026-09-04: Every dependency of a session is injected, and recovery is load and construct

**Decided.** A session is built from its store, the shared `SessionRuntime`, and its
`SessionConfig`; `Session::begin` and `Session::open` are the same construction with and
without a genesis record. Recovery folds the journal from the last checkpoint and reads the
events log. A turn the last process left running is closed with `turn_failed` whose code is
`interrupted`. No activation runs, nothing is replayed into the loop, and the loop learns what
happened from the events it receives at its next turn.

**What went away.** `take_restored`, `restored_history`, the ten-thousand-record replay cap,
`announce_history`, the `session_history_replayed` record, and the `history` field of the
create request. A conversation is carried into a new session as `transcript`: messages, not
events.

**Why.** The state on disk is the whole state, so there is nothing to replay. A loop that
never sees recovery cannot get it wrong.

## 2026-09-04: Idle sessions are suspended and rebuilt on demand

**Decided.** A session is created with `idle_ttl_ms`, or takes the server's
`BRAIN_SESSION_IDLE_TTL_SECS`. Idle past it, the session's task and memory are dropped after
its writes drain, and a `session_suspended` record is written. The next request rebuilds it
from its logs and writes `session_resumed`. On boot every session on disk is registered
suspended. `end` is still the only thing that detaches environments and writes
`session_ended`.

**Why.** Ten thousand idle conversations should cost disk, not memory, and a restart should
not have to rebuild every one of them before serving the first request.

## 2026-09-04: Environments are resources with an optional managed lifecycle

**Decided.** An environment is created with its configuration at `POST /v1/environments`,
named by its id, attached to by sessions at create, shared by any number of sessions at once,
and closed by `DELETE`, which is refused while a session is attached. A managed environment is
closed by Brain once no session has been attached for its idle TTL. Every session that was
attached sees `environment_closed`; one whose environment cannot be reached sees
`environment_unreachable`. Tools bind to environments by id; the SDK creates a managed
environment per Environment object a session's tools name and deletes it when the session
ends.

**What went away.** Inline environment declarations in the create request, the
session/shared/external `LifecyclePolicy`, provisioning on first sight of an id, and the
environment directory that compared configurations.

**Why.** A place that runs programs outlives the conversation that first needed it, and the
next conversation should be able to find it by name. Lifecycle is either the creator's or
Brain's, and the flag says which.

## 2026-09-04: One catalogue of codes

**Decided.** `brain_protocol::codes` declares every record kind, every failure code, and every
API error code, mirrored by `contracts/session/v1/codes.json` and held identical by a test.
Every `*_failed` record carries the same `Failure` payload: `code`, `message`, `retryable`,
`ambiguous`. Errors inside the runtime are typed variants whose code is read from the
variant, never from the text. A loop that fails a turn chooses its own code, and that code is
what the `turn_failed` record carries; a loop that trapped or returned something the contract
refuses gets `agentloop_failed`. Codes nothing emits are removed rather than kept for
company.

**Why.** A client, a loop, or a test matching on a string that is declared once cannot drift
from the producer.

## 2026-09-04: The data directory is not migrated

**Decided.** The data directory carries the `brain-data/1` format marker. An unrecognized format
or a nonempty directory without a marker is refused. There are no legacy layouts or migrations.

**Why.** Brain is under early development and has said so: contracts are replaced in place
until the first stable release. A migration path for a layout nobody should be depending on
is code that has to be right for no one.

## 2026-09-04: The Rust types are the source of the contracts

**Decided.** `crates/brain-protocol` and the route annotations in `crates/brain-http` define the
wire. `contracts/` is rendered from them by `cargo run -p brain-contracts` and diffed in CI. Only
`agentloop.wit` and the examples are written by hand.

**Before.** The JSON Schemas and the OpenAPI document were written by hand, and so were the Rust
types. The original typify generator had rotted and been deleted (#156), leaving two hand-written
descriptions of every message, kept in step by example-based conformance tests. The contract
digests had no reader: no admission check compared them, the SDK re-exported them to nobody, and
CI only checked that they matched files that were already in the diff.

**Why this direction.** The server is Rust, and the behaviour that a schema cannot express (a
validated identity, an overflow-checked usage merge, a deterministic seed) already lives on the
Rust types. Generating Rust from JSON Schema produces types without that behaviour and was the
pipeline that failed. Generating the schemas from Rust keeps one hand-written source and makes
the published files a build product.

**Change.**

- Every boundary type derives `schemars::JsonSchema`; the constraints the schemas carried
  (identifier and identity patterns, length bounds, the client-hosting rule of a bound tool, the
  resource policy shapes) are attributes on the fields that enforce them, or explicit schema
  fragments next to the type.
- Each HTTP handler carries a `#[utoipa::path]` annotation. The router registers routes from
  those annotations and refuses to start if the set of routed operations differs from the
  document, so a handler cannot ship undocumented or documented-but-unreachable.
- The OpenAPI document is self-contained: the session schemas are embedded as its components
  rather than referenced across files.
- `AgentloopManifest` and `AgentloopPackage` move from `brain-loophost` to `brain-protocol`,
  where every other boundary type lives, and appear in the session contract.
- `EnvironmentCommand` and `EnvironmentOperation` lose their unused type parameter.
- The contract digests, `generated.rs`, `tools/generate-contracts.py`, the SDK's
  `contractDigests` export, and the unused `paths.ts` are deleted.

**Not changed.** The wire. Every checked-in example still validates, and no serde attribute was
added or removed, so Brain accepts and produces exactly what it did.

## 2026-09-05: One canonical journal, with transcript and Events as projections

**Decided.** Each session has one ordered append-only journal. Configuration, transcript changes,
Agentloop state, lifecycle changes, effect starts and outcomes all enter that journal. The public
Event feed and the transcript sent to an Agentloop are projections; Brain does not persist either
as a second copy. This reverses the two-log and checkpoint decision above. The data format advances
in place because Brain is pre-launch and does not migrate development data.

One process-wide writer owns sequence assignment. It accepts separate synchronous and asynchronous
lanes and always selects ready synchronous work first, so informational background writes cannot
queue ahead of a turn's latency-sensitive commit. Records become visible to projections only after
the journal append is flushed to disk. `SessionStore` defines this contract; the MVP implements it
with local machine disk, while an external store remains a later adapter rather than a second
durability path.

**Why.** Two logs duplicated ordering and recovery work without adding another source of truth.
The journal is already sufficient to rebuild the transcript, state, status, configuration and
Events, and one sequencer gives them one unambiguous order.

## 2026-09-05: Placement is explicit and execution has two forms

**Decided.** A placed Agentloop or Tool is bound to exactly one Environment at session creation.
The TypeScript API expresses this as `extension({ env, ...options })`; Brain never infers a runtime
from source language, installed packages or requested capabilities. An Environment may be a remote
service, an application, a sandbox provider, or Brain's built-in `brain_wasm` Environment.
For the MVP each Environment declaration belongs to one session. Brain does not expose a
standalone Environment API until it can truthfully attach an existing Environment across sessions.

Code already resident in an application is registered as a resident Tool without a synthetic
Environment. Brain sends commands to that host over one bounded event stream and accepts results
and Tool-emitted Events over HTTP. Losing the connection makes an in-flight outcome unknown; Brain
does not replay it. This same host protocol permits a latency-sensitive handler to be resident in a
trusted server process chosen by the application, without running customer code inside Brain.

**Why.** Where code runs is authority and lifecycle, not a language-detection convenience. Making
placement visible keeps custom Environments open-ended while keeping the common resident and native
paths small.

## 2026-09-05: Brain's native Environment runs Components in one worker process

**Decided.** `brain_wasm` accepts precompiled WebAssembly Components only. Brain does not bundle a
JavaScript, Python or Rust runtime and does not install dependencies. A bounded worker process owns
one Wasmtime engine and its compiled-component cache; every invocation receives a fresh Store and
component instance. Guests start with no ambient environment, stdio or network. The Environment may
request an isolated scratch directory, a session workspace, read-only secret files and exact
HTTP(S) origins or authorities. Requested filesystem roots, secrets, and network targets must also
be granted by the server deployment, so caller input cannot expose Brain process credentials,
worker-host network, or writable host storage. The hosted deployment grants none. The parent
accepts exactly the worker's eight concurrent
128 MiB Stores and refuses overflow instead of hiding a queue behind the worker liveness bound.
That bounds guest linear memory to 1 GiB, and the worker is restarted if it stops answering.
Each invocation also receives 10 billion Wasmtime fuel units. Exhaustion traps runaway guest
code without charging time suspended in Brain or WASI I/O. Cancellation or a lost worker-client
connection drops the in-flight Store immediately, releasing its execution slot.

This is component and process isolation, not a claim that Wasmtime is a virtual machine security
boundary. Code requiring an OS, language runtime or stronger tenant boundary belongs in an explicit
external Environment such as a MicroVM.

**Why.** Native execution is for small, latency-sensitive extensions. A single worker avoids a
process or VM start per call, fresh Stores prevent guest state leaking across calls, and the narrow
WASI surface is the smallest useful built-in runtime.

## 2026-09-05: Brain sends each effect once and reports the outcome

**Decided.** An effect's started record is durably committed before Brain dispatches it. Brain sends
the effect once and never retries automatically. It then commits the ended, failed or unknown
outcome before the Agentloop can observe it. Tool failures, Environment failures and uncertain
resident-host disconnects are Events; `ctx.emit` is the common extension operation for adding an
Event. One turn may emit at most 128 extension Events and 1 MiB across their serialized kinds and
payloads. Logs remain operational diagnostics and are never the only place an Agentloop-actionable
failure appears.

**Why.** A retry can duplicate an external side effect and only the Agentloop has enough policy to
decide whether another attempt is appropriate. Effect-after-commit preserves evidence of what Brain
tried without pretending exactly-once execution is possible.

## 2026-09-05: Preserve effect identity and uncertainty across interruption

HTTP request claims and resident host token hashes live in small durable server logs. Session
journals remain the only source for lifecycle and effect outcomes, including Environment teardown.
A request without a recorded answer is not executed again; an effect without a terminal record is
unknown. Ending is fenced before detach, and failed teardown retains its resource row for an explicit
retry. Session-owned model bindings avoid credential sharing across unrelated create operations.

This closes restart and cancellation gaps without introducing a database, retry engine, or background
effect scheduler. Metadata uses the same file synchronization and torn-tail repair rules at every
acknowledgment boundary. These changes define the pre-launch `brain-data/1` layout in place.
Wasm memory limits apply across all memories in a Store, and remote
response bounds apply while reading, not after buffering.
