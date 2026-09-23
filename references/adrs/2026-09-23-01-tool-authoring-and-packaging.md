# ADR-049: Default inline Tools to the caller and generate bindings for packaged Tools

- Decision date: 2026-09-23
- Status: Accepted
- Implementation: SDK defaults, publishing helper, neutral bindings, and Node package runtime implemented; downstream Environment integration follows the same contract.

This decision records the authoring and packaging contract. It refines the SDK
surface of [ADR-007](2026-08-27-01-typed-authoring.md) and
[ADR-041](2026-09-05-10-one-execution-model.md), while preserving explicit execution
placements, Environment-owned preparation from
[ADR-046](2026-09-09-01-minimal-extension-contract.md), and the Tool lifecycle from
[ADR-048](2026-09-21-01-tool-completion-and-event-activation.md).

## Context

The current JavaScript SDK requires applications to choose between a `run` function
placed explicitly in `hostEnv` and an `implementation` descriptor interpreted by an
Environment. A declaration such as `implementation: { type: "reference_echo" }`
contains no Tool code. Its behavior lives in the example Environment. This teaches
execution plumbing before showing authors how to implement a Tool.

Official library Tools already separate declarations from executable modules, but
their descriptors name preinstalled implementations. Importing a new Tool package
does not make its code available in an Environment. The ordinary authoring workflow
therefore needs both SDK simplification and a genuine package loading path.

Brain is a multi-language platform. Its Tool definitions already carry JSON Schema,
and invocation inputs and outcomes are JSON. Zod belongs to the JavaScript SDK's
authoring and validation surface. Requiring Zod in that SDK is compatible with the
platform; requiring a Zod object in every generated binding or language integration
would not be.

The caller's runtime can differ from the Tool's runtime. A browser must be able to
import a filesystem Tool for placement in a Node Environment without importing that
Tool's Node dependencies into the browser. A dynamic import inside `run` alone does
not guarantee this separation: ordinary bundlers can still resolve that dependency
while building the caller.

## Decision

### 1. Present two ordinary authoring workflows

An inline Tool is a function with its name, description, and schemas. With no `env`,
it runs in the application process registering it with the session. The SDK supplies
the host placement and registration automatically.

A library Tool is authored, built, and published using its language's ordinary
modules and dependency packaging. Its consumer imports the generated Tool factory
and places it with `{ env, ...options }`. Neither ordinary author writes an
implementation descriptor.

The JavaScript API supports inline and published factories:

```ts
import { tool } from "@aexhq/brain";
import { readText } from "@acme/files";
import { z } from "zod";

const echo = tool({
  name: "echo",
  description: "Echo a message.",
  input: z.object({ message: z.string() }),
  run: ({ message }, ctx) => ctx.finish({ message }),
});

const tools = [echo(), readText({ env: workspace })];
```

Factory invocation configures and places a Tool; it does not invoke `run`. Required
configuration options remain required even when `env` is omitted. The source of a
published JavaScript Tool can use the same `tool({ ..., run })` declaration and
export it normally. Inline use needs no publishing step.

### 2. Resolve the default in the SDK and retain fixed placement

The default is the process registering the Tool through the SDK, such as a Node
process or browser tab. It does not depend on the Agentloop's location. The SDK
resolves omitted placement when creating the session, before Brain admits its
immutable Tool catalogue and Environment bindings.

Reuse the existing host registration, command connection, client ownership, and
reattachment mechanisms. Automatically supplied host names must not collide with
explicit application bindings. Recovery must reuse the original host binding when
handlers are explicitly reattached; an omitted authoring argument does not authorize
moving a stored session to a new host.

An inline function can capture local application state and depends on that host
remaining available. It is not serialized or moved to another runtime by specifying
an incompatible Environment. A packaged Tool can run in the default host when that
host supports its executable; otherwise it fails under the same compatibility rule
as any explicit placement.

The wire still names every authorized Tool/Environment pair. Brain does not infer,
rank, retry, or substitute execution locations.

### 3. Generate client bindings during publishing

Use ordinary compilers and package managers. Provide a small publishing helper or
build integration that generates a client entry from declared package exports and
associates it with the executable entry. The author maintains one Tool definition
and implementation; the consumer imports one factory.

The published result has two responsibilities:

- The client entry exposes Tool metadata, types, configuration, and placement. It
  contains the internal artifact/entrypoint information needed for execution without
  loading the Tool's runtime-only dependencies in the caller.
- The executable entry contains the implementation and uses ordinary package
  dependency metadata. The selected Environment's loader prepares and invokes it.

For a JavaScript library, both entries can ship in one npm package. A library written
in another language uses its normal distribution format; generated bindings for
another caller language may be published alongside it. Language-neutral metadata
connects those bindings to the executable. Publishing a Python Tool must not require
rewriting its implementation or schema in TypeScript.

The helper may consume exports produced by different compilers or library adapters.
It must not depend solely on recognizing a literal `tool(...)` expression in source.
It generates the binding; it does not promise to extract arbitrary live closures or
compile every language to a common runtime. Generation happens during the author's
build/publishing workflow, never inside the Brain session engine.

Resolve executable identity through normal package/artifact identities and the
selected loader. Generated metadata must refer to the implementation from the same
release as the advertised contract. Execution descriptors remain opaque to Brain;
this is not a new universal dependency manifest or a kernel-owned package registry.

The `brain-tools` publishing helper reads `brain.tools` in package.json after
ordinary compilation. It generates `bindTool` client bindings identifying the exact
package version and exported runtime factory. `brain-tool-runtime` executes those
factories from an Environment's prepared Node installation.

### 4. Make package loading an Environment capability

Each Environment implements loaders for the runtimes and ordinary artifacts it
supports. The SDK and Environment integration perform admission or delivery as
needed, keeping code references out of normal application composition. The consumer
does not separately register a new Tool name in the Environment's source.

An Environment can prepare eagerly or lazily within its configured authority, using
normal lockfiles, installers, or prebuilt artifacts as specified by ADR-046. Its loader
must resolve dependencies before loading the entrypoint. New Tool packages must not
require a new hardcoded handler in the runner. An Environment may still document a
limited supported runtime; the built-in Wasm Environment remains a Wasm Environment.

The Environment owns permissions and isolation. Either its loader or the Tool itself
may discover an incompatible runtime, missing facility, or denied operation and fail
the invocation. Use the existing errors/outcomes and record execution failures for
the Agentloop. Reject invalid declarations before dispatch when already known.
No compatibility matching system, automatic placement change, or replacement runtime
is introduced. A Tool error terminates that invocation; the Agentloop decides how
the session proceeds.

Runtime bindings implement input parsing, output serialization, cancellation, events,
and explicit completion. A loader that only runs a subprocess and reads one result
does not by itself implement every lifecycle supported by authored Brain Tools.
Supported capabilities and failures must be explicit.

### 5. Keep language-native authoring above the neutral contract

The common boundary is the existing Tool definition with JSON Schema, JSON
configuration/input/outcomes, and the Environment execution and callback protocols.
Executable artifacts remain runtime-specific. Language-neutral contracts do not mean
every executable runs in every Environment.

The JavaScript/TypeScript SDK may continue to require Zod for ordinary `tool()`
authoring. Another language binding can use that language's schema/type facilities
to produce the same wire definition. For example, a future Python binding could use
Pydantic's JSON Schema generation. No Python author needs to construct a Zod object
or run Node merely to describe a Tool to Brain.

Generated clients and package adapters need a path to consume the normalized Tool
definition directly. In particular, a generated JavaScript client for a Python Tool
must not require reconstructing a Zod schema from Python metadata. This lower-level
binding path is separate from the convenient Zod authoring surface.

Keep parsing and language-specific transformations with their executable binding.
Brain validates the serialized values against the declared wire schemas; the runtime
preserves native validation, defaults, and transformations. A generated foreign client
does not translate arbitrary validators or option-processing functions into another
language. Serializable configuration crosses the boundary, and native processing
runs where its implementation is available.

This decision does not require a general Standard Schema migration of the JavaScript
SDK or a new schema language. A specific JavaScript adapter can use existing schema
interoperability interfaces where useful. Supporting JSON Schema metadata does not
remove input/output validation or broaden session authority.

### 6. Integrate other libraries through existing extension types

There is no additional "framework Tool" extension type. A function defined with a
library such as LangChain or AI SDK can be adapted into an ordinary Brain Tool. An
adapter translates its definition and execution lifecycle into the existing contract.
For example, a library function whose return completes execution can be wrapped to
call `ctx.finish` explicitly.

An integration that drives the session's model turns, edits its shared transcript,
or manages Agentloop state belongs in an Agentloop extension. Tools may supply
model-facing results and make independent model calls within the boundary in
[ADR-050](2026-09-23-02-tool-context-and-model-boundary.md). An integration that
supplies an execution runtime or resources belongs in an Environment extension.
Some libraries combine these responsibilities; their adapters must preserve the
boundary when integrating with Brain.

Tool packaging does not automatically reproduce another library's graph-state
updates, approvals, workflow suspension, or loop-control behavior. Those require the
appropriate extension integration, or an explicit unsupported-operation failure.
Keep such adapters outside the kernel and avoid silently accepting behavior they
discard. Adapters and loaders remain separate so a new library integration can reuse
existing runtime loaders.

## Alternatives considered

- **Keep explicit `hostEnv` and handwritten implementation descriptors.** Preserves
  the current SDK but requires ordinary authors to handle transport/runtime references.
- **Always import executable code into the caller.** Works for compatible local
  applications but breaks callers that only want to place a Tool in another runtime.
- **Require authors to maintain client and executable entries manually.** Compatible
  with ordinary builds but duplicates binding work. Generate the client entry instead.
- **Require one Brain compiler or serialize arbitrary functions.** Couples integrations
  to source syntax and cannot generally move captured process state or native dependencies.
- **Make Zod or Standard Schema the platform contract.** Makes a JavaScript-specific
  authoring interface a cross-language requirement when Brain already uses JSON Schema.
- **Emulate every library inside Brain.** Expands the kernel into runtime and agent
  policy. Existing Tool, Agentloop, and Environment extensions provide the boundaries.

## Consequences and implementation boundary

Inline composition becomes shorter, and published Tools carry their executable
identity automatically. Library authors gain a publishing integration; we must own
its generated bindings and the supported loader contracts. This is more than hiding
the existing descriptor field.

Implementation affects SDK factories/session compilation, packaging helpers,
generated client bindings, and Environment loaders/runners. It must include ordinary
third-party package loading, not just regenerated official-name descriptors. Reuse
the current protocol unless implementation demonstrates a specific missing primitive;
any wire change follows the Rust source-of-truth and generation process.

Document the two workflows and remove normal-author examples that depend on hidden
Environment handlers. Preserve explicit finish, send-once delivery, journal ordering,
outcome uncertainty, resource ownership, and immutable session placement. This ADR
does not authorize rewriting retained session bindings.

Full SDKs for every language, universal cross-compilation, arbitrary closure transfer,
and complete compatibility with every third-party agent library are outside this
change. The design must nevertheless demonstrate a real non-JavaScript Tool and a
caller in another language; a TypeScript-only implementation is not evidence of the
platform boundary working.

## Verification required for implementation

1. Inline `echo()` works without explicit Environment setup, preserves closures and
   typed options, and follows existing client-close and host-reattachment behavior.
2. A third-party library built with ordinary tooling publishes an importable client
   and runnable artifact. A compatible Environment executes it without adding a Tool
   name to the runner's source. Verify from the published package, without repository
   source files supplying missing code or dependencies.
3. A browser imports and places a Node filesystem Tool remotely without loading Node
   dependencies. A real second-language Tool exposes the same neutral contract and
   executes from a different-language caller without a Zod dependency in its authoring.
4. Wire schema validation and native parsing preserve their respective semantics,
   including defaults, transformations, configuration, and output serialization.
5. Unsupported placements can fail in the loader or Tool. They produce clear failures
   without retries or changed placement. Exercise cancellation, intermediate events,
   return-before-finish, explicit completion, and uncertain remote outcomes.
6. Exercise a representative adapter for an existing library function. Required
   library behavior is preserved or explicitly unsupported; it is never silently ignored.
7. Run affected SDK, packaging, runtime, HTTP, and journal integration checks and the
   repository's required CI gates. Research probes alone are not implementation or
   release verification.

## Sources

- Owner discussion, 2026-09-23: two Tool authoring workflows; default inline execution
  in the registering application; Tool-owned compatibility failures; ordinary builds;
  generated client wrappers; multi-language boundaries and existing extension vocabulary.
- Current [SDK authoring](../../packages/brain-sdk/src/extensions.ts),
  [session compilation](../../packages/brain-sdk/src/client.ts),
  [host execution](../../packages/brain-sdk/src/host.ts), and
  [Tool contract](../../crates/brain-protocol/src/tool.rs).
- [Environment wire](../../crates/brain-protocol/src/environment/wire.rs),
  [Tool execution validation](../../crates/brain/src/tool/execution.rs),
  [native Tool WIT](../../crates/brain-env/wit/tool/tool.wit), and
  [Python Environment example](../../examples/python-environment.mjs).
- Official extensions at `c322e5cc9964759413bfc5f84c9b4250aca2c92c`:
  [Tool runtime build](https://github.com/aexhq/extensions/blob/c322e5cc9964759413bfc5f84c9b4250aca2c92c/tools/build-tool-runtime.mjs),
  [local runner](https://github.com/aexhq/extensions/blob/c322e5cc9964759413bfc5f84c9b4250aca2c92c/packages/env-local/image/runner.mjs), and
  [MCP adapter](https://github.com/aexhq/extensions/blob/c322e5cc9964759413bfc5f84c9b4250aca2c92c/packages/tools-mcp/src/index.mjs).
- [Node package entry points](https://nodejs.org/api/packages.html#package-entry-points)
  and [Python entry points](https://packaging.python.org/en/latest/specifications/entry-points/):
  existing language packaging mechanisms.
- [Pydantic JSON Schema](https://docs.pydantic.dev/latest/concepts/json_schema/):
  native Python schema generation.
- [AI SDK Tool execution](https://github.com/vercel/ai/blob/main/packages/provider-utils/src/types/tool-execute-function.ts),
  [LangChain Tools](https://docs.langchain.com/oss/javascript/langchain/tools), and
  [Mastra createTool](https://mastra.ai/reference/tools/create-tool): examples of
  execution and agent-runtime behavior an adapter must interpret deliberately.

[Index](README.md)
