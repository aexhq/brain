# ADR-054: Keep preparation in Environment configuration and loaders

- Decision date: 2026-09-29
- Revised: 2026-09-30
- Status: Accepted
- Implementation: `brainEnv` startup configuration, explicit SDK preparation, program/runtime artifacts, and worker restoration. Hosted authorization and conveniences are implemented in Aex.

Extends [ADR-046](2026-09-09-01-minimal-extension-contract.md)'s Environment-owned
preparation and [ADR-044](2026-09-06-03-sessions-and-brain-env.md)'s separation of
session management from the built-in Environment. Preserves
[ADR-041](2026-09-05-10-one-execution-model.md)'s implementation-based execution and
[ADR-037](2026-09-05-06-ephemeral.md)'s separation of prepared code from invocation
state. Complements the proposed
[packaging workflow in ADR-049](2026-09-23-01-tool-authoring-and-packaging.md).

## Context

An Environment may need programs, runtimes, dependencies, or assets before it can
execute an Agentloop or Tool. Moving that work earlier can reduce invocation
latency, and the same preparation can serve multiple sessions. Other Environments
already contain everything they need, such as a prebuilt image or a running service.

Applications normally configure their own Environments. The built-in `brainEnv`
needs an explicit configuration and preparation surface because Brain's server
hosts it. Session setup alone cannot prepare shared resources before sessions exist.

Uploading a complete runtime-bearing component for each new program is a related
cost. Preloading known components moves work earlier; separating reusable runtimes
from programs also reduces the work required for previously unseen code.

## Decision

**Preparation belongs to the Environment's configuration and loaders. Brain
continues dispatching an authorized implementation to its explicit Environment.**
There is no mandatory common `register -> execute(handle) -> release` lifecycle,
session registration table, or universal dependency manifest.

| Owner | Responsibility |
| --- | --- |
| Environment | Configuration schema, runtime compatibility, loaders, preparation, reuse, readiness, and resource management |
| Brain session engine | Session authority, durable effect ordering, implementation dispatch, and execution outcomes |
| Brain server | Deployment configuration and management access for the built-in Environment |
| Application or deployment | Preparation timing and resource lifetime |
| Aex | Product authorization, quotas, and conveniences such as ensuring preparation and deciding when to repeat setup or clean up |

### Shared preparation and session setup

Environment preparation establishes reusable resources: for example, compiled code,
installed packages, a language runtime, or a loaded asset. Session setup establishes
that session's binding, grants, and resources, such as its workspace. They have
different lifetimes even when an Environment offers both through related APIs.

Preparation can happen during deployment, through explicit asynchronous setup, or
inside an Environment's loader on first use. Each Environment documents the timing
it supports and uses the same loading mechanisms for eager and lazy preparation.
External Environments may use image builds, native package tools, or provider APIs;
they need no Brain registration operation.

The built-in `brainEnv` supports three entry points into the same Environment-owned
preparation implementation:

| Entry point | Behavior |
| --- | --- |
| Server startup | Operator configuration declares required preparation; complete it before offering the corresponding execution capacity. |
| Explicit async SDK setup | The caller configures the target `brainEnv` and awaits preparation, independently of session creation. |
| Async Aex setup | Aex authorizes the request and calls the same preparation operation, adding product conveniences as needed. |

Ordinary package imports remain passive. An explicit awaited configuration or setup
call can perform I/O and report preparation failures. The Brain SDK exposes this
mechanism; automatic ensure-prepared behavior and session-related cleanup policies
belong in Aex or application code.

Use existing artifact admission, configuration, and management facilities where
they fit. Built-in Environment preparation must be available without a session;
adding a preload list only to session setup is insufficient. Exposing this operation
through Brain's server does not make preparation a session-engine responsibility
or require every Environment to implement the same management API.

### Execution identity and lifetime

Sessions keep authorized implementation descriptors and placements. A component
digest, versioned package descriptor, or resident operation reference identifies
what to execute; the Environment maps it to prepared resources internally. Internal
handles and worker identities do not become session authority.

The same implementation and compatible preparation can serve multiple sessions.
The implementation can also run in multiple compatible Environments. Configuration
updates must not silently change which code an existing session is authorized to
execute. Reuse existing artifact identities and loader caches, coordinating
concurrent preparation at the actual installation boundary.

Session close, suspend, or reopen does not implicitly release shared preparation.
The application or deployment chooses its lifetime, and the Environment implements
that policy. Session-specific resource teardown remains scoped to that session.
Resident services retain their own state under their Environment's ownership rules;
portable guest execution continues to create fresh invocation state.

### Readiness and resource loss

Successful awaited preparation means the declared reusable work has completed on
the execution capacity offered for that workload. An artifact on disk or a live
worker is insufficient. The Environment must route prepared workloads to workers
with the required preparation. Preparing all eligible workers or managing a prepared
subset is an internal choice.

Preparation failure prevents the affected workload from being advertised as ready
or entering its entrypoint. Invalid or unsupported configuration fails explicitly.
Readiness does not promise zero invocation initialization cost or available capacity.

When a worker is replaced, the Environment can reconstruct configured immutable
preparation before offering that worker for the affected workload. Resource loss
ends any readiness claim until preparation is restored. Reconstructing code and
declared assets neither replays an interrupted execution nor silently recreates lost
mutable resources such as a workspace or browser session. Execution failures and
unknown outcomes keep Brain's existing send-once semantics.

### Authority and isolation

Preloading makes resources available; it does not authorize sessions to use them.
Session creation still fixes allowed implementations, placements, and grants.
Preparing an already-authorized implementation again does not widen that authority.
Shared preparation does not share credentials or session state.

Async configuration is limited to resources the caller may manage; it cannot rewrite
shared operator defaults or expose another tenant's resources. Aex owns hosted
authorization and quotas; the Environment enforces resource access. Preparation
that executes package or user code stays outside the Brain session process and
within the Environment's allowed capabilities.

Startup and out-of-session preparation use the Environment's management lifecycle,
without creating a synthetic session or a second session journal. Session-mediated
operations retain commit-before-effect ordering and explicit outcomes.

### Programs and reusable runtimes

Allow Environment loaders to accept programs separately from compatible reusable
language runtimes, so a new Agentloop or Tool need not carry another interpreter.
Preloading and smaller program packaging solve different parts of startup cost;
both belong at the Environment and packaging boundary.

Wasm components, native packages, and resident operations remain supported through
compatible Environments. This decision selects no new language engine and gives
official extensions no special execution path.

## Consequences

Most implementation work belongs in `brain-env` and `brain-env-worker`: configuration,
loaders, preparation reuse, and worker readiness. `brain-server`, `brain-http`, and
the Brain SDK expose startup and explicit management access. The session engine and
common Environment protocol retain implementation-based dispatch without adding a
universal registration lifecycle. Aex supplies authorized configuration and
preparation conveniences.

Applications can prepare an Environment before creating sessions and reuse that
work across sessions. Keeping resources prepared consumes capacity, and new code
still costs time to transfer and load. Environment-specific configuration and
supported loaders remain part of each Environment's documented contract.

## Alternatives considered

- A mandatory registration handle in every execution couples sessions to preparation
  lifetime and imposes bookkeeping on Environments that need none.
- Preloading only during session setup cannot prepare a reusable Environment before
  sessions exist and conflates shared resources with session bindings.
- Preloading selected official components or relying on cache hits alone leaves the
  cost of previously unseen programs and duplicates runtime packaging.

## Sources

- [Codex discussion, 2026-09-29](SOURCES.md#session-01a0ecd4-f869-7ef3-af99-24034b62c721):
  original preparation timing and reuse requirements.
- [Codex review and revision, 2026-09-30](SOURCES.md#session-01a0f0aa-589a-7e12-a8a9-c7a9f05d2c0b):
  explicit caller-owned lifetime, cross-session reuse, Environment-owned preparation,
  startup and async `brainEnv` setup, and approval to revise this ADR.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
