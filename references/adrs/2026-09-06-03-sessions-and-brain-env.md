# ADR-044: Separate session management from the built-in execution Environment

- Decision date: 2026-09-06
- Status: Accepted

Implementation plan: [ADR-044 implementation](../plans/2026-09-06-adr-044-implementation.md).
Implemented by the coordinated ADR-044 contract cut.

Refines [ADR-021](2026-09-02-04-session-runtime.md), which assigns multi-session management
to the server; extends [ADR-041](2026-09-05-10-one-execution-model.md), which makes Brain's
own Environment ordinary at the session boundary. Revises the single-worker implementation
described by [ADR-034](2026-09-05-03-wasm-worker.md) and revises the Environment-owned TTL
and cleanup policy in [ADR-037](2026-09-05-06-ephemeral.md).

## Context

Loophost was named for hosting the Agentloop. It now runs both Agentloop and Tool Components,
behind a BrainEnvironment adapter that lives in brain-server. The server also owns the session
registry, lifecycle coordination, artifact admission, and worker readiness. Its Environment
registry holds concrete adapter implementations, tying session coordination to the built-in
runtime. WorkerPool supervises one child process with concurrent invocation slots.

Brain needs a built-in Environment for latency-sensitive loops and small Tools, deployed
alongside the session service. That Environment must support multiple worker processes and
both kinds of execution. An Agentloop activation may be ephemeral while resources used by
Tools remain alive across calls and turns. The session abstraction should not depend on
where the Environment runs, how many workers it has, or how it retains those resources.

## Decision

### Crate responsibilities

| Crate | Owns |
| --- | --- |
| brain | One session's ordered execution, canonical journal, projections, and injected execution ports |
| brain-sessions | Multi-session management: create, load, list, activate, suspend, end, delete, and decide when to call session-scoped Environment lifecycle operations |
| brain-env | The built-in Environment adapter and lifecycle implementations, worker pool and IPC, capability policy, and Environment resources. Links no Wasm runtime |
| brain-env-worker | The worker process: Component admission, Wasmtime execution, and capability enforcement inside the guest sandbox. The only crate that links a Wasm engine |
| brain-server | Deployment configuration and composition, concrete storage and credential wiring, process lifecycle, health, and the API facade delegating to the composed services |
| brain-http | HTTP routes and transport for the server API |
| brain-sdk | Client access and extension authoring |

Extract brain-sessions from the session-management portions of brain-server. Keep brain as the
single-session runtime. Rename brain-loophost to brain-env and move BrainEnvironment and its
policy and resource handling into it. The server composes brain-sessions and brain-env as
separate services. Arbitrary extension code continues to execute only in worker processes.

brain-sessions has no dependency on brain-env or brain-http. It uses an injected Environment
interface and session-scoped services. Put the common Rust Environment adapter interface beside
the execution ports in brain; keep the wire types in brain-protocol. Concrete routing among
the brain, host, and HTTP adapters is assembled by brain-server. Journaled lifecycle coordination
belongs to brain-sessions; worker selection belongs to brain-env.

The server wires session services to the Environment; brain-env bridges them over its worker
transport. Tools receive Tool services, and Agentloops receive turn services. Workers receive
neither the session registry nor direct journal access. Model calls, Tool dispatch, and emitted
Events keep the existing session authority and commit-before-effect rules.

Artifact upload and status may remain on the server API, but admission, validation, and
artifact storage are brain-env operations, not session-management responsibilities. The HTTP
BrainApi interface can retain its current dependency inversion: the server implements it and
delegates to the appropriate service.

### One Environment, multiple worker processes

brain-env must operate a pool of multiple OS worker processes on the server's machine. A
single child with multiple Wasm Stores does not satisfy this decision. Worker count and
invocation capacity are deployment settings injected by brain-server under
[ADR-043](2026-09-06-02-deployment-limits.md).

Each worker can execute both Agentloop and Tool Components. The pool admits work, selects
workers, makes admitted artifacts available to them, and supervises their lifecycle. Sessions
bind to a logical Environment, never a worker. Local IPC remains an implementation detail;
being an ordinary Environment does not require an HTTP hop.

Keep separate capacity for executions that dispatch nested work and for leaf executions, using
caller-granted services rather than an Agentloop-specific Environment operation. A parent
waiting for Tools cannot consume every slot those Tools need. Worker replacement restores
capacity for subsequent work; it never retries
an effect whose execution may have begun. Report failures and unknown outcomes through the
ordinary session machinery. Losing one worker must not restart unrelated healthy workers.

### Invocation lifetime and Environment lifetime

The Environment implements lifecycle operations; its caller decides when to invoke them.
brain-sessions orchestrates the lifecycle of session-bound Environments using the policy
supplied by the host. brain-env implements setup, detach, teardown, and cancellation without
deciding how long a logical Environment should stay up. It does not infer expiry from idle
time, turn completion, or whether its caller happens to run an Agentloop or a Tool. The
server controls the lifetime of the worker pool as deployment infrastructure.

An Agentloop and Tools can use the same logical brain Environment. They can also be placed
in separate named brain Environments when their resources should be separate. Neither an
Agentloop-only Environment type nor a Tool-only Environment type is introduced.

The lifetimes are distinct:

- An invocation lasts for one turn or Tool call. Ending or cancelling it releases its execution
  and its invocation-scoped services, including scratch resources.
- A logical Environment holds resources needed across invocations, such as a workspace,
  until its caller requests the lifecycle operation that releases them. Turn completion or
  actor suspension alone is not such a request.
- A worker process is pool infrastructure. It can serve many invocations and is not the identity
  or lifetime of a session or Environment.

Retained resources are scoped by session id and Environment name. Multiple Tools in one
Environment can share that Environment's resources; another named Environment does not inherit
them merely because it belongs to the same session. The current workspace path keyed only by
session id must change accordingly.

The current caller policy runs setup during session creation, detach at session end, and
teardown at session deletion. That policy lives in brain-sessions, not in the Environment
implementation. Setup establishes resources, detach releases active execution resources,
and teardown removes remaining resources, including retained files. The implementation
performs each operation when asked without needing to know the caller's planned lifetime.
No callback authority survives the invocation that granted it. Environment resources remain
outside canonical session durability: a worker heap, connection, or workspace is not recovered
by replaying the journal, and resource loss must not be presented as successful restoration.

The pool preserves resource identity when scheduling across workers. Machine-local files can
be shared by the workers serving that Environment. Initially each execution gets a fresh Wasm
Store and instance; retained Python/JavaScript heaps, guest globals, and connections are not
provided. Other Environment implementations can retain live instances behind the same protocol.
Session code does not route around resource loss or choose a substitute Environment.

### Generic execution and typed extensions

Replace the Environment protocol's Agentloop-specific Turn/Turned and Tool-specific invocation
shapes with a common execution operation carrying an opaque implementation descriptor,
input, needs, deadline, and invocation-scoped services. Runtime-specific entrypoint and
configuration belong in the descriptor. Return opaque output or
an execution failure. Keep Agentloop input/output interpretation outside the general
Environment interface. The runtime still implements the artifact's ABI and granted host
interfaces; typed Agentloop turn and Tool run APIs can remain above that mechanism.

brain-env continues to execute precompiled Wasm Components. It does not execute arbitrary
native OS binaries or install source-language runtimes. A Python implementation must be built
into a compatible Component with its runtime/dependencies; compatibility requires validation.
Other Environments can interpret their own descriptors for native processes and application
functions. Exact pool sizing defaults and worker-selection policy are implementation choices;
multi-process execution is required.

### Authorized placements and independent model presentation

Separate a Tool's definition from its permitted placements. Each placement fixes an
Environment, implementation/configuration, and needs at session creation. The existing
Tool name and Environment name identify a placement; allow one implementation per pair.
Every dispatch explicitly identifies one authorized placement. A fixed Tool has one; a selectable Tool has
several. Choosing among granted placements does not grant new destinations, credentials, or
arbitrary implementation code. Keep the Agentloop's own execution placement fixed.

The Agentloop supplies model-facing Tool schemas and translates model responses into
authorized dispatches. It can expose ordinary Tools with predetermined placement or expose
meaningful Environment choices. Model-facing names and schemas confer no execution authority;
Brain validates the resolved dispatch against the canonical Tool and its permitted placements
and journals the selected placement before sending it. The actual model presentation is
recorded independently. This extends ADR-018's name-only presentation choices and refines the
single-placement rule in ADR-033/041 and the deferral in ADR-038.

When model-facing lifecycle actions are exposed, they must use the caller's authorized,
journaled lifecycle operations. This decision does not introduce a default lifecycle Tool,
new Environment provisioning during a session, or mutable grants. Initially keep setup during
create, detach at end, and teardown at delete. The protocol remains suitable for Environments
with different internal resource implementations and for either model presentation.

## Alternatives considered

A rename alone leaves Environment behavior split across crates and session management coupled
to worker admission. Moving all server code into brain-sessions preserves that coupling under
a new name. Requiring HTTP for the built-in Environment adds transport overhead without changing
its session semantics. Giving every invocation a new Environment destroys resources that Tools
need across calls. Pinning every session to one worker unnecessarily couples ephemeral turns
to resource ownership.

## Consequences and verification

This is a clean prelaunch cut across Brain, Aex, existing extensions, deployment consumers,
and documentation/site examples. Replace obsolete contracts and names without compatibility
aliases, dual schemas, old-protocol adapters, or data migrations. Preserve ADR-030's explicit
owner reset of incompatible development data; do not delete it automatically.

The implementation moves session coordination and its tests into brain-sessions, moves the
built-in adapter and WIT into brain-env, and updates worker binary names, configuration,
Docker packaging, SDK generation paths, and CI references together. The SDK name brainEnv
and the ordinary Environment protocol already express the intended abstraction.

The current Environment authoring guide and lazy-environment example assign idle expiry to
the Environment itself. Their lifecycle policy must move to the caller as part of this change;
the Environment continues to implement resource allocation and release. This revises the
earlier provider-owned TTL guidance, not just the crate names.

Acceptance requires real multi-process tests: concurrent work reaches distinct workers; both
turns and Tools execute; nested Tool dispatch progresses when turn capacity is occupied; a
worker failure leaves healthy workers usable without replaying uncertain work. Lifecycle tests
must show Tool resources survive turn completion and actor suspension until explicitly
released, an Environment does not expire itself, separate named
Environments have separate resources, and detach and teardown perform their respective cleanup.
Existing journal, recovery, HTTP, remote-Environment, SDK, and image gates remain required.

Cross-server session ownership and tenant fairness remain outside this decision. The local
worker pool does not change a session's granted placements; selection chooses among them.

## Sources

- User discussion, 2026-09-06: normalize loophost into an ordinary built-in Environment,
  extract brain-sessions, and compose both in brain-server beneath HTTP and SDK access.
- User clarification, 2026-09-06: require multiple worker processes and support both ephemeral
  Agentloop execution and longer-lived Tool Environments in brain-env.
- User clarification, 2026-09-06: the Environment does not decide how long it stays up;
  it only supplies lifecycle implementations such as setup and teardown.
- User discussion, 2026-09-06: questions Agentloop-specific turn operations on the Environment
  interface and requires both hidden placement and model-visible Environment choice to remain
  viable options.
- User agreement, 2026-09-06: accepts generic execution with typed extension APIs, Wasm-only
  brain-env, fresh guest instances with retained Environment resources, create-time authorized
  placements, independent model schemas, and caller-owned existing lifecycle milestones.
- User clarification, 2026-09-06: audit the implementation plan against workspace code principles
  and Ponytail, include docs/site and existing extensions, and cut clean without backward compatibility.
- [Current server composition](../../crates/brain-server/src/main.rs).
- [Current session management and artifact API](../../crates/brain-server/src/service.rs).
- [Current Environment adapter](../../crates/brain-server/src/environment/adapter.rs),
  [registry](../../crates/brain-server/src/environment/registry.rs), and
  [built-in implementation](../../crates/brain-server/src/environment/brain.rs).
- [Current worker supervisor](../../crates/brain-loophost/src/supervisor.rs).

[Index](README.md) · [Source coverage and dating](SOURCES.md)
