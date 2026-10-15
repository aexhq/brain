# Brain predecessor decision review

Date: 2026-10-15  
Status: Review updated with successor design decisions

Keep the successor's local-first Go server, upstream harness adapters, optional external persistence, and bounded background work. The predecessor's history supports several of these choices. Its most useful lessons concern execution ordering, lifecycle ownership, native recovery, and resource accounting. Those lessons fit the existing boundaries and do not require restoring its extension framework.

The largest intentional change is weaker durability. That changes what can be promised about request deduplication and recovery after machine loss; it does not justify ignoring a known local write failure or repeating an uncertain action. Several other findings below are unspecified details in our plans, rather than conflicting decisions or defects in an implementation.

This comparison uses the predecessor's [ADR collection][old-index], later amendments through ADR-057, and relevant aex-research findings. Historical reproductions establish failures in their named versions, not current upstream bugs. Rust/Wasm selection, the Environment framework, external-storage scope, and associated learning/build costs are excluded as requested.

The user's follow-up narrows the recommendations below: familiar sessions/events, harness-owned context and continuation, all agent tools external, approvals implemented by tools, and clean breaks on every release. [ADR 0001](adr/0001-local-first-session-server.md) and the [plans](implementation-plan.md) now reflect those decisions. [Current integration research](harness-integration-notes.md) distinguishes native recovery from prompt caching and documents the available harness controls. The earlier recommendations for core approval gates and upgrade compatibility are withdrawn.

## Decisions compared

| Predecessor decision | Successor position | Recommendation |
| --- | --- | --- |
| **Durably commit before effects and publication.** ADR-032/035 require a local filesystem flush; ADR-009's earlier write-behind acknowledgement was superseded because acknowledged effect records could disappear. | Local acceptance and asynchronous external synchronization permit tail loss. This is an explicit change in guarantee, not evidence that asynchronous storage is inherently wrong. | Keep our weaker guarantee. For Brain-dispatched tools and lifecycle actions, specify local intent-before-dispatch and result-before-exposure ordering without requiring a forced disk sync for each operation. A known failure to record locally stops the affected execution path. External upload failure follows bounded backlog/admission policy. [ADRs 009, 032, 035][old-032] |
| **One journal owns session execution facts; other views are derived.** ADR-032 replaced separate canonical transcript/state and public-event logs. | SQLite owns configuration/access/discovery; the event log owns accepted inputs and observed history; the harness owns native conversation state. | Keep these responsibilities explicit without adding a second effect journal or public execution taxonomy. A normalized transcript cannot replace native state. Approval state belongs to tools. [ADR-032][old-032], [ADR-053][old-053] |
| **Never automatically retry an effect.** ADR-035 covers models, tools, and resource operations, even when a failure says retryable. | Brain does not replay tool invocations or add model retries. It retries bounded external-storage operations. | Keep tool retry policy in its external service with any needed idempotency guarantees. Harness model retries remain native behavior. A timeout or retryable flag does not prove an action never ran. [ADR-035][old-035] |
| **Every upstream model/tool call passes through Brain's effect barriers.** ADR-053 disables direct model transports, ambient tools and upstream transport retries. | Our harness adapters retain upstream model execution; the plans do not introduce a Brain-owned model service or universal model proxy. | State which operations Brain mediates and which it observes. An event received after a model call starts cannot enforce a pre-dispatch barrier. Required tool restrictions still need interception or disabling of bypassing tools. Add model mediation only for a concrete requirement; do not inherit guarantees that depend on it. [ADR-053][old-053] |
| **Record identity is only session ID plus sequence.** ADR-040 removes a separate event ID as redundant under its storage model. | Our API permits restore to an older checkpoint and uses opaque event IDs. | Keep IDs unique after lost-tail recovery so a new event cannot impersonate a lost one. No public generation/incarnation entity is needed. [ADR-040][old-040] |
| **Approvals belong to application tools.** ADR-045 excludes a built-in approval facility. | Aligned after the user's correction: approvals are ordinary external tools. | No Brain approval gate, store, event family, or decision endpoint. The action service enforces its business authorization and approval rules. Waiting/returning a handle follow ordinary tool semantics. [ADR-045][old-045], [application integration ADR][app-integration] |
| **General session APIs use deployment authority; application tenancy is enforced upstream.** The predecessor's product review distinguishes its deployment bearer from scoped execution callbacks. | Our API maps credentials to tenant grants and enforces scope inside Brain. | Keep this stronger boundary. The host still establishes identity and placement, while Brain checks every session, event stream, artifact and inspection operation. An administrative key must remain distinct from an end-user/session grant. [Product review][product-review] |
| **Prelaunch contracts and stored data may be replaced without migration.** ADR-030 takes clean breaks. | The successor applies clean breaks by default on every release, including after launch. | No API/schema versioning, backward-compatibility layer, deprecated aliases, or migration framework unless explicitly requested. Keep reproducible dependency pins and fail clearly on unreadable state. [ADR-030][old-030], [ADR-056][old-056] |
| **The default distribution bundles and initializes Pi, Codex and OpenCode.** ADR-057 supersedes ADR-056's separate base/extension distribution. | Our local preview includes Pi; other harness packages remain optional. | Keep the smaller initial distribution. A later convenience bundle can use the same adapters without making every upstream runtime a prerequisite for local startup. This is a packaging difference, not an execution-model conflict. [ADR-057][old-057] |

Two important apparent conflicts disappear when the amendments are followed. ADR-053's startup-wide harness selection was replaced by **per-session selection** in ADR-056, which agrees with our design. ADR-037's earlier turn-boundary release policy was amended for upstream native state and unfinished tools; it should not be read as permission to destroy a live harness continuation. Our capability-dependent unloading is appropriate, but its memory policy still needs definition.

## Tool completion needs a deliberate choice

ADR-048 chose explicit finish: returning from a tool releases its caller but does not necessarily finish execution. The September 24 experiments reproduced a surprising consequence: an ordinary lookup returned an object, remained open, timed out later, and caused further model activity. The September 24 application-integration proposal mapped ordinary HTTP handler return to completion. This is an authoring choice with demonstrated cost, not a universal requirement for asynchronous tools. [ADR-048][old-048], [interface experiments][interface-research], [application integration ADR][app-integration]

Use an ordinary HTTP result to complete its invocation. A tool may wait for its result or return a job reference and offer status/cancel tools. A job receipt completes the initial invocation, not the external job. Async code or SSE progress alone does not let the model continue while its harness awaits a final tool result.

There is no universal emit/return/finish contract. Internal storage retries belong to Brain; domain jobs belong to external services. The harness controls whether input/result delivery starts, queues, or steers model work. A late result can use a supported input path; Brain does not build its own agent scheduler.

## Lessons to incorporate

### 1 Serialize state transitions without blocking progress

The predecessor found a process-wide lock held across activation, a worker that serialized accepted work again, and a shared permit pool in which parent turns consumed the capacity their tools needed. Merely replacing rejection with queueing could turn tool overload into deadlock. A separate review found recovery work under the global session-table lock. [ADR-010][old-010], [ADR-037][old-037], [performance verdicts][perf-verdicts], [September 5 review][review-sep05]

Our ordered control path serializes **state transitions**, while model/tool waits run outside it. Results, interruption, and inspection remain serviceable during a wait; this does not promise concurrent model inference. Keep admission separate from capacity needed to route results. Do not hold a global registry lock during I/O/history loading. These are implementation constraints, not new public lifecycle concepts.

Add R0 tests with every turn slot occupied and every turn waiting on a tool, and with one slow session alongside an unrelated fast session. These are execution-correctness tests, not a deferred fleet-scale benchmark. Owners: L3/L5, H1, O2, V3.

### 2 Define request acceptance and turn outcomes separately

A reproduced busy rejection left an unresolved request claim, so retrying the same key later failed as ambiguous even though the rejected turn never ran. Another probe showed a failed model turn resolving the SDK send call with an idle session summary. Our API promises idempotency and typed errors but does not yet specify these cases. [Interface experiments F1/F2][interface-research]

An overload rejection must not consume a request key. For accepted input, retain its event identity and report completion/failure separately. Same-key retries reuse the accepted input while retained state exists. Expiry or cache loss is not evidence that an earlier action never ran. Implement this in the existing event/request handling, without a public claim entity.

Return an identity for accepted work and expose its exact terminal outcome through observation and SDK helpers. Session idle, turn success, tool success, and application success are different facts. A session can be idle after a failed turn, and a successful turn can report that a business action failed. Owners: A1–A3, L2/L3, V2/V3.

### 3 Verify restored execution context against the next request

Our separation of native state, normalized transcript and workspace is already correct. The predecessor supplied concrete evidence for it: an OpenCode export/import preserved message structure but changed the next model request because the restored project path changed. Pi's restoration experiments also initially used a broken reference that reconstructed zero messages. Matching two serialization outputs is not sufficient recovery evidence. [Native-state research][native-research], [ADR-045][old-045], [ADR-053][old-053]

Extend H2–H4 and P5 tests to inspect the next constructed model request after restore: active history, compaction, continuation data, call/result identity, media, and path-dependent prompt content. Use a recording test provider; no per-turn production prompt hash is needed. Distinguish correct continuation from best-effort cache reuse: a provider cache miss increases input processing cost but does not require replaying past execution. Declare unavoidable prompt differences.

ADR-053 additionally guarded against upstream automatic recovery replaying dispatched work and against upstream catching a fatal Brain callback failure. Retain our completed-saved-turn baseline and test these cases explicitly. A harness catching an error must not bypass Brain's interruption, policy or local-recording failure. Do not advertise partial-turn resume merely because an upstream open-session call succeeds. Owners: H1–H5, P5, V2/V3.

### 4 Measure growth across turns and checkpoints

The predecessor measured quadratic journal growth from saving the entire growing context after every turn. Moving full-context persistence from every decision to every turn reduced a constant without removing the problem. Append-count deltas also lost rewritten or compacted context; ADR-019 introduced replacement-tail semantics. [ADR-010][old-010], [ADR-019][old-019], [performance verdicts][perf-verdicts]

Our immutable transcript segments already address ordinary append growth. Native checkpoint uploads are a separate risk: copying an ever-growing native directory at every turn can reproduce the same cumulative write/upload amplification. Measure total local bytes, uploaded bytes, checkpoint time and restored context across many turns, including compaction and branches. Bound checkpoint frequency and retention; reuse unchanged artifacts where the adapter can do so correctly.

Do not force every harness into the predecessor's delta format. Preserve its native semantics and make the actual cost visible. SQLite remains reasonable for metadata: the [earlier SQLite rejection][old-009] involved full-history scans and shared-connection serialization on the transcript path, not proof that SQLite cannot serve session metadata. Owners: L2, P1–P3, H5, V4.

Retain our strict distinction between a torn final record and interior corruption. The September 21 review reproduced a damaged length header causing recovery to discard later intact records as a supposed incomplete tail. Include that case in local-format recovery tests; accepted tail loss is not authorization to silently repair arbitrary corruption. [September 21 review F2][review-sep21]

### 5 Set a residency policy and prove any worker reuse

ADR-037 requires history reads without activation and lazy session loading. The native research found substantial costs for a process or worker thread per session, while a tested recycled Pi RPC process retained prior-session context/accounting. Its shared-session experiments used a different integration and do not prove arbitrary cross-tenant process reuse safe. These results apply to the recorded versions and workloads. [ADR-037][old-037], [native-state research][native-research]

Our Pi runner topology is unspecified, and the explicit reuse/isolation condition currently appears most clearly under OpenCode. Apply it to every harness. Select runner capacity and trust scope using both memory measurements and isolation tests. Do not default to one process per retained conversation or assume reset/new-session clears process-global state.

List/history/inspection reads should not start a harness. Bound retained native contexts, metadata caches, request claims and open files; load sessions on demand. Measure cold readiness against many stored sessions and active memory against long histories. Conditional idle unloading is sufficient initially; no new cache tier is required. Owners: L2/L5, H2/H5, O2/O4.

### 6 Authenticate event provenance and close execution authority correctly

Two distinct predecessor failures matter. Extension-controlled event names entered private recovery paths, and a tool could emit a checkpoint-shaped custom event indistinguishable from an Agentloop's event. Later, callbacks admitted before grant revocation could mutate state after turn completion. [ADR-045][old-045], [September 5 review F1][review-sep05], [September 21 review F3][review-sep21]

Brain authenticates its API/tool boundaries and supplies event authority from that path. A tool supplies result data; it cannot promote it to privileged system context or internal control by choosing a type/origin string. The harness owns conversation mutation after accepting content. Typed request/result boundaries and call correlation are sufficient; no public provenance hierarchy is needed.

A result must resolve only its authorized call and cannot complete newer work after rewind/restore. Duplicate/stale completion is rejected. A genuine later notification may enter as new input through a supported harness path; that is different from completing a closed call. Domain-job lifetime remains with the tool service. Owners: T1/T2, H1, L3, V3.

### 7 Bound actual allocations and in-flight work

The existing plans already require count/byte bounds, slow-consumer handling and admission control. The predecessor shows where those checks can be ineffective: checking a body after buffering it, checking SSE size only for incomplete frames, allocating a huge vector from a tiny sparse index, and counting queue bytes only after producers have already allocated payloads. [September 5 review F3 and capacity section][review-sep05], [September 21 review F5/F12][review-sep21]

Enforce bounds while reading/decoding, before size-driven allocation, and across queued plus admitted in-flight work. Budget subprocess output, large results, subscriber buffers and native contexts as well as the Go queue. Use session-scoped event fan-out so one session's traffic does not force every unrelated subscriber to receive and discard it.

[ADR-043][old-043]'s configurable budgets remain useful, but its premise that content-byte budgets made content-block count caps unnecessary was disproved by the sparse-index case. Keep each bound tied to actual allocation or work. Keep finite queues nonzero, as that ADR already requires, and define any unbounded timeout explicitly. Owners: T1, H1, A2, O2, V3.

### 8 Separate deadlines from worker health and remote certainty

The predecessor killed a healthy shared worker because a valid asynchronous wait produced no protocol traffic within a timer derived from one HTTP timeout. Other reproductions ran an expired tool before a zero-delay cancellation timer fired, and silently clamped a server deadline in the SDK. [September 21 review F4/F7/F9][review-sep21]

Define deadline units and propagation once, preserve the remaining budget, and reject expired work before invoking its handler. A quiet stream does not establish worker failure. Any health mechanism must work during supported model/tool/approval waits and must not kill unrelated sessions merely because one invocation is silent.

Retain the local stop reason separately from knowledge of remote completion: timeout/cancellation can be known while the external effect remains uncertain. Preserve a first terminal outcome and all known parallel results when another call times out. Owners: H1, T1/T5, A3, O1/O2, V3.

### 9 Retain only Brain's own lifecycle responsibilities

The earlier runtime recovered interrupted turns but mishandled incomplete creation and ending. Failed teardown could discard the only resource handle; deletion could leave the writer appending to an unlinked old file. Our plans cover turn recovery and generic cleanup but do not yet describe these boundaries fully. [ADR-036][old-036], [September 5 review F7/F8][review-sep05], [September 21 review F1][review-sep21]

For Brain-owned runners, files, and connections, stop new work before close and account for partial creation/cleanup failures. Close file handles before removing cached data. Releasing a tool connection does not destroy the caller's remote resource.

External compute expiry, sandbox snapshots, browser state, and eventual job cleanup belong to the tool services/platform. Those predecessor concerns no longer justify a Brain Environment interface or recovery controller. Owners: L2/L5, O5, V3 for Brain resources; T5 for external-service examples.

### 10 Keep ordinary tool results ordinary and preserve their meaning

ADR-047 allowed ordinary values or special outcomes distinguished by top-level status. A later real application probe returned business data with status cancelled and received a protocol error. Other findings involved schemas generated from post-default output instead of accepted input, unsupported async validation, changed schemas on reattachment, non-JSON results failing after invocation tracking was removed, and images remaining JSON instead of model-visible media. [ADR-047][old-047], [SDK lifecycle ADR][sdk-lifecycle], [interface experiments][interface-research], [September 21 review F8][review-sep21]

Choose an explicit protocol envelope that keeps business data separate from execution status. A natural SDK return value becomes the data inside that envelope; it is not inspected for reserved business-field values. Apply successful-output schemas only to successful data and validate final serialized output.

Restore recorded tool definitions/bindings and validate incoming values at the tool boundary. No per-call contract hashing or compatibility negotiation is needed. Preserve input-default semantics, useful structured output, and model-visible media. A blob or image-shaped JSON object alone is not modality support. Keep this in T1/T7 and A3, without another authoring framework.

## Existing choices to retain

The following predecessor principles already agree with our plans:

- **One conversation owner.** Harnesses choose context, compaction and presentation; tools supply results without editing shared conversation state. Fixed authority is distinct from what schemas/context the model currently sees. [ADR-018][old-018], [ADR-050][old-050]
- **Restore data without replaying effects.** Opening history does not run the agent. Workspace recovery and native-context recovery remain separate from transcript retrieval. [ADR-026][old-026], [ADR-053][old-053]
- **Bounded, best-effort telemetry.** Telemetry failure must not stall sessions. Canonical session history and operational logs have different purposes. Preserve absent usage counters as absent, rather than treating missing as zero. [ADR-015][old-015], [ADR-012][old-012]
- **Independent client and execution lifetimes.** An observer disconnect is not session cancellation. A browser/tool host disappearing is loss of that execution capability, not authorization to redirect the tool. [SDK lifecycle ADR][sdk-lifecycle], [ADR-052][old-052]
- **Code-owned contracts and real release checks.** Generate derived API/client views and exercise actual serialized responses, packaged consumers and native harnesses. Generated types alone did not catch all old wire mismatches. [ADR-031][old-031], [September 21 review F10/F11][review-sep21]
- **Measured capacity.** Count successful completed work and actual provider/tool activity, with rejections and failures reported separately. Include harness processes, retained histories, session churn, cold readiness and restore cost. Historical enqueue or empty-server numbers are not current capacity claims. [ADR-039][old-039], [performance verdicts][perf-verdicts]

## Implementation follow-through

| Delivery point | Concrete additions |
| --- | --- |
| **Before implementing L2/L3 and A1** | Use the revised ADR and small session/event contract: scoped input, unique event/call identity, local acceptance versus actual outcome, and separate tool/fatal errors. No extra approval, Environment, or public execution framework. |
| **R0 correctness and capacity baseline** | Add nested-capacity, unrelated-session progress, busy-rejection retry, failed-turn outcome, event provenance, late callback, bounded decoding, deadline and partial-lifecycle cases. Prove the Pi runner's chosen sharing boundary and lazy history reads. |
| **R1 persistence gate** | Test restored next-request context, required control metadata, cursor identity after rollback, native checkpoint amplification, and restore without stale cache authority. Include reattachment limitations for external tools/workspaces. |
| **R2 and R3 integration gates** | Repeat native recovery, fatal-stop, input timing, and isolation checks. Keep HTTP completion simple; long-job handles and provider cleanup belong to external tools. |
| **R4 operating evidence** | Keep fleet-scale, soak, and same-build disaster recovery. No upgrade compatibility gate. Earlier releases already need the deterministic correctness checks above. |

The recommended change is greater precision in the existing design and tests. None of these findings requires a broker, consensus service, durable workflow engine, universal resource tree, or another extension taxonomy.

[old-index]: ../../../aex_workspace/brain/references/adrs/README.md
[old-009]: ../../../aex_workspace/brain/references/adrs/2026-08-28-01-write-behind.md
[old-010]: ../../../aex_workspace/brain/references/adrs/2026-08-28-02-growth.md
[old-012]: ../../../aex_workspace/brain/references/adrs/2026-08-30-01-provider-model.md
[old-015]: ../../../aex_workspace/brain/references/adrs/2026-08-31-01-telemetry.md
[old-018]: ../../../aex_workspace/brain/references/adrs/2026-09-02-01-presentation.md
[old-019]: ../../../aex_workspace/brain/references/adrs/2026-09-02-02-prefix-deltas.md
[old-026]: ../../../aex_workspace/brain/references/adrs/2026-09-04-04-load-recovery.md
[old-030]: ../../../aex_workspace/brain/references/adrs/2026-09-04-08-clean-break.md
[old-031]: ../../../aex_workspace/brain/references/adrs/2026-09-04-09-rust-contracts.md
[old-032]: ../../../aex_workspace/brain/references/adrs/2026-09-05-01-canonical-journal.md
[old-035]: ../../../aex_workspace/brain/references/adrs/2026-09-05-04-send-once.md
[old-036]: ../../../aex_workspace/brain/references/adrs/2026-09-05-05-interruption.md
[old-037]: ../../../aex_workspace/brain/references/adrs/2026-09-05-06-ephemeral.md
[old-039]: ../../../aex_workspace/brain/references/adrs/2026-09-05-08-benchmark-policy.md
[old-040]: ../../../aex_workspace/brain/references/adrs/2026-09-05-09-session-names.md
[old-043]: ../../../aex_workspace/brain/references/adrs/2026-09-06-02-deployment-limits.md
[old-045]: ../../../aex_workspace/brain/references/adrs/2026-09-07-01-protocol-freeze.md
[old-047]: ../../../aex_workspace/brain/references/adrs/2026-09-12-01-tool-outcomes.md
[old-048]: ../../../aex_workspace/brain/references/adrs/2026-09-21-01-tool-completion-and-event-activation.md
[old-050]: ../../../aex_workspace/brain/references/adrs/2026-09-23-02-tool-context-and-model-boundary.md
[old-052]: ../../../aex_workspace/brain/references/adrs/2026-09-28-01-client-connection-lifecycle.md
[old-053]: ../../../aex_workspace/brain/references/adrs/2026-10-14-01-startup-harness.md
[old-056]: ../../../aex_workspace/brain/references/adrs/2026-10-14-02-registered-agentloops.md
[old-057]: ../../../aex_workspace/brain/references/adrs/2026-10-14-03-default-agentloops.md
[sdk-lifecycle]: ../../../aex_workspace/aex-research/docs/adr-2026-09-12-sdk-lifecycle-and-tool-execution.md
[app-integration]: ../../../aex_workspace/aex-research/docs/adr-2026-09-24-app-integration-and-tool-status.md
[interface-research]: ../../../aex_workspace/aex-research/docs/brain-interface-research-2026-09-24.md
[native-research]: ../../../aex_workspace/aex-research/docs/brain-findings.md
[perf-verdicts]: ../../../aex_workspace/aex-research/docs/brain-performance-verdicts-opus-2026-08-28.md
[review-sep05]: ../../../aex_workspace/aex-research/docs/brain-architecture-review-2026-09-05/README.md
[review-sep21]: ../../../aex_workspace/aex-research/results/2026-09-21-brain-review/report.md
[product-review]: ../../../aex_workspace/aex-research/docs/brain-product-review-2026-09-27.md
