# ADR-048: Separate Tool return from finish and activate the loop from committed events

- Decision date: 2026-09-21
- Status: Accepted; implemented in the coordinated 0.28 release

Refines [ADR-023](2026-09-04-01-turn-services.md),
[ADR-035](2026-09-05-04-send-once.md),
[ADR-037](2026-09-05-06-ephemeral.md), and
[ADR-047](2026-09-12-01-tool-outcomes.md). The owner authorized implementation and
coordinated release, subject to the required CI and release gates.

## Context

A Tool may emit data, release its synchronous caller, and continue producing results
in the background. Previously, dispatch waited for one terminal outcome and Environment
completion closes the callback grant. Tool emission also shares its parent turn's
service state. That couples result availability, execution lifetime, and Agentloop
activation even though they can end at different times.

The review also found that removing a callback grant does not close calls already
admitted through it. A retained Agentloop service can commit state after the actor has
taken its final transcript/KV snapshot. Supporting background Tools must preserve
their own execution authority without retaining a completed Agentloop's authority.

An asynchronous result should wake the Agentloop, which may call the model, ignore
the observation, or save a decision for a later user message. Several events commonly
arrive together, especially a result followed immediately by completion. Activating
the loop separately for each adds unnecessary work.

## Decision

### 1. Emission, return, and finish have separate meanings

| Operation | Meaning |
| --- | --- |
| Emit | Commit data associated with this Tool invocation. |
| Return | End the synchronous phase and release the caller; unfinished execution may continue. |
| Finish | Commit the terminal execution event and close the Tool's emission authority. |

Synchronous means the caller is waiting, not that the implementation cannot use
asynchronous language operations. Results emitted before return are available to
the synchronous caller. Results emitted afterward remain valid and cause event-based
activation. Every result keeps the original session and Tool-start sequence identity.

Tools must explicitly finish. A plain return does not imply finish, and Brain does
not infer completion from outstanding promises, timers, threads, or remote jobs.
The owner accepts that a Tool which forgets to finish can remain open until an
applicable deadline, cancellation, or execution loss. An unlimited execution has no
additional timeout invented to compensate for a missing finish.

Support a convenient form such as `return ctx.finish(value)`, which emits the optional
result and finishes without publishing the value again when the handler returns.
If data was already emitted, `await ctx.finish(); return;` is valid. The concrete
language bindings must preserve these meanings; finish does not perform a language
return. Finishing before the handler unwinds is valid and requires no further result
from that handler to declare execution complete.

Dispatch must represent zero or more available results separately from whether the
execution has finished. A return before any result is available is an open execution
with no result yet, not a successful terminal result containing null. If a binding
allows a return value to supply data, publish that data once before releasing the
caller without implicitly finishing. A bare return need not fabricate output.

Result data receives the Tool's successful-output validation; structured failures
retain their existing meaning. Custom extension events and best-effort telemetry
remain distinct from Tool result data. Brain does not treat each update as another
terminal response to the original model Tool call.

### 2. Finish is a durable event and an ordering boundary

Use the same canonical journal and sequence order for results and finish. Admit
finish against the invocation's emission path, commit all earlier admitted emissions,
then commit the finish event before acknowledging it. Reject subsequent emissions.
The terminal event has Brain-assigned invocation provenance and is observable even
when it carries no new result data. The Agentloop acknowledges processing finish
through the same sequence marker as other observations.

Preserve one terminal cause. A result already committed is not withdrawn or rewritten
if the remaining execution later fails, times out, is cancelled, or becomes unknown.
Those terminal outcomes are committed and made available to the loop too. Cancellation
and timeout do not promise rollback of external work; send-once behavior remains.

Agentloop completion separately closes its model/dispatch/state authority and
coordinates with already-admitted service calls before finalizing activation state.
Background Tool emissions use execution ownership independent of that old turn cursor.

### 3. Keep complete history and one processed sequence

Always append Tool result and terminal events to the canonical journal/history,
regardless of whether the loop will call a model. The owner clarified that this does
not mean automatically appending them to the stored model transcript. The Agentloop
continues to own model messages, context selection, and transcript changes under
[ADR-018](2026-09-02-01-presentation.md).

Maintain one durable processed-through sequence for the session's Agentloop. The
loop explicitly advances it after processing a contiguous prefix of observations,
including finish. Processing may mean using data, deliberately dismissing it, or
saving enough state to act on it after a later user message. Acknowledging does not
erase history or require a model call. Commit any loop state needed for that decision
before acknowledging the corresponding observations.

The marker only moves forward within committed history. Advance to the sequence the
loop actually processed, not the journal head at activation end: an event arriving
during the activation must not be silently acknowledged. Seeing an event on a live
transport is not a processing acknowledgement. The acknowledgement's own journal
record does not trigger another activation. There is no per-event acknowledgement
table or second durable inbox.

The existing `brain.last_activation` watermark is the starting point to replace or
refine, rather than maintain a competing cursor. Its current automatic advancement
to the activation's initial event page does not express explicit loop processing.
Exact field and service names belong in the Rust contracts and generated bindings.

### 4. Activate from committed observations, one loop at a time

New asynchronous Tool results and terminal events make the loop eligible to run.
Pass the actual event cause and unprocessed history; do not fabricate a user message.
Keep at most one Agentloop activation per session.

When an activation is running, new observations remain available through the event
services. After it yields, schedule pending observations it has not already handled.
Results and finish handled through synchronous dispatch use the same processed marker
and do not require a second activation. Once a loop has acknowledged an observation,
choosing to wait for the user must allow the session to become idle.

Loop-authored events, telemetry, processing acknowledgements, and ordinary journal
bookkeeping do not independently trigger this Tool-result wakeup path. Failure does
not acknowledge unprocessed data or automatically replay the failed activation;
preserve the existing interruption and send-once rules. A later activation can inspect
the retained history and decide what to do.

### 5. Briefly coalesce wakeups after commit

Use a small fixed collection window beginning with the first pending committed
observation. Subsequent results and finish events join the pending wakeup without
resetting its deadline. The implementation recommendation is an initial **5 ms**
window; the exact value is not a measured latency guarantee or a new public setting.

The window delays only automatic loop scheduling. Event commits, emission/finish
acknowledgements, live publication, and synchronous result delivery retain their
normal immediate path. Store pending sequence information and the deadline; obtain
the actual events from the journal instead of keeping another payload buffer.

For example, a result committed at 0 ms and finish at 1 ms normally produce one
activation eligible at 5 ms, containing both. A finish committed after that window
may require another activation. A finite collection window cannot guarantee one
activation for every arbitrarily spaced result/finish pair.

If the loop is busy when the window expires, run pending work when that activation
yields without adding a fresh collection delay. If the active loop has acknowledged
all pending observations, discard the pending wakeup. User-message admission does
not acquire this artificial delay and may consume the same pending observations.
Actual scheduling can occur later because the loop, journal, or runtime is busy;
the fixed window bounds the intentional batching delay, not end-to-end latency.

Do not use a sliding debounce: a continuous event stream must not keep postponing
activation. Do not delay journal commits to obtain batching or wait for finish before
exposing intermediate results. Losing the transient timer cannot lose committed
events; history and the processed marker remain durable. This does not add automatic
replay of unfinished Tool execution or eager session recovery after restart.

### 6. Own background execution beyond normal turn completion

An unfinished Tool may outlive its parent's normal return and actor passivation.
Its original deadline, resource accounting, and emission budget continue across both
phases; return neither renews the budget nor releases capacity still in use. Retain
only the execution state needed by unfinished work, not the completed loop's guest
or mutable turn state. Environment turn-end cleanup must account for unfinished
Tools before releasing resources they need.

`session.interrupt()` also requests cancellation of unfinished Tools belonging to
that session, including when the Agentloop is idle. End/delete close execution
authority and request cancellation before Environment teardown or journal removal.
Client close keeps its existing ownership scope: close the client's transport and
interrupt its locally hosted work without ending unrelated stored sessions.
Preserve bounded cleanup, known versus unknown outcomes, and no automatic replay
after connection or process loss.

Interruption also invalidates a pending wakeup, including one already waiting for
the session lock. A transient generation fences that scheduled activation; it is
not another processing marker. Cancellation observations stay in history for the
next explicit activation without immediately starting more work after interrupt.

The current Wasm profile releases a fresh Store when the exported invocation ends.
An adapter must signal logical return to Brain while retaining and driving that
invocation for background work; a physical return from today's export cannot leave
a runnable guest continuation behind. Finish can close protocol authority before
runtime cleanup completes. Retain resource accounting until that cleanup actually
releases the resources. Do not introduce persistent Store reuse or a new runtime to
make the source syntax identical across Environments.

## Alternatives and tradeoffs

- **Finish automatically on return, with explicit background registration:** shorter
  existing one-shot functions and fewer forgotten finishes, but return changes meaning
  depending on registration. Rejected in favor of explicit finish, accepting open
  executions when authors omit it.
- **Wake immediately for each event:** removes the collection delay but commonly runs
  the loop separately for adjacent result and finish events. A short fixed window
  trades a small intentional delay for fewer activations.
- **Reset the timer on each event or wait for finish:** may collect more events but
  allows indefinite postponement or hides useful intermediate results. Rejected.
- **Automatically append model messages:** conflates durable observations with model
  presentation. The journal retains all observations; the loop selects model context.
- **Per-event acknowledgements or another durable inbox:** duplicate ordering and
  state already represented by the canonical journal and a cumulative sequence.

## Implementation and validation

This is a coordinated protocol change alongside the callback lifetime fix. Update
the Rust Tool/Agentloop/Environment contracts, journal projections, session scheduling,
SDK host and HTTP paths, native WIT/runtime adapters, and Agentloop consumers together.
Generate schemas and bindings through the existing generators. Update user-facing
Tool, session, and Environment documentation alongside the behavior.

The concrete contract uses `ToolReturn { call_id, sequence, events, finished }`,
`tool_result_emitted`, `tool_call_returned`, and terminal `tool_call_ended` records.
The cumulative marker remains `brain.last_activation`, advanced only by `acknowledge`.
Result and extension emissions share their originating activation's byte budget.

WIT packages advance to `brain:agentloop@0.2.0` and `brain:tool@0.2.0` so old
Components fail admission clearly. Native deadline zero denotes unlimited; JSON
wires omit the deadline. Host updates nest their tagged result/returned/finish
payload under `update` and receive HTTP 200 with the committed sequence. The
official loops present later results and finish to the model as observations,
preserve one provider reply per Tool call, and dismiss custom progress.

Deployment records the Tool completion contract version. The first upgrade runs
the read-only compatibility check with `--from-tool-return` after drain/stop.
Closed sessions remain readable. Any retained session that could execute its
immutable old implementations requires explicit migration and blocks cutover;
the checker never deletes or rewrites data.

Verification must cover synchronous finish, return before any result, multiple later
results, finish before handler return, finish racing admitted emissions, and rejection
after terminal closure. It must also cover post-return failure/cancellation, interruption
while the loop is idle, teardown, and closure of old Agentloop state authority.

Use controlled time to verify burst coalescing, a continuous stream that cannot extend
the collection deadline, busy-loop handoff without an extra window, and results consumed
synchronously without redundant activation. Verify a result and finish are separate
ordered committed records that can be acknowledged with one processed sequence. Verify
cursor durability, arrivals during an activation, deliberate deferral without repeated
wakeups, and unchanged model transcript unless the Agentloop edits it. Exercise host,
HTTP, and native execution and run all applicable correctness/integration gates.
The unrelated benchmark review items remain deferred.

## Sources

- Owner's Brain review discussion on 2026-09-21: explicit Tool finish with the risk of
  forgotten completion accepted; separate return/result/completion; asynchronous loop
  wakeup; one processed sequence; session interruption of background Tools; durable
  finish events; a short collection window. The owner explicitly clarified journal
  history versus Agentloop-owned model transcript.
- Reviewed Brain revision: `8d7cfc4dd7ba89f2351cb78a620af25e68f49585`.
- [Current Tool protocol](../../crates/brain-protocol/src/tool.rs),
  [activation input/output](../../crates/brain-protocol/src/agentloop/turn.rs),
  [session actor and watermark](../../crates/brain/src/session/actor.rs),
  [callback grants](../../crates/brain-server/src/executions.rs), and
  [native Tool WIT](../../crates/brain-env/wit/tool/tool.wit).

[Index](README.md)
