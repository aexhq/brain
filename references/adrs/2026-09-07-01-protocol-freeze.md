# ADR-045: Resolve protocol gaps before stable v1

- Proposal date: 2026-09-07
- Status: Accepted; pre-v1 implementation

This record covers custom Event provenance, preservation of model content/continuation
state, inline durable state operations, error fidelity, and child cancellation. It records their context,
decisions, alternatives, and validation requirements together.
The model-state decision refines [ADR-012](2026-08-30-01-provider-model.md). This does not declare
the entire v1 API frozen. The original reproductions below describe the pre-change revision.

## 1. Attribute extension Events to their originating execution

### Context

Agentloops and Tools can both append custom Events through their invocation-scoped
`emit` service. Brain authenticates the callback and limits its services, but the public
Event records only its sequence, timestamp, type, and caller-supplied data. It does not
record which execution emitted it. The current Tool service delegates emission to the
same `TurnHost` implementation used by the Agentloop.

That omission matters when independently authored extensions share a session. A loop
may record a checkpoint or pending decision in an Event and reconstruct it after an
interrupted turn. A Tool may emit progress or observations into the same feed. The
consumer needs to distinguish the loop's state from the Tool's observations; choosing
a distinctive event name does not establish that distinction.

A protocol spike against Brain revision
`539c7deb3182837feaaf2b4199c3c122faad9eb2` reproduced this through a real Linux server
and separate HTTP placements:

1. The Agentloop emits `loop_checkpoint` with its transcript and kv.
2. A dispatched Tool uses its own valid callback to emit `loop_checkpoint` with
   fabricated state.
3. The loop fails before returning its final transcript and kv.
4. On a new turn, recovery that selects the latest checkpoint installs the Tool's state.

Both checkpoints have the same public envelope. A producer field inside `data` would
also be supplied by the emitter. The spike demonstrates a provenance gap and a
consumer that misuses it, not a checkpoint subsystem built into Brain. In particular,
it qualifies the otherwise successful experiment using Events for loop recovery.

The tested execution controls still held: the Tool could not call the model, dispatch
other Tools, emit reserved lifecycle Events, use an unauthorized placement, or reuse
its callback after completion. Event impersonation does not widen sealed authority.
It can influence already-authorized behavior when a consumer mistakes Tool data for
trusted loop state. The same attribution question applies to custom user-facing output.

### Decision

Brain assigns execution provenance to every extension-emitted Event, persists it with
the Event, and exposes it consistently through event pages, live recorded Events, and
Agentloop event reads.

The envelope adds `origin` with:

- `kind`: `agentloop` or `tool`;
- `sequence`: the existing `activation_started` or `tool_call_started` record sequence
  for that execution, within the Event's session.

The Event's own sequence remains its identity. Its origin references another record;
it introduces no new identifier. Tool name and Environment placement are resolved from
the referenced start record rather than copied into every Event. Brain derives both
origin fields from the granted service context, never from extension arguments.

The `emit(kind, data)` authoring interface remains unchanged. Custom event names remain
available to both extension kinds. Consumers that interpret an Event as loop state
must check its Brain-assigned origin before using it; a Tool may still use the same
name, but cannot make its Event appear to originate from the Agentloop.

This establishes which execution grant submitted an Event, not the truth of its
payload, a human's approval, or the integrity of code inside a compromised Environment.
Approval authorization remains application policy. It also does not introduce automatic
replay or settle transcript/checkpoint semantics.

### Alternatives considered

- **Names or producer fields inside payloads:** the Tool can supply either, so they do
  not authenticate the source.
- **Reserve checkpoint names for loops:** ties Brain to extension-specific policy and
  does not solve attribution for other custom Events or concurrent Tool observations.
- **Keep trusted state elsewhere or sign extension payloads:** applications can do this,
  but every consumer must supply machinery for execution identity Brain already knows.
- **Infer the producer from nearby start/end records:** concurrent executions make
  ordering alone insufficient to identify which one emitted an Event.
- **Add a dedicated loop checkpoint service:** may address another state-management
  requirement, but does not establish provenance for the rest of the custom event feed.

### Consequences and compatibility

This changes the Event contract and its durable representation. Execution identity must
reach the emitter through native, host, and HTTP service paths, including concurrent
Tool calls; using only the parent activation identity would lose the distinction.
The implementation must update the Rust source types and regenerate affected schemas
and SDK types. The proposal does not require another WIT service or another Environment
operation.

Previously recorded custom Events have unknown origin. Do not backfill provenance from
their names or payloads. Old records remain readable with absent origin; consumers must treat that absence as
unauthenticated. New records persist the complete envelope in an internal journal frame. Absence must never be
interpreted as proof of Agentloop authorship. New extension Events must carry origin.

### Acceptance criteria

1. The reproduction retains the authentic loop checkpoint when the consumer checks
   origin; the Tool's identically named Event remains readable as a Tool observation.
2. Native, host, and HTTP Tools receive the same attribution semantics. Concurrent
   calls are attributed to their respective Tool-start sequences, and loop emissions
   to their activation-start sequence.
3. Origin survives process restart and agrees across historical pages, recorded live
   delivery, and Agentloop reads.
4. Supplying a forged origin in payload data cannot alter the envelope. Existing
   reserved-event and expired-callback protections continue to pass.
5. The chosen handling of historical Events is tested. Updated generated contracts,
   SDK consumers, and required release CI pass before promotion.

### Sources

- [Current Event type](../../crates/brain-protocol/src/client/session.rs).
- [Agentloop and Tool emission](../../crates/brain/src/session/actor.rs).
- [Invocation-scoped callback grants](../../crates/brain-server/src/executions.rs).
- [Environment execution services](../../crates/brain-sessions/src/execution.rs).
- [ADR-040: Existing record identity](2026-09-05-09-session-names.md) and
  [ADR-044: Execution and service boundaries](2026-09-06-03-sessions-and-brain-env.md).
- Workspace evidence in the separate private research repository:
  [reproduction](../../../aex-research/experiments/brain-protocol-launch-2026-09-07/composition.test.mjs)
  (`a Tool can impersonate a loop checkpoint because custom events carry no authenticated emitter`),
  [result](../../../aex-research/results/brain-protocol-launch-2026-09-07/02-composition.json)
  (`custom_event_provenance`), and
  [investigation context](../../../aex-research/docs/brain-protocol-launch-findings-2026-09-07.md).

## 2. Preserve model content and adapter-owned continuation state

### Context

Every Agentloop uses Brain's shared model request, result, and transcript types.
Currently, messages have user or assistant roles and text, tool-use, or tool-result
blocks. The model adapter normalizes provider responses into that representation.

Some model workflows require more than the visible answer and tool call. A provider
can return continuation state that must accompany subsequent requests, including
requests carrying tool results. An Agentloop cannot preserve information that the
adapter has discarded or that the shared result type cannot represent. Loop kv does
not repair that loss.

The same boundary affects visual agents. A Tool can return arbitrary JSON containing
an image, but carrying those bytes through an Environment and journal does not establish
that the model receives image content.

### Evidence

Probes against Brain revision `539c7deb3182837feaaf2b4199c3c122faad9eb2` exercised
public HTTP callbacks and the shipped `RemoteModelClient` against local HTTP/SSE fixtures:

| Case | Observed behavior |
| --- | --- |
| Anthropic-shaped thinking/signature stream followed by text | Thinking is ignored, then its block-stop event fails with `provider completed absent content block 0` |
| Compatible Chat stream containing `reasoning_content` and a visible answer | Answer succeeds; reasoning content is absent from the returned result |
| Image-shaped content inside a Tool result | Anthropic rendering turns the image blocks into a JSON string rather than image input |
| Developer role, native reasoning/thinking blocks, image input, or provider options through the model callback | Rejected before the provider is called |
| `response_format: null` on a session configured for JSON output | Inherits the session format; cannot express a neutral reset |

These are controlled representation and codec tests, using synthetic content. They do
not establish live provider acceptance, cryptographic validity of a signature, vision
quality, or the correctness of a proposed replacement.

The continuation requirement has concrete provider examples: Anthropic documents
unchanged preservation of signed/redacted thinking blocks; DeepSeek documents reasoning
content preservation in tool-enabled thinking conversations; OpenAI compaction returns
opaque continuation state and retained items. Their shapes differ. Neutral text/tool
normalization alone cannot carry all three faithfully.

### Decision

Retain common representations for text, tool calls/results, and supported media. Add
a bounded representation for adapter-owned continuation state or native items that
survives model results, journal persistence, subsequent requests, and process restart.
The receiving adapter validates and renders that state for the session's fixed model
binding. Common tool-call identity, error status, and usage remain available to Brain.

The representation must preserve provider-significant ordering and the association of
state with messages, tool turns, or the context window. The spike must determine which
of those scopes are needed; this ADR does not assume that one metadata object on every
message is sufficient. Required opaque state must not be converted to prose, silently
dropped, or silently translated for an incompatible adapter.

Separate model data from execution authority. Neither opaque state nor per-request
provider options may select model credentials, transport destinations, or additional
Tools/Environments. Options are validated by the already-selected adapter. Unsupported
state or options fail explicitly. Size limits use Brain's deployment-limit conventions
to bound provider output and retained input, rather than unbounded generic metadata.

The Agentloop continues to own context selection and presentation. It may omit context
according to its policy, but retaining an item must preserve its provider-required
contents. A valid continuation can require keeping related items together. Those
requirements must be expressible and tested without teaching every loop the full
provider wire format.

Also settle explicit inherit/set/reset semantics for model presentation options.
For media, validate the path from user input or Tool output through the transcript to
the provider request. A generic JSON payload alone is not proof of modality support.

### Alternatives considered

- **Freeze the existing text/tool surface:** a viable narrower product promise, but
  excludes the demonstrated native-state and visual workflows. Later expansion affects
  every consumer of the shared model representation.
- **Add each provider feature directly to common types:** strong static typing, but
  repeated provider evolution changes the central contract and some state has no shared
  semantics.
- **Use arbitrary provider request/response JSON throughout:** preserves information
  but exposes provider wire formats to loops and obscures the common execution semantics
  ADR-012 established.
- **Keep state only inside a stateful adapter:** avoids some message changes, but
  requires a durable association with the exact context across restart and compaction.
  It cannot be assumed equivalent to journaled continuation state.
- **Call models as ordinary Tools:** JSON can carry the data, but the call then has Tool
  budget, usage, streaming, and credential semantics. It is not an equivalent repair of
  the public model service.

### Representation and validation

The common contract adds image blocks, user-input and Tool-result media, a developer role,
`ModelRequest.options`, and ordered `native { format, data }` blocks. Adapter format names
are `anthropic.messages.v1`, `openai.chat.v1`, and `openai.responses.v1`. Anthropic retains
signed/redacted thinking, Chat retains `reasoning_content`, and Responses retains native
reasoning/compaction items. Compaction returns retained items in one message; rendering
restores the item sequence without interpreting encrypted contents. Existing common
Tool-call identities and outcomes remain available to the loop.

`response_format` distinguishes omission (inherit), a value (set), and explicit null
(reset). Provider options use adapter allowlists; credentials, destinations, model
bindings, and execution authority cannot be overridden through them. Input/output limits
bound retained data. Unsupported formats fail instead of being translated or dropped.

The validation cases are:

1. Complete an Anthropic signed-thinking tool turn, persist its context, restart the
   Brain/adapter processes, and continue without changing the required blocks.
2. Complete a distinct OpenAI native-state flow, including a tool turn and provider
   compaction, then persist, restart, and continue with the returned state/items intact.
3. Verify an image from both user input and Tool output reaches the provider as image
   content; test explicit format reset after a session-level format was configured.
4. Exercise unsupported adapter/state combinations, bounded payload rejection, and
   attempted authority changes through options. Preserve ordinary text/tool behavior,
   effect journaling, error status, and absent usage counters.
5. Use deterministic fixtures to locate contract failures, then bounded live calls to
   validate provider acceptance. Passing JSON serialization alone is insufficient.

Deterministic tests cover codec rendering/streaming, ordered media, format reset,
unsupported options, native-state size bounds, and journal reopening. Bounded live
calls using the actual Rust client passed Anthropic signed-thinking Tool continuation
and OpenAI reasoning Tool continuation across separate processes through Vercel.
OpenAI compaction of completed visible context and a separate-process continuation
also passed with the returned compaction items intact.

The same live run found an endpoint compatibility limit: Vercel's generated reasoning
uses a `gwenc1` wrapper, while its compaction route forwards directly to OpenAI, which
rejects that wrapper. The successful compaction probe explicitly selected visible
context after the Tool turn completed. Brain does not strip that state automatically.
Direct OpenAI credentials were unavailable; this evidence does not establish direct
endpoint acceptance of every native flow or arbitrary gateway portability.

### Consequences and compatibility

Changed surfaces are UserInput, Message/content, ModelRequest/ModelResult,
model streaming, journal projections, SDKs, and generated schemas. A new Tool execution
or Environment lifecycle operation is not justified by the current evidence.

Adapter-owned state narrows provider coupling but does not make retained state portable
across arbitrary providers or adapter versions. Its ownership and compatibility rules
must be explicit. Old text-only context remains readable. Existing Agentloop Components must be rebuilt
against the new WIT and sessions recreated with the new immutable implementation, under
[ADR-030](2026-09-04-08-clean-break.md). Do not reconstruct state that was
previously discarded or imply that an SDK update migrates journals.

Acceptance requires the design spike above, regenerated contracts from Rust sources,
documentation of the resulting support boundary, and the required release CI. This
proposal does not claim full provider parity or change the fixed-authority and
send-once rules.

### Sources

- [Shared message types](../../crates/brain-protocol/src/model/message.rs),
  [model-call types](../../crates/brain-protocol/src/model/call.rs), and
  [user input](../../crates/brain-protocol/src/client/session.rs).
- [Model client](../../crates/brain/src/model/http.rs),
  [Anthropic codec](../../crates/brain/src/model/anthropic.rs), and
  [Chat codec](../../crates/brain/src/model/openai.rs).
- Provider requirements reviewed for the September 7 investigation:
  [Anthropic thinking](https://platform.claude.com/docs/en/build-with-claude/extended-thinking),
  [DeepSeek thinking](https://api-docs.deepseek.com/guides/thinking_mode/), and
  [OpenAI compaction](https://developers.openai.com/api/docs/guides/compaction), and
  [Vercel compaction forwarding](https://vercel.com/docs/ai-gateway/sdks-and-apis/responses/compaction).
- Workspace evidence in the separate private research repository:
  [codec probe](../../../aex-research/experiments/brain-protocol-launch-2026-09-07/model-probe/src/main.rs),
  [codec results](../../../aex-research/results/brain-protocol-launch-2026-09-07/05-model-codecs.json),
  [callback results](../../../aex-research/results/brain-protocol-launch-2026-09-07/01-state.json), and
  [investigation](../../../aex-research/docs/brain-protocol-launch-findings-2026-09-07.md).

## 3. Persist author-directed operations inline within the turn

### Context

The current interface already lets an author await model and Tool calls inside a turn.
Brain records effect intent before execution and records the outcome before exposing
it to the loop. Model requests also update the transcript immediately. In contrast,
loop kv is only persisted when the loop returns `TurnOutput` successfully.

The interruption spike exposed that asymmetry: a completed turn stored the original
conversation and `kv.phase = ready`; the next turn sent temporary classification input
to the model; Brain was killed during the request. Recovery yielded the classification
transcript and kv from the previous completed turn.

The design preference is to give authors inline durable state operations alongside
model and Tool calls. A whole-turn transaction would constrain ordinary imperative
code and delay state persistence to protect against an interruption that can instead
be represented as partial progress. The invariant should be durability of completed
operations, not atomic completion of the whole application turn.

### Decision

Expose explicit transcript and kv mutation services to the Agentloop. Each awaited
mutation returns after its change is durably committed. Writes are ordered in the
session journal; a later failure does not discard earlier committed state changes.
Brain commits each operation automatically; authors do not call a separate `commit`
or open a transaction. Optimize the initial interface for ordinary inline authoring.
Model and Tool calls retain their existing commit-before-effect and recorded-outcome
rules. Authors can interleave these operations in ordinary control flow without
returning a declarative plan for Brain to execute.

Persisting a mutation means invoking the service; changing an ordinary local object
does not implicitly persist guest memory. `set_transcript(messages)` and `set_kv({ key, value })` return the committed sequence.
WIT names are `set-transcript` and `set-kv`; its JSON strings follow the generated contracts. An individual state write must have a defined commit boundary,
but the protocol need not make several separate writes and external effects atomic.
Authors decide ordering and how to interpret partial progress on the next activation.

A crash can occur between two writes, or after a remote effect begins but before its
outcome is known. Recovery exposes committed state and effect records and reports the
interrupted turn honestly. It does not roll back completed operations, replay the
function, or infer that a pending effect never happened. Related application state can
be represented in one value when an author needs to update it together; a general
transaction interface is not required by the current evidence.

Only explicit transcript writes change the saved conversation. Model-start records retain
the actual request context as a delta against the saved transcript at a referenced journal
sequence; model-result records retain the complete result. Auxiliary calls do not replace
conversation state, and repeated requests do not copy the whole transcript into the log.

Choose one authority for persisted state. Under this direction, turn completion
reports its result and terminal status; it must not overwrite inline state changes
with a stale transcript/kv snapshot returned by the function. `TurnOutput` contains only the optional result; old transcript/kv fields are rejected.
Tools do not inherit the Agentloop's state-mutation services.

### Requested stopping and crashes

For an explicit graceful stop or shutdown, drain active turns before releasing their
execution. Keep their granted services available while they finish, so they can save
state and complete outstanding work. Stop admitting work that would prevent the
requested drain from finishing. The current cancellation path, which denies later
service calls and discards final returned state, is not equivalent to draining.

Document a graceful stop as waiting for completion, not immediate cancellation. Existing
execution deadlines and forced termination remain distinct: they can interrupt work
and must preserve the honest partial-progress/unknown-outcome behavior above. Graceful
draining reduces planned interruption; it is not a crash-consistency guarantee.

### Alternatives considered

- **Checkpoint transcript and kv only at successful turn return:** keeps one final
  state boundary but loses unsaved author progress and creates the current asymmetry
  with effects that were already committed.
- **Return a declarative effect/state plan:** transfers control flow into Brain and
  constrains the ordinary inline authoring model without a demonstrated requirement.
- **Implement every state write as a custom Event:** mechanically possible, but forces
  each loop to define state projections and recovery conventions; provenance alone
  does not remove that authoring cost.
- **Treat a turn as one transaction:** external effects cannot generally be rolled back
  with local state, and the demonstrated use cases do not require that promise.

### Deferred roadmap

Author-controlled commits and automatic end-of-turn buffering are deferred beyond the
initial protocol release. Revisit them only when a concrete workflow needs atomic
transcript/kv updates across multiple operations; crash edge cases alone do not justify
adding transaction machinery to the current interface.

A future state batch could use commit start/end markers to hide incomplete state
updates during recovery. It must retain effect intents and outcomes independently:
discarding an incomplete state batch cannot undo an external model or Tool call, or
justify automatically repeating it. The marker design and recovery interface remain
unevaluated alternatives, not requirements for the initial release.

### Validation requirements

1. Implement a loop that interleaves model calls, explicit transcript writes, kv writes,
   and Tool dispatch through the public services in native and HTTP placements.
2. Kill Brain after acknowledged state writes and between successive operations.
   Confirm that committed writes survive and incomplete work is reported without
   automatic replay. Do not assert that separately written fields must share one
   application checkpoint.
3. Complete a remote mutation before interrupting the following kv write. Verify that
   the loop can inspect its recorded effect outcome and reconcile the missing state
   update on a new activation.
4. Request a graceful stop while a turn is running. Its remaining services and writes
   must work, its terminal result must be recorded, and execution must then drain.
   Separately exercise a deadline/forced stop without claiming graceful completion.
5. Prove that final turn output cannot clobber inline state and that Tool callbacks
   cannot invoke the new state services. Document operation ordering for concurrent
   calls and run generated-contract checks and required release CI.

### Sources

- [Current turn services](../../crates/brain/src/session/services.rs),
  [turn completion and model recording](../../crates/brain/src/session/actor.rs), and
  [TurnInput/TurnOutput](../../crates/brain-protocol/src/agentloop/turn.rs).
- [ADR-023: Inline turn services](2026-09-04-01-turn-services.md) and
  [ADR-035: Send-once effects](2026-09-05-04-send-once.md).
- Workspace [state reproduction](../../../aex-research/experiments/brain-protocol-launch-2026-09-07/state.test.mjs)
  and [results](../../../aex-research/results/brain-protocol-launch-2026-09-07/01-state.json).
- September 7 design discussion: prefer inline model/Tool calls and immediately
  persisted state mutations; drain active loops for requested graceful interruption.

## 4. Preserve failure information and classify uncertain outcomes correctly

### Context and correction

The HTTP Environment spike returned a failure with `retryable: true`, but the Tool
result exposed to the Agentloop lost that field. Environment failures cannot carry
structured `details`, although Tool outcomes can; an extra supplied field was ignored.
An `accepted` or `progress` receipt returned from Execute became a definite Tool error
with `ambiguous: false`, despite not establishing the outcome of the sent execution.

Preserve reported failure information through Environment, Brain, and Agentloop
interfaces. Align the existing error fields where necessary, and specify terminal
receipts for each operation. A dispatched execution without a conclusive outcome must
be represented as unknown. Retryability is advisory and does not prove that repeating
an external action is safe. Brain continues to send once without automatic retries.

The loop may pass the error to its LLM, consult another model, or apply application
logic. Brain prescribes no classifier or recovery strategy. These corrections do not
justify a richer error framework or automatic Environment recovery.

### Validation and scope

Verify field preservation through native and HTTP paths and journal reads after
restart. Cover nonterminal Execute responses and ordinary conclusive failures without
conflating them. Regenerate affected contracts and run required release CI. Field
alignment may change shared contracts; uncertainty classification is a runtime fix.

Evidence: workspace [composition results](../../../aex-research/results/brain-protocol-launch-2026-09-07/02-composition.json),
cases `error_fidelity` and `execute_terminal_contract`; current
[receipt types](../../crates/brain-protocol/src/environment/wire.rs),
[outcome types](../../crates/brain-protocol/src/environment/outcome.rs), and
[execution conversion](../../crates/brain-sessions/src/execution.rs).

## 5. Propagate cancellation to owned child sessions

### Context and intended behavior

The composition spike created a separate session inside a host Tool. Cancelling the
parent left that session running; explicitly cancelling the child succeeded. The user
expects delegated child work to stop when its owning parent is cancelled.

Ownership belongs to the caller integration. The SDK accepts an AbortSignal on
`SessionHandle.send`: a host Tool explicitly passes its execution signal to an exclusively
owned child's send. Cancellation arriving before admission waits for `turn_started`, then
issues cancellation once. Send waits for an issued cancellation request before returning.
Already-aborted signals refuse to start work. Use a fresh handle and do not share concurrent
sends with another owner; this convenience does not correlate arbitrary competing writers.

### Decision and validation

No Brain session-relationship field or scheduler is introduced. Independent sessions are
unchanged. The SDK tests early cancellation and pre-aborted ownership; a real host Tool
journey cancels a parent waiting on a child and verifies child termination, recorded unknown
outcomes, no replay, and an unaffected independent session. Graceful server draining remains
distinct from this force-cancellation path.

Evidence: workspace [child-session spike](../../../aex-research/experiments/brain-protocol-launch-2026-09-07/children.test.mjs)
and [results](../../../aex-research/results/brain-protocol-launch-2026-09-07/04-children.json).

## Scope exclusions and deferred work

- Author-controlled commits and turn-wide buffering remain deferred as described above.
- Further background-job work is deferred. Existing submit/poll Tools already compose;
  this review adds no background-job protocol or automatic wakeup requirement.
- Approval belongs to application-authored Tools using facilities such as `hostEnv`.
  No builtin approval feature, official approval Tool, or additional approval spike is planned.
- Environment recovery remains application/Agentloop policy, typically decided by the
  LLM using reported errors and authorized Tools. The lost-response durability check
  does not create a new recovery feature requirement.

[Index](README.md)
