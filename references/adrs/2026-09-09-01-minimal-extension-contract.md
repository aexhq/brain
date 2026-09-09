# ADR-046: Keep dependency preparation in Environments and give Agentloop KV one API

- Proposal date: 2026-09-09
- Status: Accepted
- Implementation: Implemented; coordinated release verification in progress

This record covers the KV and dependency-setup decisions from the extension-interface
review. It does not specify the separately discussed long-lived callback and transcript
submission changes.

This record replaces the universal `needs` declaration in
[ADR-041](2026-09-05-10-one-execution-model.md) and its carriage through setup,
execution, and placements in [ADR-044](2026-09-06-03-sessions-and-brain-env.md).
It refines the Agentloop KV interface in
[ADR-045](2026-09-07-01-protocol-freeze.md#3-persist-author-directed-operations-inline-within-the-turn),
while preserving its inline durable-commit semantics. Their other decisions remain
unchanged.

## Context

The Agentloop reads a KV snapshot from `TurnInput.kv` and writes individual values
through `set_kv`. The split is functional but gives authors two ways to interact with
one state store. There is no deletion operation: writing JSON null stores a value
rather than removing a key. The journal currently has `KvSet` but no KV deletion entry.

Agentloops and Tool placements also declare `needs: string[]`. Brain aggregates those
URIs for Environment setup and forwards them again during execution. The list mixes
software preparation (`pkg:apt/ripgrep`) with resource access
(`file:///workspace?access=write`, `https://api.example.com`). Each Environment must
interpret the same vocabulary despite having different runtimes and capabilities.

A package URI does not resolve interpreter bootstrap, package versions and lockfiles,
native libraries, installation order, or conflicting dependencies. These remain the
Environment's responsibility. The built-in Environment installs nothing and rejects
`pkg:`; it instead uses filesystem and network needs to derive invocation grants,
bounded by deployment policy. Removing `needs` is therefore also a change to grant
configuration, not merely removal of dependency metadata.

Brain already has the useful boundary: an application places an opaque implementation
in an explicitly chosen Environment. That Environment can use its platform's existing
packaging and installation mechanisms. The core does not need another dependency
language to mediate between them.

## Decision

### 1. Expose one Agentloop KV interface

The author-facing interface is:

```ts
await ctx.kv.read(key);
await ctx.kv.put(key, value);
await ctx.kv.delete(key);
```

Its semantics are:

- KV belongs to the session's Agentloop. This does not grant Tools access to it or
  introduce a shared, multi-writer database.
- `read` returns the current value or absence. In JavaScript, absence is `undefined`;
  JSON null remains an ordinary stored value. Other bindings represent absence
  explicitly rather than conflating it with null.
- `put` creates or replaces one JSON value. `delete` removes one key from the current
  state. Deleting an absent key is a no-op.
- Awaited mutations resolve with a committed journal sequence, as `set_kv` does today.
  A no-op may return the already committed sequence. A successful subsequent read in
  the same execution observes the completed mutation.
- Each mutation commits independently. A later turn failure does not undo it. There
  is no explicit author-controlled commit or automatic end-of-turn flush.
- Deletion appends a journal mutation; it does not remove an earlier record. Recovery
  folds put and delete operations to reconstruct current state. Existing reserved-key
  restrictions continue to apply.

The runtime/SDK can initialize its KV view from the existing turn snapshot and update
that view only after successful commits. Under the current single-writer scope, this
does not require a remote request for every read. The snapshot may remain a transport
detail; it is not a second mutable authoring API. Remote and native bindings must
provide equivalent semantics.

Do not add listing, transactions, compare-and-swap, expiry, cross-session access, or
background KV writers for this change. Exact wire encodings are defined in the source
contracts during implementation, not by treating this illustrative TypeScript as an
already exported interface.

### 2. Remove universal `needs`

Remove `needs` from the common Agentloop and Tool authoring contracts, placed session
definitions, and Environment setup and execute requests. Remove its aggregation and
URI validation from Brain. Do not replace it with a structured universal package or
resource manifest.

Keep the existing separation:

- The Tool definition describes the model-visible input and output.
- The implementation descriptor identifies what the selected Environment should run.
- Extension packages carry their normal dependency metadata, lockfiles, setup code,
  or prebuilt artifacts.
- Environment configuration describes the resources and access the application has
  authorized there.

Brain validates fixed placement and the protocol envelope. It does not resolve
packages, choose a language runtime, negotiate compatibility, or select a different
Environment when execution fails. An Environment may document its own descriptor
schema without making that schema part of Brain's universal contract.

### 3. Keep resource authority explicit in Environment configuration

Removing dependency declarations does not grant unrestricted access. The application
configures filesystem, network, secrets, and other access through its chosen
Environment before session creation, within deployment policy. The Environment
enforces those boundaries during preparation and execution. Setup code cannot widen
them by declaring that it needs more access.

For the built-in Environment, move grant selection from `needs` to explicit
Environment-owned configuration, retaining deployment ceilings and deny-by-default
behavior when access is absent. Do not infer permissions from package metadata or
enable broad access to make a failing installation succeed.

This changes the default granularity: extensions in one Environment may share its
configured grants. An Environment can impose finer restrictions itself; applications
can use separate Environment bindings when different access is required. Translating
old per-placement needs into one union of grants must not silently give every Tool
the union's authority. Callers must choose the intended boundary explicitly.

The format of those options belongs to each Environment. Brain does not introduce a
replacement cross-runtime authorization DSL. The existing session boundary and
prohibition on access to unrelated tenants or control-plane resources remain intact.

### 4. Let the Environment prepare an extension before running it

Support optional extension setup through the Environment's implementation loader.
An Environment may build or prepare eagerly, or prepare lazily before the first
execution. Preparation must complete successfully before the extension entrypoint
runs. Extensions that already ship everything they need require no setup hook.

The Environment supplies the bootstrap mechanism: a runtime, command runner, image
builder, or another platform facility. A Python setup function cannot install the
interpreter required to execute itself. Dependencies required when importing a module
must likewise be prepared before that module is loaded. Setup can be a script,
function, or package-manager operation supported by that Environment; it is not a
JavaScript closure that Brain somehow executes in every language runtime.

Use the existing opaque implementation descriptor to identify preparation and
execution where needed. Do not add a Tool-specific Brain setup operation, require
all implementation descriptors in Environment setup, or add a separate readiness
protocol. The generic Environment `setup` operation still establishes the configured
Environment. Its `execute` implementation may prepare the selected extension before
invoking it. The same mechanism applies to Agentloops.

Preparation reuse belongs to the actual installation and resource lifetime, not to a
model turn or automatically to a session. Concurrent first executions must not race
mutating preparation of the same installation; the Environment coordinates that
work using its own runtime/package-management mechanisms. A resource replacement or
explicit installation change can require preparation again. This is not an
exactly-once setup guarantee across crashes.

All setup code runs in the chosen Environment, never in the Brain session-engine
process. Preparation uses only configured authority. Environments must not assume
permission to alter an application's host installation merely because a Tool has a
setup hook.

### 5. Reuse standard packaging for official integrations

Official integrations are ordinary extensions and Environment helpers. For Python
on a supported OS, a helper can use `pyproject.toml`, a lockfile, and `uv` to prepare
an isolated project environment and invoke the extension. A standalone script can
use standard inline dependency metadata. A prebuilt image is an alternative when
runtime installation is undesirable or system dependencies are significant.

These are packaging choices, not Brain requirements. Neither `uv`, a Python version,
nor a container format becomes mandatory in the core. Package managers handle their
own dependency semantics; Brain does not duplicate them in `needs`.

The built-in Wasm Environment remains a Wasm Environment. This decision does not add
native process execution or automatic Python installation to it. A Python extension
needs an Environment that supports it, or a separately built compatible artifact.
An unsupported implementation fails explicitly instead of triggering a fallback.

### 6. Report preparation failures through existing outcomes

Brain records the Environment operation intent before delivery. Preparation inside
that operation does not bypass the commit-before-effect boundary. Failure prevents
entrypoint execution and is returned through the ordinary operation/turn/Tool error
path, with useful details about what could not be prepared. If the result is lost,
the outcome remains unknown rather than being assumed not to have happened.

Neither a failed setup nor an interrupted execution triggers automatic replay by
Brain. A caller or Agentloop may explicitly request another attempt where applicable.
Partial files or other external effects are not automatically rolled back, and a
successful session creation is not proof that every lazily loaded extension is
compatible. Lazy preparation deliberately moves some failures to first use.

## Alternatives considered

- **Keep URI needs or replace them with typed dependency objects.** Both require a
  cross-runtime vocabulary while leaving bootstrap, dependency resolution, and
  installation to the Environment. Typed Environment options and existing package
  formats already cover the respective concerns.
- **Make every Tool self-install from its run function.** This can work in a suitable
  Environment but cannot bootstrap a missing interpreter or imports needed to load
  the function. Optional preparation allows those prerequisites to be handled first.
- **Require a container image for every extension.** Useful for some deployments, but
  unnecessarily excludes host functions, Wasm Components, and ordinary package runners.
- **Have Brain run setup scripts or choose an appropriate Environment.** Violates the
  execution boundary and fixed placement, and makes Brain own runtime-specific policy.
- **Keep snapshot reads and `set_kv`, treating null as deletion.** Avoids an operation
  but loses the distinction between a stored null and a missing key. A coherent KV
  interface with explicit deletion is small and unambiguous.

## Consequences and implementation boundary

The public protocol becomes smaller around dependencies and more complete around
durable Agentloop state. Environment authors gain freedom to use normal packaging,
but compatibility is an explicit responsibility of the selected implementation and
Environment, not a portability promise made by Brain.

Implementation requires a coordinated change to:

- Rust protocol types, SDK authoring, and generated session/Environment contracts to
  remove `needs`; session creation must stop aggregating it.
- The built-in Environment's configuration and grant enforcement, plus official Tool
  declarations, examples, and tests. Removing declarations must not remove enforcement.
- Agentloop service bindings and wrappers, including native and HTTP paths, for the
  KV interface; journal mutation representation and recovery for deletion.
- The official loops, which currently read `input.kv` and call `setKv`.
- Authoring and Environment documentation, plus the corresponding resource-declaration
  wording in the workspace's `platform/docs/core.md`.

Apply the existing pre-stable contract-change policy; regenerate contracts from their
owning sources rather than editing generated files. Do not retain two permanent
authoring APIs or silently reinterpret old grant declarations. This decision authorizes
no deletion or conversion of existing session data.

## Validation required for implementation

1. KV distinguishes missing from null, supports put/read/delete and absent-key delete,
   observes completed writes within a turn, and recovers committed mutations after
   interruption/restart without deleting historical journal entries. Native and HTTP
   Agentloops have the same behavior. Tools gain no KV access.
2. Generated contracts, SDK factories, examples, and official extensions agree on
   removal of `needs`; explicit placement remains enforced.
3. Configured filesystem, network, and secret access works in the built-in Environment;
   absent and deployment-denied access stays denied during setup and execution. A
   separate Environment binding does not inherit another binding's grants.
4. A real supported Python Environment prepares a package before imports and execution.
   Setup failure prevents the entrypoint, and concurrent first calls cannot execute
   against a partially prepared shared installation. A preprepared extension needs no
   redundant custom hook; unsupported placement fails honestly.
5. Lost responses and setup failures produce explicit errors/unknown outcomes without
   automatic replay, replacement placement, or a claim that partial effects were undone.
6. Run the affected existing tests and required CI gates, including real Linux runtime,
   HTTP, journal recovery, SDK, and official-extension integration coverage. A draft
   document or isolated helper test is not evidence that the implementation is complete.

## Out of scope

Long-lived callback authorization, queued Tool transcript submissions, the proposed
`tool_returned` vocabulary, and callback wakeup semantics need their own record. This
ADR does not reintroduce universal Tool sync/async or timeout options. It does not add
Tool KV access, per-message transcript CRUD, durable workflows, dynamic placement, a
new Environment retention policy, or a universal installer.

## Sources

- [Current Agentloop input](../../crates/brain-protocol/src/agentloop/turn.rs),
  [state services](../../crates/brain/src/session/services.rs), and
  [journal projection](../../crates/brain/src/journal/store.rs).
- [SDK extension factories](../../packages/brain-sdk/src/extensions.ts) and
  [Environment wire types](../../crates/brain-protocol/src/environment/wire.rs).
- [Native grants and package refusal](../../crates/brain-env/src/environment.rs).
- [Environment lifecycle coordination](../../crates/brain-sessions/src/environment.rs)
  and [execution adapters](../../crates/brain-sessions/src/execution.rs).
- [uv Python management](https://docs.astral.sh/uv/concepts/python-versions/) and
  [project synchronization](https://docs.astral.sh/uv/concepts/projects/sync/): existing
  mechanisms for interpreter discovery/installation and locked project preparation.
- [uv script support](https://docs.astral.sh/uv/guides/scripts/): existing script
  dependency metadata and isolated execution, without a Brain-specific needs list.
- [Docker's Python guide](https://docs.docker.com/guides/python/): an image-based
  preparation alternative.
- User discussion, 2026-09-09: requests a cohesive KV API, removal of universal needs,
  Environment-executed setup, standard runtime tooling, and a smaller Brain contract.

[Index](README.md)

## Implemented bindings

Native WIT imports are `kv-read(key) -> option<string>`, `kv-put(key, value-json) -> u64`,
and `kv-delete(key) -> u64`, each returning the ordinary turn-error result. HTTP methods
are `kv_read`, `kv_put`, and `kv_delete`. Read/delete take an identifier string; put takes
`{ key, value }`. HTTP read returns `{}` for absence or `{ value }`, including null.
Official JavaScript loops expose `ctx.kv.read/put/delete` and no `setKv` authoring API.

Native Environment configuration accepts `filesystem: { workspace?, scratch? }` with
`read`/`write` values, `network: string[]` of HTTP(S) origins, and `secrets: string[]`.
Omitted grants stay denied and deployment allow-lists remain the ceiling.

The Python project example uses pinned standard tooling and tests actual setup/import order,
concurrent preparation, retained installations, explicit failures, and unsupported descriptors.
This pre-stable contract cut requires rebuilt Agentloop Components and matching SDK/server
versions. It does not migrate retained sessions or authorize deleting them during deployment.
