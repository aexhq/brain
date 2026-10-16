# ADR 0003: Application SDK, session bindings, and harness integration

- **Status:** Proposed. Go application SDK and server-default credentials with optional session overrides selected; callback execution, credential custody, and native-tool policy remain open. See [Decisions to finalize](#decisions-to-finalize).
- **Date:** 2026-10-16
- **Extends, if accepted:** [ADR 0001](0001-local-first-session-server.md) and [ADR 0002](0002-subsystem-contracts-and-sdk-delivery.md).
- **Scope:** Architecture and SDK semantics, not implementation, package names, or a final wire schema.

## Proposal

Make Brain an application developers compose through an application SDK. The application installs harness adapters and tool providers; clients create sessions by binding those installed capabilities to authorized configuration and credentials. Tools can be application-owned functions, independently deployed services, or tools discovered from an MCP server. Every supported harness uses the same Brain tool-dispatch boundary.

**One application means one owned deployment and lifecycle, not necessarily one process.** A function passed to the application SDK is executable code. A value passed to a remote client SDK is a serializable request. Similar constructor syntax must not hide that difference.

This changes the requirement that every tool needs an independently operated service. It preserves the small session coordinator, harness-owned inference and context, single-owner sessions, local-first state, passive recovery, explicit error layers, and absence of automatic tool-action retries.

## Context and implementation baseline

The code at `415b7c1`, rather than older plans, establishes the starting point:

| Area | Implemented behavior | Consequence for this proposal |
| --- | --- | --- |
| Application composition | [`cmd/brain/main.go`](../../cmd/brain/main.go) opens concrete dependencies and owns shutdown. Extension interfaces and their types are under `internal/`. | There is a composition root, but no public application SDK that another Go module can import. |
| Client SDKs | [`core.CreateSession`](../../internal/core/types.go) selects a configured harness and a list of tool names. The [TypeScript client](../../sdks/typescript/src/index.ts) serializes HTTP requests; Python exposes the same server contract. | An SDK helper alone cannot add session-specific executable functions, endpoints, or credentials. |
| Tools | [`tools.Registry`](../../internal/tools/registry.go) binds a host-global catalog at startup, with tenant authorization, schemas, limits, and transport dispatch. | Separate installed provider definitions from each session's resolved bindings; retain one policy path. |
| MCP | [`tools.MCPTransport`](../../internal/tools/mcp.go) supports remote Streamable HTTP, explicit discovery, progress, cancellation, and no action replay. Discovery produces static bindings. | This is an existing transport, not a new integration to build from zero. Its connection/authentication settings should belong to an MCP provider rather than be repeated for each tool. |
| Harnesses | The [Pi runner](../../runners/pi/src/main.ts) delegates tool calls to Brain and excludes ambient tools and resources. [Recorded probes](../research/harness-probes.md) cover Codex 0.163.0 and OpenCode SDK 2.0.30, not completed adapters. | Constructors for multiple harnesses must not imply equivalent support before qualification. |
| Provider integrations | [`internal/toolservice`](../../internal/toolservice/service.go) and [`services/sandbox`](../../services/sandbox/README.md) contain storage, web, and sandbox services. | Reuse their execution and isolation rules when packaging providers. Do not replace tested provider behavior with new wrappers solely for naming consistency. |

The [registry policy tests](../../internal/tools/registry_test.go), [MCP transport tests](../../internal/tools/mcp_test.go), and [Pi restore tests](../../internal/session/pi_restore_test.go) are existing behavioral evidence. No new harness or live-provider qualification was performed for this ADR. Integration maturity remains on the [roadmap](../roadmap.md).

## Application and client SDKs have different authority

| Surface | What the developer supplies | What it may do |
| --- | --- | --- |
| Application SDK | Trusted executable factories, tool handlers, authorization and credential resolvers, persistence, limits, and server settings | Install code, open resources, build the server, start it, and drain/close what it owns |
| Harness integration package | An adapter factory and validated adapter-specific settings | Run the native harness and translate its events, tool callbacks, cancellation, and checkpoints |
| Tool integration package | Tool definitions or a provider that binds configuration into a set of executable tools | Access the configured external resource and enforce resource-specific authorization, approvals, quotas, and cleanup |
| Client SDK | Harness selection, session options, tool/provider references, permitted binding parameters, and credential selection or permitted private overrides | Request a session from an application that has authorized those capabilities |

The application SDK is the composition root, not a second agent loop. The stock Brain executable becomes one application built with that SDK. Configuration-file startup and code-based startup must use the same validation and runtime; they are not separate implementations.

Expose the small public contracts needed for composition, not every internal subsystem. Application dependencies are installed and pinned at build/deployment time. Session creation never downloads packages, evaluates source, or accepts a subprocess command.

Keep the generated HTTP contract and client wire types authoritative as in ADR 0002. Handwritten SDK constructors produce that contract; they do not become a competing source of protocol types. Preserve the published clients' release rules without introducing versioned routes or runtime compatibility negotiation.

### Illustrative application

The following is pseudocode. Names distinguish executable factories from remote references; they do not select a programming language or commit to signatures.

```text
server = BrainServer(
  agentloops = {
    pi: PiRuntime(...),
    codex: CodexRuntime(...),
    opencode: OpenCodeRuntime(...)
  },
  tools = {
    invoice: Tool(input = ..., output = ..., run = lookupInvoice),
    documents: S3StorageProvider(credentials = serverStorageCredential, ...),
    database: SupabaseProvider(
      credentials = serverDatabaseCredential, allowedSessionOptions = ...
    ),
    compute: ModalSandboxProvider(credentials = serverComputeCredential, ...),
    lambda: HTTPToolProvider(endpoint = ...)
  },
  mcpServers = {
    crm: RemoteMCP(endpoint = ..., allowedTools = ["search", "get"]),
    localDocs: StdioMCP(command = installedCommand, allowedTools = ["lookup"])
  },
  authorize = applicationPolicy,
  credentials = applicationCredentialResolver,
  persistence = CheckpointArchive(...),
  ...
)
server.start()
```

Registering an integration does not give every session all its tools. `mcpServers` is an application-SDK convenience for named MCP providers; it lowers into the same tool-provider registry rather than adding a second dispatch system or public MCP-server resource.

### Illustrative remote session

```text
brain = BrainClient(url = ..., token = ...)
session = brain.sessions.create(
  agentloop = CodexRef("codex", model = ..., reasoning = ...),
  tools = [
    ToolRef("invoice"),
    S3StorageBinding("documents", alias = "files", operations = ["read", "list"]),
    SupabaseBinding(
      "database",
      alias = "customerData",
      project = approvedProject,
      credential = SessionCredential(...),
      operations = ["select"]
    ),
    ModalSandboxBinding("compute", alias = "workspace", limits = ...),
    ToolRef("lambda"),
    MCPTools("crm", alias = "crm", select = ["search"])
  ]
)
```

`CodexRef` selects an installed adapter; it does not run Codex in the caller. `SupabaseBinding` configures an installed provider; importing it does not install server code. A `MyLambda(...)` client helper can produce an authorized remote reference, but importing a function from a local source file does not deploy it to Lambda.

Omitting `credential` selects the tool's authorized server default. `SessionCredential(...)` illustrates an optional private override, not a model argument. Whether that helper accepts a secret value, a reference, or both, and how long a supplied value remains available, is part of the open custody decision.

A raw `run` callback in this remote request is rejected, not silently omitted by JSON serialization. Supporting that exact shape requires choosing the connected-executor or deployment alternative below.

### What crosses the network

A **closure** is a function together with values it captures from the code around it. For example, a `run` function that calls a locally created database client depends on that client and its credentials. Those objects live in the process that created them.

The **session JSON** is the network request produced by the client SDK, not a JSON file the application developer must write. It can carry a tool identifier, configuration, and an authorized private credential input. It does not automatically transfer executable functions, database connections, or caller memory to the Brain server.

The desired SDK syntax can still hide transport details. For a caller-local `run` function, it would register a connected executor and route tool calls back to that process. For server execution, the function must already be installed in the Brain application or deployed to a callable service. This is a placement and lifetime decision, not a requirement to expose low-level JSON to SDK users.

## Tool definitions, providers, and session bindings

Keep three concepts distinct:

| Concept | Contents | Lifetime |
| --- | --- | --- |
| Tool definition | Model-facing name, description, input schema, optional output schema, and executable handler | Installed application code |
| Tool provider | Trusted factory, binding-parameter schema, credential requirements, resource policy, and discovery/execution lifecycle | Application registration; creates authorized session bindings |
| Session binding | Provider identity, session-local alias, selected operations, non-secret configuration and credential-selection metadata, and resolved tool descriptors | Persisted with the session; executable objects are reconstructed; secret values are not persisted here |

A single function is a provider with one tool and no configurable resource. S3, Supabase, a sandbox, and an MCP server can each supply several tools. The model sees the selected operations, not a generic privileged `runAnything` method.

Separate **binding parameters** from **model arguments**. A bucket, project, credential reference, sandbox limit, or approved endpoint is bound by application policy. It is not an argument the model can replace. If an operation needs a model-selected resource, its handler must authorize that resource within the binding.

### Binding and selection

Session admission authenticates the caller, resolves registered providers, validates requested parameters and operations, authorizes the credential selection and any override, and resolves the exact tool descriptors. Only then does it persist the session. Discovery may read provider metadata, but admission does not execute model-visible tools or provision a sandbox.

The session's tool list is an explicit selection, not the union of every server registration and client request. Application defaults apply only when omission is explicitly defined; an explicit empty list means no tools. A request that exceeds application policy is rejected rather than silently widened or narrowed. Two bindings of the same provider, such as two S3 buckets, require distinct aliases.

Resolve model-visible names deterministically from the binding alias and operation. Keep the reverse mapping to the original provider/MCP name. Validate uniqueness and harness naming constraints before accepting the binding; do not rely on lossy name sanitization or an MCP server's self-reported name for uniqueness.

Persist the selected descriptors, their order, provider/configuration identity, and non-secret credential-selection metadata. Restoring or redeploying must not silently reinterpret a tool name against a changed catalog. Require the matching deployed implementation and descriptor identity, or fail explicitly. This is a recovery check against changed code/configuration, not a schema-migration or old-plugin support system.

This ADR covers binding at session creation. Idle-boundary configuration changes remain separate work in [plan 05](../plans/05-api-and-sdks.md#future-configuration-updates-proposed). Revocation remains effective at call time even when a descriptor is unchanged.

### Provider lifecycle and resource ownership

The application owns shared connections and process lifecycles. Bindings own session-scoped execution context. A handler receives trusted tenant, session, and call identity, a cancellation signal, and a progress emitter separately from model arguments.

Do not store mutable per-session credentials in a shared client's global headers or environment. Shared network pools are acceptable; sharing authorization state between bindings is not.

Binding, opening, and restoring must not recreate paid resources or replay actions. A sandbox is created by an explicit tool action; subsequent calls use a scoped provider handle. Session checkpoints can preserve that handle, but do not checkpoint the external machine or promise it still exists. Expired handles produce tool errors. The provider/application owns expiry and cleanup, including after a Brain crash; a shutdown callback alone is insufficient.

S3/R2 **agent tools** and S3/R2 **checkpoint stores** are different roles, with separate configuration and credentials. Supabase data-access tools and a PostgreSQL session index are likewise distinct. A vendor name is not a reason to merge these contracts.

`FlyMachine`, `ModalSandbox`, and other compute integrations are tool providers, not new Brain environment or job resources. Their operations need not be interchangeable. A provider package may wrap a vendor SDK or an existing MCP server; choose from the required operations, authorization model, and qualification evidence rather than requiring one integration method for every vendor.

## One tool-call boundary, independent of execution location

The call path is:

```text
harness callback
  -> Brain session membership and call identity
  -> authorization, argument schema, deadline, and output policy
  -> application handler OR HTTP/SSE adapter OR MCP adapter
  -> normalized tool result and event
  -> harness continuation
```

The application can package trusted functions with Brain without making the session coordinator responsible for S3, SQL, sandbox, or approval logic. Independent services remain available for a different language, isolation, or separate scaling. No mandatory separately deployed tool gateway is introduced.

An in-process handler shares its host's failure and trust domain. A deadline bounds how long Brain waits, not whether arbitrary code has stopped. Handlers must honor cancellation; work requiring forced termination or untrusted code belongs in a separate process or sandbox. A subprocess is also not, by itself, a tenant security sandbox.

| Failure | Required behavior |
| --- | --- |
| Invalid session binding, forbidden resource, unsupported harness option | Reject admission; do not create a partly valid session |
| Tool denial, provider failure, cancellation, or malformed tool output | Return a model-visible tool error, preserving whether an action may have happened |
| Connection/process loss after dispatch without a definitive outcome | Mark the outcome unknown; never automatically resend the action |
| Harness protocol failure, core persistence failure, or broken coordinator invariant | Stop the affected execution as a Brain error |
| Invalid application registration or failure opening a required startup resource | Fail startup and close already-opened owned resources |

Provider SDK retry defaults must not silently reintroduce action retries. Reopening a connection or restarting a process for a future call is different from replaying the previous call. Restoring history and reconnecting event observers never execute tools.

### Input, output, and progress

Use JSON Schema as the wire description, with language-specific typed authoring helpers. Preserve the distinction between successful structured output, model-readable content, progress, and an error.

The proposed `output` schema describes a tool's **successful structured payload**, not Brain's complete result/error envelope. This deliberately changes the present registry's envelope-schema interpretation. MCP's `outputSchema` has the structured-payload meaning, and custom `{ input, output, run }` authoring should mean the same thing. Errors do not need to satisfy a success schema.

Preserve MCP structured content rather than only converting it to text. An adapter can render structured data as JSON text for a harness that needs text, without losing the stored structured value. Content that a harness cannot represent must have an explicit conversion policy or fail; do not silently discard it. Resource links are data, not permission to fetch a URL or read a file.

Progress does not commit an action or authorize continuation. Final results remain bounded and tied to the authenticated call. Tool output cannot inject session/control events.

## MCP is a provider, not the architecture of the whole application

Brain owns MCP discovery, selected-tool mapping, credentials, invocation, cancellation, and normalization once, outside the individual harnesses. Pi custom tools, Codex dynamic-tool callbacks, and OpenCode tool transforms bridge into that same dispatcher.

Use the official MCP SDK rather than reproducing the protocol. The [2026-07-28 transport specification][mcp-transports] is request-scoped: Streamable HTTP uses POST with JSON or a request-scoped SSE response, and this revision removes protocol-level sessions and the old standalone GET stream. Do not equate a Brain session, an SDK connection object, and an MCP protocol session. External protocol support does not imply versioning Brain's own API.

**Remote MCP:** A provider holds an endpoint policy, authentication resolver, and explicit tool selection. Discovery uses the binding's authorized credentials because catalogs can differ by authorization. Discovery caches must not cross those boundaries. Catalog changes affect new bindings, not the tool set in a running session. A stored descriptor cannot prove that a third-party server's implementation has stayed identical; incompatible rediscovery must fail rather than silently replace the contract.

**Stdio MCP:** The application installs the executable, supplies an explicit environment and working directory, and owns startup, cancellation, stderr handling, and shutdown. A remote caller cannot submit a command or trigger package installation. Default to session-scoped processes when credentials or mutable state are session-specific; sharing requires an explicit provider contract. Use OS isolation when the server is not trusted application code.

Do not pass the Brain API token through as an upstream MCP credential. Use credentials intended for the upstream resource, and retain tenant/session/call identity as trusted metadata where the server supports it. Third-party MCP servers may ignore Brain-specific metadata; their own authentication and resource authorization must establish access. A metadata field is not an isolation guarantee.

Tools are the supported MCP capability in this proposal. Sampling, elicitation, roots, prompts, resources, and multi-round-trip input are separate decisions, not implicit effects of attaching a server. Preserve explicit failure for unsupported requests, no automatic inference, and provider-owned approvals. Adding interactive MCP later requires a defined user interaction and continuation contract; blindly retrying `tools/call` is not that contract.

The strong alternative is to let each harness connect to MCP directly. It reduces adapter work and exposes native MCP features sooner, but duplicates authentication, discovery, policy, event accounting, and recovery behavior. It is not interchangeable with the common dispatch guarantee.

## Harness support is an adapter contract, not a constructor name

Use **harness** for the implementation concept; `agentloop` is a possible SDK spelling. Do not maintain two separate concepts or two equivalent wire fields merely for naming.

### What the upstream SDKs and servers expose

These are existing upstream interfaces, not proposed Brain method names:

| Surface | Control interface | Custom-tool integration |
| --- | --- | --- |
| **Pi SDK** | `@earendil-works/pi-coding-agent` embeds the agent in Node/Bun. `createAgentSession()`, `session.prompt()`, `subscribe()`, `steer()`, `abort()`, and `dispose()` control the loop; `SessionManager` owns native state. | `customTools` supplies schemas and `execute` callbacks. Settings/resource loading control built-in tools and extensions. Brain's existing runner embeds this SDK and bridges callbacks to Go. |
| **Pi RPC** | `pi --mode rpc` is a persistent subprocess with JSONL commands, responses, and events. `RpcClient` supplies a typed wrapper, including `promptAndWait()`. | Uses the native Pi tools/extensions loaded into that subprocess. It is a separate integration choice from Brain's custom SDK-based runner, not the same wire protocol. |
| **OpenCode embedded SDK** | `OpenCode.create({ plugins })` from `@opencode/sdk` creates an owned host. `sessions.create()`, `sessions.prompt()`, `events.subscribe()`, and `close()` operate through its in-memory HTTP router, without opening a listener. | `Plugin.define()` and `ctx.tool.transform()` add, update, or remove tool definitions and executors. Executors receive a cancellation signal and progress context. |
| **OpenCode network client** | `OpenCode.make({ baseUrl })` from `@opencode/client` calls a running HTTP server. It exposes `client.session.create()`, `session.prompt()`, and `event.subscribe()`. The optional service helper can manage a local `opencode serve --service` process. | Plugin implementation is installed on the server. Importing a plugin RPC contract in the client provides types and methods; it does not register server code. |
| **Codex SDKs** | The TypeScript SDK wraps the Codex CLI subprocess: `new Codex()`, `startThread()`, `resumeThread()`, `thread.run()`, and `runStreamed()`. The Python SDK exposes `Codex`/`AsyncCodex` and `thread_start()` over local App Server JSON-RPC. | These retain the native Codex runtime and its configured tools. Use the App Server callback contract below for the proposed Brain adapter rather than assuming a TypeScript `run` callback becomes a Codex tool. |
| **Codex App Server** | `codex app-server` exposes bidirectional JSON-RPC over stdio by default. Methods include `initialize`, `thread/start`, `thread/resume`, `turn/start`, `turn/steer`, and `turn/interrupt`; notifications include `item/agentMessage/delta` and `turn/completed`. | Register `dynamicTools` when starting a thread. App Server sends `item/tool/call` to its client, which executes the tool and replies with `contentItems` and `success`. Native approvals and MCP interactions have their own interfaces. This is not an MCP server protocol. |

Sources: [Pi SDK][pi-sdk] and [CLI/RPC integration][pi-rpc]; [OpenCode SDK][opencode-sdk], [network client][opencode-client], and [plugins][opencode-plugins]; [Codex SDKs][codex-sdk], [TypeScript SDK implementation guide][codex-typescript], [App Server][codex-app-server], and [dynamic-tool callback declaration][codex-dynamic-protocol].

All three already own an agent loop: model calls, context, tool selection, continuation, and native session state. Brain integrates those loops rather than replacing them with its own provider client. A shared SDK constructor can hide these different control interfaces without pretending they are the same runtime.

### What tool ownership means

For a Brain-managed tool, the harness asks Brain to dispatch the action:

```text
model -> harness -> Brain dispatcher -> configured tool implementation -> harness
```

For a native tool, the harness's own implementation executes the action:

```text
model -> harness's shell/file/MCP implementation -> harness
```

Both arrangements can support custom Supabase, storage, and sandbox tools, and both keep inference/context in the harness. The open question is whether native actions may execute outside the Brain dispatcher. Observing a native tool event is not the same as authorizing and dispatching the action. A hybrid can explicitly retain selected native tools, but must state the different policy and recovery guarantees.

### Proposed baseline and qualification

Install adapters at application startup and select one for each session. Separate installation settings, such as executable paths and credentials, from validated session settings, such as model and reasoning effort. An arbitrary native configuration dictionary must not override isolation, MCP ownership, process commands, or recovery policy.

Require a baseline: harness-owned inference and context; exact Brain-bound tool access; cancellation; normalized events; no ambient credentials/resources; passive native restore; context preservation after compaction; and coherent checkpoints with writes proportional to changed data. Optional features, such as steering or native structured responses, have explicit capabilities. Unsupported requests fail instead of being ignored or falling back to another harness/model.

| Harness | Upstream integration surface | Evidence and remaining architectural issue |
| --- | --- | --- |
| Pi | [Embedded SDK][pi-sdk], custom tool callbacks, native `SessionManager` | Existing reference runner. Native session state, not reconstructed event text, owns the next prompt. |
| Codex | [App Server][codex-app-server], dynamic tools and callbacks | The recorded 0.163.0 probe still exposed `request_user_input` after disabling execution tools. An upstream control, narrow patch, or explicit external question-tool mapping must be qualified. Merely adding dynamic tools or a read-only sandbox does not prove exclusion. |
| OpenCode | [Embedded SDK][opencode-sdk] and [tool transforms][opencode-plugins] | The recorded 2.0.30 host resumed suspended sessions at startup; mutable SQLite state also needs a coherent incremental checkpoint adapter. Documented embedding alone does not satisfy passive restore. |

These probe findings are version-specific evidence from 2026-10-15, not claims that every later release has the same behavior. Reading newer documentation does not replace rerunning qualification against a selected, pinned release.

Wrapping Codex as a tool inside Pi is a valid nested-agent feature, but it does not implement a Codex session harness. It introduces a second agent loop and separate context. The [Agents SDK's Codex tool][agents-tools] illustrates this distinction.

A strong alternative is an explicitly **sandbox-native harness profile**: let Codex/OpenCode retain their own shell, files, approvals, and MCP within an isolated workspace. That preserves more upstream behavior and may shorten integration work, but changes tool ownership and the exact-tool guarantee. It needs separately stated event, approval, filesystem, credential, and recovery semantics; it must not silently pass as the baseline adapter.

## Credentials and untrusted configuration

Selected direction: supply default credentials when constructing a tool/provider in the Brain application, and allow an authorized session to supply its own credentials for that binding. A default can be a credential value or an application resolver. Reusing the default and supplying an override use the same installed tool implementation; neither requires a new agent loop or tool deployment.

An override affects only its binding and never mutates the provider's shared default. Application policy authorizes use of both defaults and overrides. An invalid or revoked override fails rather than falling back to a possibly more privileged server credential. Tool credentials go only to the execution component that needs them, not the model or harness.

The remaining choice is custody, not whether overrides are useful. The SDK may accept a convenient credential value while separating it from the persisted session configuration. Direct values need an authenticated, protected ingress path and an explicit lifetime. A reference can resolve through the host environment, the application's existing secret store, or an external credential service. Authorize references against the authenticated tenant and permitted provider/resource; an opaque name alone is not authorization.

Recommend references for persisted bindings, without requiring that every user-facing SDK call expose a reference-registration step. A helper that accepts a raw key still needs an explicit storage or transient-use policy underneath it. Transient secrets disappear on restart and must be resupplied before calls can execute. Persistent custody requires encryption/key ownership, rotation, deletion, and recovery rules. Brain-managed secret storage has not been selected.

An application-provided resolver does not require Brain to become a vault, identity provider, or OAuth control plane. Rotation changes the resolved secret without expanding the authorized binding; revocation affects subsequent dispatch.

Credentials must stay out of descriptors, model arguments, session events, checkpoints, and diagnostics. This prevents the framework from serializing credentials; it cannot make a trusted tool safe if the tool itself deliberately returns a secret. Provider code still owns what it emits.

Per-session URLs are also authority, not harmless configuration. Allow them only through a registered provider's endpoint policy, with credential audience binding, redirect rules, and network restrictions appropriate to that provider. Otherwise a session creator could cause server-side requests to internal services or send credentials to a different host. These checks address new reachable failures introduced by client-supplied bindings.

For Supabase in particular, a secret/service-role key bypasses row-level security. Tenant metadata does not restore RLS. Prefer user-scoped authorization where appropriate, or enforce resource policy in the tool implementation. See [Supabase API keys][supabase-keys] and [MCP authorization][mcp-auth].

## Decisions to finalize

These are product choices, not implementation details to leave to an adapter author. The review selected D1 and the default/override model in D3. The other choices remain open; questions about them are not acceptance of the recommended variants.

### D1: Application SDK runtime and language

| Variant | Benefit | Cost / reason to choose it |
| --- | --- | --- |
| **Selected: public Go application SDK, retain Go core** | Smallest change to the existing composition root; trusted Go functions can run directly; harnesses remain language-independent subprocesses | JavaScript/Python handlers still need an explicit process/service host. Client SDKs remain independent of the application SDK's language. |
| **TypeScript application host supervising the Go core** | JavaScript application factories and tool callbacks, with one start/stop and deployment, while retaining existing session/storage behavior | Strong choice if JavaScript authoring is a product requirement. Requires a private host/core bridge, packaged Go binary, shared admission policy, and coordinated failure/shutdown. A Node callback executes in Node, not inside Go. |
| **TypeScript runtime replacing the Go core** | One primary implementation language and direct access to the Pi/OpenCode ecosystems | Requires reimplementing and requalifying session, persistence, and API behavior. SDK syntax alone does not justify this rewrite. Codex/stdio MCP still need processes. |
| Configuration-generating SDK only | Least runtime change | Does not provide application-owned handlers or session binding; insufficient for the proposed programming model. |

Selected the Go application SDK with the existing Go engine. The TypeScript host and rewrite remain documented alternatives, not parallel implementation commitments.

### D2: Custom functions supplied by a remote client

| Variant | Execution and disconnection behavior | Recommendation |
| --- | --- | --- |
| **Deployed application function or remote service reference** | Brain calls installed application code, Lambda, or another service; the initiating client can disappear | Default for server sessions. Move `myTool.run` into the application or deploy it independently. |
| **Explicit connected tool executor** | A backend, desktop, or CLI process registers handlers and serves calls over an authenticated live channel; functions remain in that process | Strong alternative when caller-local functions are a requirement. The SDK must make executor lifetime explicit. |
| **Upload/build/deploy the function into a sandbox** | A deployment system packages code and dependencies and returns a callable reference | Useful for untrusted customer code, but adds a code-hosting/build product. Keep it an external deployment integration rather than pretending session JSON contains a closure. |

If a connected executor is selected, it needs scoped registration, call/result correlation, one authorized executor for a binding, bounded queues/deadlines, cancellation, and rejection of late or duplicate results. Loss after dispatch yields an unknown-outcome tool error; reconnect must not replay the call. An event observer is not an executor and cannot submit results. After restore, the binding must reconnect before it can serve new calls; there is no checkpoint of a function's closure or caller memory. This is additional tool transport work, not existing SSE behavior.

### D3: Credential custody

**Selected: server-default tool credentials, with optional authorized session overrides.** How overrides are supplied and retained remains open. References to application-managed credentials, explicitly transient client-supplied secrets, and Brain-managed persistent connections are different custody options, not competing default/override models. Recommend references for persisted state; choose Brain-managed custody only if built-in credential onboarding is part of the product. Caller-held secrets can instead stay with a connected executor, with its availability tradeoff.

### D4: Harness tool ownership

**Recommend one Brain-owned dispatcher and the strict adapter baseline.** The alternative is a separately named sandbox-native profile with harness-owned local tools and possibly native MCP. That trades uniform control and recovery for upstream fidelity and earlier integration. Decide explicitly rather than weakening the guarantee to get a constructor working.

## Other alternatives considered

- **Require MCP for every tool.** This unifies the external protocol and improves reuse with other MCP clients, but makes every local function depend on an MCP host/adapter and does not solve authorization or code deployment. Keep MCP as one provider; an optional MCP exporter can be considered when there is a consumer.
- **Retain only independently deployed HTTP/SSE/MCP services.** This preserves the strongest operational separation and is still a supported topology. It does not meet the goal of composing a complete application with ordinary trusted functions unless the application SDK also owns the tool-service lifecycle.
- **Adopt a larger agent server as Brain's core.** [LangSmith Agent Server][langsmith-server] demonstrates deployed code plus configuration/factories, but brings assistants, threads, runs, cron jobs, and database/queue ownership. That is a different public model and persistence design, not a shortcut to exposing Brain's composition root.
- **Let every harness define the public tool API.** This preserves native features but makes the same session request behave differently by harness. Use typed, validated capability-specific options rather than raw upstream configuration passthrough.

The [OpenAI Agents SDK][agents-tools] is useful prior art for authoring local function tools separately from hosted tools and MCP. Its in-process runner does not make a remote client's function serializable, and adopting its runner would replace, rather than merely configure, Brain's harness ownership.

## Consequences and acceptance criteria

If accepted, this ADR amends ADR 0001's external-service-only tool rule and ADR 0002's repository-internal-only extension seams. It extends the tool policy path to session bindings and changes successful output-schema semantics. It does not authorize automatic inference, automatic action retries, raw code uploads, cross-harness state conversion, or a platform scheduler.

The application SDK adds a supported composition surface and lifecycle responsibility. Session bindings add persisted descriptors and credential/resource authorization. Co-packaged handlers reduce separate deployment work but share more of the application's failure domain. All provider catalogs remain optional dependencies; importing the client or using one provider must not install every harness and cloud SDK.

Before implementation is considered complete, demonstrate:

1. A separately built consumer application can compose and start Brain without importing `internal/`, and stop it without leaving owned processes or pending executions behind.
2. The same provider binds independently to two tenants and to two resources in one session, with no credential, schema, name, or mutable-state crossover; invalid requests create no partial session.
3. Function, HTTP/SSE, and MCP tools pass the same authorization, input/output, cancellation, progress, and unknown-outcome cases. Provider SDKs do not replay actions.
4. MCP catalog changes, unavailable selected tools, name collisions, credential revocation, disallowed endpoints, and stdio process loss have explicit outcomes without silent tool-set changes.
5. Every advertised harness exposes the exact bound tools and preserves the next model request after compaction and cold restore, without startup model/tool calls or whole-history checkpoint rewrites.
6. Credentials are excluded from persisted sessions, archives, events, logs, and model input. Per-session overrides cannot mutate server defaults, cross bindings, or fall back to a default after authorization failure. If connected executors or transient secrets are selected, disconnect/restart behavior matches the declared availability contract.

These are future acceptance gates, not test results from this design work. Acceptance of the ADR is separate from implementing or advertising its SDKs and integrations.

## Research sources

Primary documentation reviewed on 2026-10-16; moving upstream documentation establishes an integration surface, not qualification of an unpinned release.

| Source | Design evidence |
| --- | --- |
| [Pi SDK][pi-sdk] and [CLI/RPC integration][pi-rpc] | Embedded versus subprocess control, custom tools, native context, resource control |
| [Codex SDKs][codex-sdk], [App Server][codex-app-server], and [upstream dynamic-tool tests][codex-dynamic-tests] | CLI/SDK versus bidirectional App Server control and external dynamic-tool callback integration |
| [OpenCode SDK][opencode-sdk], [network client][opencode-client], and [plugins][opencode-plugins] | Embedded versus remote host lifecycle, tool transforms, cancellation, stable request tool snapshots |
| [MCP tools][mcp-tools], [transports][mcp-transports], and [stdio][mcp-stdio] | Discovery, namespacing, structured results, request-scoped transport, process ownership |
| [MCP authorization][mcp-auth] and [security guidance][mcp-security] | Resource-scoped credentials, token audience, proxy and endpoint trust |
| [OpenAI Agents SDK tools][agents-tools] | Local function versus hosted/MCP execution; nested Codex tool versus session harness |
| [Supabase API keys][supabase-keys] | Application credentials versus user identity; secret-key RLS bypass |
| [LangSmith Agent Server][langsmith-server] | Deployed code/factories with remote configuration, and the cost of a larger server model |

[pi-sdk]: https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/sdk.md
[pi-rpc]: https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/cli-integration.md
[codex-sdk]: https://learn.chatgpt.com/docs/codex-sdk
[codex-typescript]: https://github.com/openai/codex/blob/main/sdk/typescript/README.md
[codex-app-server]: https://learn.chatgpt.com/docs/app-server
[codex-dynamic-tests]: https://github.com/openai/codex/blob/beae473be8564d884a8cfe9704194a5c803d2ac1/codex-rs/app-server/tests/suite/v2/dynamic_tools.rs
[codex-dynamic-protocol]: https://github.com/openai/codex/blob/96eb400d1231f0199e96f640c740096087cf3e9b/codex-rs/app-server-protocol/src/protocol/common.rs
[opencode-sdk]: https://opencode.ai/v2/docs/build/sdk/
[opencode-client]: https://opencode.ai/v2/docs/build/client/
[opencode-plugins]: https://opencode.ai/v2/docs/build/plugins/
[mcp-tools]: https://modelcontextprotocol.io/specification/2026-07-28/server/tools
[mcp-transports]: https://modelcontextprotocol.io/specification/2026-07-28/basic/transports
[mcp-stdio]: https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio
[mcp-auth]: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization
[mcp-security]: https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/security_best_practices
[agents-tools]: https://openai.github.io/openai-agents-js/guides/tools/
[supabase-keys]: https://supabase.com/docs/guides/getting-started/api-keys
[langsmith-server]: https://docs.langchain.com/langsmith/agent-server
