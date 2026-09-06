# Implementation plan: ADR-044

Prepared 2026-09-06 against Brain `8ff1238` and the locally agreed ADR-044. This is an
implementation plan, not an implementation or release verification result.

## Scope and assessment

Implement [ADR-044](../adrs/2026-09-06-03-sessions-and-brain-env.md): extract brain-sessions,
normalize loophost into brain-env, operate multiple worker processes, remove Agentloop
semantics from the general Environment execution protocol, and support fixed or explicitly
selected authorized Tool placements with independent model presentation.

This is a substantial contract change around a comparatively small crate extraction. The
largest semantic changes are placement/presentation and execution callbacks. Pooling is
bounded local scheduling work; it does not require a distributed scheduler or retained guest
instances. Fresh Wasm Stores and separate parent/Tool permits already exist and should be reused.

The deployment-limits implementation has landed in this baseline. Preserve its injected
limits, shared worker argument definitions, zero-as-unbounded ceiling semantics, and generated
configuration reference. Do not implement the plan against the earlier constant-based code.

This is a clean prelaunch cut. Replace old contracts and consumers together; delete obsolete
exports, fields, routes, configuration names, fixtures, and documentation. Do not add aliases,
dual-schema readers, protocol translators for old clients, deprecation periods, or data
migrations. Retained typed guest ABIs are supported runtime interfaces, not compatibility shims.
Keep the ADR-030 data-layout marker; owners explicitly reset incompatible development data.
Implementation and verification are in scope; publishing and deployment require the existing
release process and are not actions authorized by this planning task.

## Implementation choices

### Execution

Use one Environment execution request with an opaque implementation descriptor, input,
needs, deadline, and invocation-scoped services. The descriptor owns runtime-specific
entrypoint and configuration; do not duplicate them in a second generic selector.
Use one generic terminal output
or failure. Keep lifecycle operations separate. brain-sessions/brain interpret Agentloop
turn input/output and Tool outcomes; the Environment protocol does not contain TurnInput,
TurnOutput, or a Turned receipt.

Keep the existing typed guest WIT worlds initially. The Wasmtime runtime's bindings adapt
generic execution to the admitted Component's export; knowing an ABI is necessary to run it.
That does not require exposing its semantics on the general Environment wire. Do not build
arbitrary WIT reflection, a new universal guest ABI, or a new language compiler for this change.
Runtime-specific entrypoint/profile metadata belongs in the Environment's implementation
descriptor and is sealed with the placement, not selected freely in a model response.

Generalize the existing service bridge rather than introduce a broker: callbacks are scoped
to an execution, with only the service methods its caller grants. The session side maps those
calls to TurnServices or ToolServices. The Environment transports them without interpreting
transcripts or model requests. Native IPC and HTTP use the same capability semantics; the
browser host may continue supporting only its registered function execution profile.

### Placement and model presentation

Represent each Tool with a stable definition and a nonempty collection of allowed placements,
keyed by the existing Environment name. Each fixes one implementation descriptor and needs.
The pair (Tool name, Environment name) identifies a placement; do not introduce another
placement name or handler identifier. Reject duplicate pairs at create. A dispatch
contains the Tool name, explicit Environment name, call correlation, and input. There is no
implicit ranking or fallback. The SDK/reference loop may fill in the sole placement before
dispatch. Every selected placement is validated against the create-time configuration.

The Agentloop remains fixed to one authorized placement. Generalize its implementation
descriptor too: current SDK creation always admits a Wasm Agentloop to Brain even for an HTTP
Environment. Remote Agentloops must be allowed their Environment's descriptor without being
forced through brain-env admission.

ModelRequest supplies ToolDefinition values, not only names to resolve from session config.
The Agentloop maps model-facing names/arguments to authorized dispatches. Omission can preserve
the existing default presentation; an empty list still means no Tools. Validate presentation
shape and unique names, but never use presentation as the authority for execution. Validate
actual dispatched inputs and successful outputs against the canonical Tool schema. The SDK
host already validates its handlers, but the common Rust dispatch path currently does not
perform that schema validation. Reuse the workspace's jsonschema dependency, without remote
schema fetching, rather than introduce a second validator implementation.

Provide the loop with the allowed Tool/Environment pairs and useful logical Environment descriptions
through its activation context. Do not expose credentials, process paths, or opaque executable
descriptors in a default model prompt. Journal the actual model-facing schemas and the selected
execution placement so model presentation and execution can be understood independently.

### Lifecycle

Keep caller milestones: setup during create, detach at end, teardown at delete. Move their
orchestration into brain-sessions. The Environment does not expire itself. Fresh invocation
state ends with execution; workspace resources survive until explicit cleanup. Scope native
workspace paths by session id and Environment name.

Do not add a model-facing lifecycle Tool or repeated setup/recreation protocol in this batch.
The design permits such Tools to call authorized caller operations later. They must use an
in-turn journal/service path, not recursively invoke the session's public HTTP methods.
Choosing a preauthorized execution placement is included now; new Environment provisioning,
new grants, and mutable placement definitions are not.

## File ownership after extraction

| Current code | Destination and treatment |
| --- | --- |
| brain-server/service.rs session table, stores, session/store locks, create/read/message/end/delete, passivation and recovery | brain-sessions service; retain one mutable owner/store per session and on-demand recovery; leave API request claims in server |
| brain-server/environment/registry.rs journaled setup/call/detach/teardown | brain-sessions lifecycle coordination; remove concrete adapter fields |
| brain-server/tool_dispatcher.rs and EnvironmentLoopExecutor | brain-sessions adapters connecting single-session execution ports to generic Environment execution |
| brain-server/environment/adapter.rs | Common execution port in brain, wire types in brain-protocol |
| brain-server/environment/brain.rs plus brain-loophost | brain-env, including policy, grants, admission, execution, IPC, supervision, and workspace operations |
| brain-server/environment/{host,http}.rs | Remain concrete server adapters; inject routing into brain-sessions |
| brain-server/turns.rs | Keep transport callback registration in server; generalize it to invocation-scoped service registrations |
| brain-server/model.rs CredentialStore and metadata.rs ModelCredential | Remain in server with encryption, credential storage, model transports, and provider loading; sessions receives credential-free config and injected executors |
| brain-server/idempotency.rs, digest.rs, persistence.rs | Remain shared server infrastructure for request claims, metadata, hosts, and artifacts; do not make brain-sessions their owner |
| brain-server/service.rs artifact admission, host connection endpoints, health, BrainApi implementation | Remain in a composing ServerApi facade that delegates to sessions/env/adapters |
| brain-server/main.rs, config.rs, data_layout.rs | Remain composition, flag/env parsing, data-directory ownership, HTTP startup, and deployment shutdown |

brain-sessions depends on brain/protocol/telemetry, not brain-server, brain-env, or brain-http.
brain-env can use the common execution ports in brain but does not depend on brain-sessions or
brain-server. The server is the composition root. Reuse existing request-claim and credential
ordering; do not introduce an all-purpose storage framework during extraction.

The server validates deployment-specific request details, acquires the existing request-key
lock, checks/claims the request, assigns the session id, and seals credentials before calling
session creation with that id, credential-free SessionConfig, and initial transcript. Sessions
owns genesis, setup, and session/store locking. The server completes the saved API response
and applies existing credential cleanup after failed creation or deletion. Preserve the
current ordering and failure behavior, including uncertain outcomes. Keep raw credentials
out of session records and preserve credential access during Environment cleanup. Reuse
existing helpers; do not expose session lock guards or create a framework of per-effect hooks.

## Ordered implementation

### 0. Resolve the two implementation feasibility risks early

Before a broad rewrite, run the smallest Python Component admission/execution probe against
the existing WIT/import restrictions, including an Agentloop host-service call. Also trace
one official Tool through the MicroVM driver to its guest runner using the proposed final
execution fields. These establish whether packaging and remote descriptor resolution fit
existing machinery. Keep experimental findings in aex-research; do not ship prototype adapters.
If Python requires a new compiler/runtime integration, report that as a separate scope decision
instead of quietly adding a language platform or claiming the scenario is supported.

### 1. Extract ownership without changing execution behavior

Move the code above, initially keeping the current execution shapes. Change registry
construction to injected adapters. Keep BrainApi and HostConnection dependencies outside
brain-sessions. Move session tests with the service and add the new crate to every explicit
CI crate list. Wire the current concrete Environments through the new composition.

Pass the existing session, idempotency, custody, cold-read, and recovery tests before proceeding.
This stage makes later changes local and catches accidental duplicate stores or eager startup
loading independently of the wire rewrite.

### 2. Normalize Environment execution and callbacks

Change the authoritative types in brain-protocol/environment/wire.rs and the common Rust
port. Remove Environment Turn/Invoke distinctions, translate generic execution to typed
guest calls inside brain-env, and keep turn journal semantics in brain. Generalize
brain-server/turns.rs, the HTTP callback routes/service facade, worker wire/client/service,
and native Tool/Agentloop bridges together. Adapt the host driver to the supported function
profile, returning unsupported for other profiles.

Preserve a complete awaited execution: return its terminal result, not merely acceptance.
Caller deadlines govern all executions; remove the HTTP adapter's special long-turn branch
without falling back to its short ordinary HTTP timeout. Revoke callbacks on completion,
cancellation, connection loss, and failure. Tool execution must not gain model or dispatch
services because the transport became generic. Callback credentials never enter the journal.

Update HTTP Environment examples, schema examples, conformance tests, and generated contracts
in this stage. Keep guest WIT signatures unless an actual implementation requirement forces
a change; update moved paths and JSON payload consumers regardless.

### 3. Implement the actual worker pool

Replace the single socket/child state in supervisor.rs with individually supervised worker
slots. Use a configured positive worker count, with a default of two and support for one;
zero is invalid because this is an allocation count, not an unbounded ceiling. Each slot has
its own socket, process identity, admission cache, and capacity accounting. Prefer a simple
rotating scan of workers with available capacity, with immediate overload if none can accept.
Do not hold pool-wide or per-worker lifecycle locks while running guest work or its callbacks.

Preserve separate capacity for executions that can dispatch nested work and for leaf work,
derived from caller-granted services rather than an Agentloop operation tag. Keep the current
one-level nesting contract: Tool services do not gain recursive dispatch. Supervisor admission
and worker limits must agree so accepted requests do not wait indefinitely inside a worker.

Retain one durable artifact store; load/prelink an admitted artifact into a selected worker
on demand. Do not eagerly compile every artifact in every worker at server startup. Keep
the existing admission serialization (the fixed temporary filename depends on it); no new
per-artifact lock registry is needed. A stale failure must not kill a replacement process: associate execution errors
with the worker incarnation that ran them. Restart only the failed incarnation, never replay
its uncertain work, and keep healthy slots available.

Start the configured workers before initial readiness. After a worker failure, readiness can
remain true while a usable worker remains; surface degraded capacity operationally and restore
the failed slot. Full capacity and instantaneous free slots are distinct from readiness.
Shutdown closes admission and reaps every owned child; retain clean worker environments,
restricted sockets, and process-exit cleanup.

### 4. Separate Tool definitions, placements, and model presentation

Change brain-protocol/tool.rs, client/session.rs, model/call.rs, activation context, and their
SDK equivalents together. Update session validation, needs_for, dispatch, cancellation,
model-call recording, and interrupted-operation handling. Each setup receives the union of
needs from every placement authorized in that Environment; each execution receives only its
selected placement's needs. A setup refusal still fails create, even for an alternative the
model might never select. That is the cost of preparing all allowed placements up front.

Update SDK extensions.ts/types.ts/client.ts: compile all placements, admit only descriptors
destined for brain-env, preserve supplied remote descriptors, and register browser handlers
by (Tool name, Environment name) within the session. Update host.ts/client-pump.ts and
HostCommand to carry the Environment name, distinguishing placements of the same Tool.
Update reopen validation, configuration/options routing, cancel, and emitted Event association.

Model transports already accept ToolDefinition slices; reuse their serialization. Remove
only the session-side name-only presentation resolution. Update the reference Agentloop to
map its default one-placement Tools explicitly and add a loop fixture demonstrating a
model-visible choice between two authorized placements. Neither path bypasses canonical
dispatch validation or journal-before-effect.

### 5. Finish lifecycle/resource separation

Change native workspaces to session/Environment paths and ensure workers executing the same
logical Environment see the same files. Calls in separate named Environments must not share
those paths. Keep scratch invocation-local. Retain per-invocation grants when multiple Tools
with different needs share a logical Environment.

Remove Environment-owned expiry from lazy-environment.mjs and its examples/tests; put any
demonstration expiry decision in its caller. Do not confuse the session actor idle TTL or
unconnected host-registration retention with logical Environment expiry. For host bindings,
stop using a session-wide release when detaching one named Environment: track and release
the relevant binding without disconnecting sibling Environments/other sessions.

Do not silently re-run setup while reopening a session. Report resource unavailability through
the existing result/Event machinery. Do not interpret surviving files as restored guest heaps
or promise that an external provider cannot independently lose resources.

### 6. Complete consumers, documentation, and release gates

Consumer work starts alongside stages 2/4, against local builds of the final contract. Do not
wait until the core implementation is complete to discover remote runtime incompatibilities.
The existing extensions checkout is aex-extension (aexhq/extensions); its historical worktrees
are not additional products to upgrade.

| Repository | Required implementation and documentation changes |
| --- | --- |
| brain | Cargo workspace/lockfile, worker binary/socket/config names, Dockerfile, fixture builds, package-smoke, SDK generator WIT sources, release harnesses, and explicit CI crate/job lists. Change authoritative Rust types/route annotations, then npm run gen. Update README, CONTRIBUTING, AGENTS paths, embedding, Environment/Agentloop/Tool authoring, placement, lifecycle, configuration, and runnable examples with the behavior. |
| aex-extension | Upgrade both Agentloops, official Tools, and AWS MicroVM Environment to the final SDK/protocol; rebuild shipped Components, update root/package/runtime READMEs, declarations, examples, tests, lockfiles, and CI Brain checkout pins. Details below. |
| aex | Replace obsolete Brain reexports in packages/sdk/src/index.ts with final factories/types, including brainEnv/hostEnv where exposed. Update control-plane typed consumers, SDK/CLI callers, root/package READMEs, docs/quickstart.md, examples/quickstart, smokes, exact npm dependencies and Rust revisions. Fix tools/verify-brain-overlay.mjs to consume the authoritative brain-http generated OpenAPI path; retain its no-duplicate-session-contract check. |
| site | Update docs.lock.json to the final immutable Brain revision. Reuse scripts/sync-docs.mjs and gen-api.mjs; never hand-edit content/docs or content/contract. Update app/brain/page.tsx prose, architecture table, roadmap claims, and examples, plus app/dashboard/DashboardClient.tsx SDK example and affected shared copy. Remove resident bypass, brainWasm, and Environment-owned expiry claims. Check README build guidance against the existing import pipeline. |
| platform | Reconcile docs/core.md rules 3/5/6/10 and affected definitions with authorized model placement, ordinary host Environments, descriptor preparation, and caller-owned lifecycle. Update terraform/modules/brain-service/main.tf worker executable, config and tracing names, exact images, and customer-canary assertions/examples. Remove obsolete BRAIN_LOOP_WORKER and obsolete Environment configuration entries against the final generated server configuration; no alias variables. Refresh affected operations/release documentation and exact consumer pins using docs/release.md. |

The MicroVM extension is a material protocol rewrite. Its runtime lives under
packages/env-aws-microvm/runtime. environment-driver/src/lib.rs currently defines its own
EnvironmentCommand/Operation/Request/Binding, including attach/provisions, attachment identity,
and directory generation. Replace the public Brain boundary with final brain-protocol types;
delete obsolete public operations and receipts rather than translating old clients. Bind
resources to the final session/Environment identity, implement setup/execute/cancel/detach/
teardown, and resolve the sealed implementation descriptor during execution. Trace private
driver-to-guest fields before deleting them: an internal runtime protocol is not a duplicate
Brain API. Preserve filesystem, egress, secret, deadline, authentication, and uncertain-outcome
enforcement. Adapt both relay and AWS paths, not only mocked SDK factories.

The generic contract permits both Agentloop and Tool execution; it does not magically add
every language/ABI to each provider. Preserve the MicroVM driver's supported implementation
profiles and reject unsupported descriptors explicitly. brain-env must execute both profiles;
prove remote Agentloop callbacks with the HTTP loop fixture. Do not add a second Wasmtime
runtime to the MicroVM driver merely to make all implementations look identical.

Replace env-aws-microvm/src/index.ts's old driver/options factory with the final named
Environment authoring API and endpoint/credential binding. Move optional idle/maximum lifetime
decisions to the caller; do not remove command deadlines, authorization expiry, or unavoidable
provider resource ceilings. Report provider resource loss honestly. Reuse existing caller
infrastructure for lifecycle policy; do not add a second orchestration service for this change.

Update loop-pi and loop-codex component.mjs JSON adapters, logic.mjs model/dispatch handling,
factory declarations, and tests for final Tool schemas and Tool/Environment selection. Preserve
their existing loop behavior, including parallel versus sequential dispatch and compaction.
Use tools/build-agentloop.mjs, the installed componentize-js/esbuild toolchain, and packaged
Brain WIT. Keeping a WIT signature does not keep an obsolete JSON payload contract valid.

Update official Tools' factories, configuration, and needs to final placement contracts.
Their existing aex_official_tool descriptor can remain a MicroVM-owned implementation format;
generic execution does not require every remote Tool to become a Wasm Component. Update its
resolver and existing tool runners together. Do not broaden grants while changing needs syntax.

Keep docs minimal: Brain owns shared semantics and API schemas, extension READMEs own their
implementation-specific setup, and the site imports canonical docs. Link between them rather
than copying another architecture guide. Runnable examples must use the released artifacts;
mark unverified Python packaging explicitly until its real journey passes. Retain historical
ADRs with amendment links; delete old behavior from current guidance and examples.

## Unexpected costs and side effects to verify

| Evidence in current code | Consequence and required response |
| --- | --- |
| runtime.rs uses typed AgentloopPre/ToolPre and separate admission/linkers | Removing Environment turn does not automatically remove guest ABI adapters. Keep those runtime adapters; a universal guest ABI would be a separate, larger change. |
| WorkerService has one Engine and compiled maps per process | N workers replicate resident compiled/prelinked code. Keep lazy loading and measure cache/RSS growth before latency or density claims. |
| Two per-worker capacity classes each use max_concurrent_turns | With finite cap C and memory limit M, N workers permit up to 2*N*C*M guest linear memory, plus runtime/cache overhead. Document this formula; do not present the old per-worker cap as a whole-server cap. |
| WorkerPool serializes admission and publishes through a fixed temporary filename | Parallel admission after extraction can race on the same artifact. Retain the existing admission serialization; test simultaneous admission of identical bytes. |
| Tool dispatch currently journals Tool name, relying on one immutable Environment | Record the selected placement explicitly. Interrupted execution must keep its original target; never select another binding on recovery. |
| ModelRequest currently contains names only | Arbitrary presentation adds schema payload/validation work and changes recorded model-request JSON. Enforce existing byte budgets and canonical dispatch authority separately. |
| Common Rust dispatch does not validate values against Tool JSON Schemas | Canonical input/output validation can reject remote/native results that previously passed through. Test this deliberate strictness and retain meaningful failure results; reuse the existing validator dependency. |
| SDK create always admits loop.component through Brain | Remote Python/JS execution must use opaque remote descriptors; otherwise the supposedly ordinary remote Environment still depends on local Wasm admission. |
| HostToolRegistry keys handlers by name; HostCommand lacks Environment identity | Multi-placement browser Tools can collide. Route by Tool/Environment pair and test reopen, options, events, and cancellation. |
| Host release_session removes the session from every host | Releasing one logical Environment must not release sibling bindings. Narrow lifecycle bookkeeping to the named Environment. |
| Native workspace is keyed only by session | Moving to per-Environment paths changes where existing files live. Do not silently share or copy old workspaces. |
| send_message holds the session lock while awaiting the turn; actor awaits turn before consuming more commands | A loop calling the same session's HTTP lifecycle method can wait on itself. Future model lifecycle Tools require an in-turn service/journal path and must handle self-teardown explicitly. They are not implemented in this batch. |
| HTTP callback registry stores Arc<dyn TurnServices>; HTTP adapter special-cases turn timeouts | Generic execution must preserve cancellation and least-privilege callbacks. A Tool must not acquire the entire turn service set or inherit the wrong deadline. |
| SessionConfig is persisted as JSON and read back into Rust types | Old preview sessions/idempotency responses may no longer load under the changed shape. Follow ADR-030: rebuild consumers and use explicitly owner-reset development data; no automatic deletion or migration. The journal frame format itself need not change. |
| Existing MicroVM driver duplicates an older public Environment protocol | Updating dependency versions cannot make it conform. Replace its Brain-facing contract and lifecycle/dispatch handling; keep needed private guest protocols and sandbox enforcement. |
| Site imports pinned Brain docs but maintains handwritten product/SDK examples | A docs revision bump alone leaves contradictory behavior and obsolete imports on the Brain page and dashboard. Update both sources and inspect the production build. |

## Verification and completion

Run the existing gates without exclusions: contract regeneration/diff, npm test, package-smoke,
npm audit, Rust formatting/clippy, all Rust tests including brain-sessions, Windows/macOS
workspace tests, Linux real-worker integration, HTTP runtime harness, all SDK journey suites,
and Docker image smoke. Update the final build-test dependencies if the loophost job is renamed.

Run downstream gates against the same final artifacts, without exclusions:

- Extensions: existing workspace build/test, dependency verification, package-smoke, npm audit,
  runtime formatting/clippy/all workspace tests, tool-runner tests, and Linux Environment image
  checks. Add final-protocol conformance and real Brain execution of rebuilt Pi/Codex Components
  and official Tools through the MicroVM adapter. Preserve guest-image security assertions.
- Aex: contract generation, build, all workspace tests, verify-brain-overlay, package-smoke,
  and existing Rust/control-plane and CLI gates. The overlay must consume Brain's contract.
- Site: npm run lint, npm run build, npm test, including rendered production HTML/routes.
  Use BRAIN_REPO_PATH for local integration, then verify the immutable docs.lock build. Inspect
  the Brain landing page, dashboard snippet, and affected docs/API/example pages in a browser.
- Platform: existing Terraform/configuration checks and customer canaries, including real
  remote-model and sandbox journeys required by the release process. Prepare the exact npm/
  Rust/image revision tuple; do not replace integrated checks with text-only rename assertions.

Search active source/docs/config for obsolete contracts, factories, paths, and names after
regeneration. Review each remaining occurrence; historical ADRs and fixtures that prove rejection
may mention them. Do not build a permanent compatibility inventory or suppress failing old tests.

Add focused coverage for the behaviors this change introduces:

- Two actual worker PIDs execute concurrent work; each supports both extension profiles.
  Saturated parent capacity still permits nested Tool work. Worker death leaves healthy work
  progressing, does not replay uncertain work, and an old error cannot kill a replacement.
- Both hidden and model-visible placement use the same authorized dispatch, with wrong Tool/
  placement pairs, injected descriptors, and mismatched canonical inputs rejected. Successful
  results satisfy the canonical output schema; generated schemas do not grant authority.
- Browser same-name placements select the right handler/configuration after reconnect; remote
  descriptors execute without local Wasm admission; HTTP Tool callbacks cannot call model/dispatch.
- Journal failure prevents dispatch, selected placement survives interruption inspection, and
  cold transcript/Event reads and request-claim behavior remain unchanged.
- Workspace continuity across workers/turns and actor suspension, isolation between named
  Environments, explicit cleanup, and callback revocation after completion/cancellation.
- The three discussed scenarios become examples/journeys using the finalized APIs. Browser
  JavaScript and sandbox HTTP execution use real adapters. Before claiming Python-in-brain-env
  support, admit and run one real Python-built Component with Brain's WIT/import restrictions;
  test an Agentloop host-service call as well as a pure Tool. Python packaging compatibility is
  unverified today and is not established by a renamed crate or a Rust-only fixture.

Use the existing worker fixtures and telemetry to measure startup, admission, latency, and RSS
at the configured multi-worker capacity. This is a regression check, not a new benchmark suite
or a performance claim. Diagnose increases from duplicated caches or payload serialization.

The stages are implementation/checkpoint order, not permission to publish mutually incompatible
half-upgraded contracts. Release protocol, SDK, reference extensions, and image together under
the existing release process. No runtime tests were run for this documentation-only planning
task; the implementation must pass every applicable gate above.

## Explicit exclusions

No native OS executables in brain-env, bundled Python/Node installation service, retained guest
heaps/connections, cross-server worker fleet, automatic Tool placement, new runtime grants,
default model-facing lifecycle Tool, old-data migration, or replacement of model transports,
journal storage, or telemetry. Exact SDK spelling may follow existing authoring conventions;
the explicit placement and independent-presentation semantics are required.
