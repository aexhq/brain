# ADR 0003: Application SDK, session bindings, and harness integration

- **Status:** Proposed. Go application SDK, managed and client-runtime tools, direct HTTP/reverse SSE plus HTTP/RPC bindings, bounded shared local harness hosts, and external-only arbitrary code execution selected. Hosts may contain different tenants, subject to session-isolation qualification. Server-default credentials and authorized session overrides must survive normal server restarts; encrypted credential storage in the session SQL database is selected, with external vault integration available through application providers. See [Decisions to finalize](#decisions-to-finalize).
- **Date:** 2026-10-16
- **Extends, if accepted:** [ADR 0001](0001-local-first-session-server.md) and [ADR 0002](0002-subsystem-contracts-and-sdk-delivery.md).
- **Scope:** Architecture and SDK semantics, not implementation, package names, or a final wire schema.

## Proposal

Make Brain an application developers compose through an application SDK. The application installs harness adapters, managed tools, and shared tool-transport building blocks. Clients create sessions by selecting managed tools and declaring runtime tools. Managed implementations belong to the Brain application; runtime functions remain in the client process, behind an SDK-owned communication bridge. Brain receives the runtime tool's descriptor and execution binding at session creation, not its implementation.

**All tools share one descriptor, invocation, progress, and result contract.** Registration and execution location differ; authorization, schemas, limits, and event accounting do not. HTTP, SSE, RPC, and MCP adapters are building blocks, not separate application-level tool models. Managed tools can use the same building blocks as runtime tools.

**One application means one owned deployment and lifecycle, not necessarily one process.** Brain supervises a bounded local fleet of shared harness hosts on its PC or VM. Sharing may cross tenant boundaries, but grants no access between sessions. Harnesses own inference and native context; Brain owns public session identity, admission, host lifecycle, tool dispatch, and recovery. No arbitrary model- or client-supplied code executes on the Brain or harness hosts. Explicitly authorized external sandbox services provide code execution.

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
| Harness residency | [`ProcessFactory`](../../internal/harness/process.go) starts child runners; [`Engine`](../../internal/session/engine.go) loads them lazily, limits residency, evicts idle runners, and restores native state. | Extend these responsibilities into an explicit local host-supervision contract. Process-tree cleanup and multi-harness recovery need qualification; a PID and a working directory alone do not establish isolation. |
| Provider integrations | [`internal/toolservice`](../../internal/toolservice/service.go) and [`services/sandbox`](../../services/sandbox/README.md) contain storage, web, and sandbox services. | Reuse their execution and isolation rules when packaging providers. Do not replace tested provider behavior with new wrappers solely for naming consistency. |

The [registry policy tests](../../internal/tools/registry_test.go), [MCP transport tests](../../internal/tools/mcp_test.go), and [Pi restore tests](../../internal/session/pi_restore_test.go) are existing behavioral evidence. The [local architecture spikes](../research/application-sdk-spikes.md) add startup, residency, and synthetic-turn measurements, not adapter qualification or live-provider results. Integration maturity remains on the [roadmap](../roadmap.md).

## Application and client SDKs have different authority

| Surface | What the developer supplies | What it may do |
| --- | --- | --- |
| Application SDK | Trusted executable factories, tool handlers, authorization and credential resolvers, persistence, limits, and server settings | Install code, open resources, build the server, start it, and drain/close what it owns |
| Harness integration package | An adapter factory and validated adapter-specific settings | Run the native harness and translate its events, tool callbacks, cancellation, and checkpoints |
| Tool integration package | Tool definitions, providers, and shared transport adapters | Run in the declaring execution environment, access configured resources, and enforce resource-specific authorization, approvals, quotas, and cleanup |
| Client SDK | Harness selection, managed-tool references, runtime tool declarations, binding parameters, and credential selection or permitted private overrides | Create authorized sessions, keep runtime functions local, and register/serve their calls through an SDK-managed bridge |

The application SDK is the composition root, not a second agent loop. The stock Brain executable becomes one application built with that SDK. Configuration-file startup and code-based startup must use the same validation and runtime; they are not separate implementations.

Expose the small public contracts needed for composition, not every internal subsystem. Application dependencies are installed and pinned at build/deployment time. Session creation never downloads packages, evaluates source, or accepts a subprocess command.

Keep the generated HTTP contract and client wire types authoritative as in ADR 0002. Handwritten SDK constructors produce that contract; they do not become a competing source of protocol types. Preserve the published clients' release rules without introducing versioned routes or runtime compatibility negotiation.

### Managed and runtime tools

| Kind | Registration | Client-side value | Execution |
| --- | --- | --- | --- |
| Managed tool | Installed by the application at server startup | Reference plus permitted parameters and optional credentials | Brain-owned implementation, either a local trusted handler or an adapter calling another service |
| Runtime tool | Declared for a session through the client SDK; unknown to Brain beforehand | Ordinary tool declaration with input, output, and `run` | The declaring client process; the SDK bridges invocation, progress, cancellation, and results |

This distinction is not local versus HTTP. A managed tool can use HTTP/SSE/MCP, and a runtime function can use the same transport helpers in its own process. The normalized executor contract hides placement from the harness. A client reference to a managed tool does not contain or execute that tool's implementation.

### Illustrative application

The following is pseudocode. Names distinguish executable factories from remote references; they do not select a programming language or commit to signatures.

```text
server = BrainServer(
  agentloops = {
    pi: PiRuntime(maxShared = piSessionLimit, maxActiveTurns = piTurnLimit, ...),
    codex: CodexRuntime(maxShared = codexSessionLimit, maxActiveTurns = codexTurnLimit, ...),
    opencode: OpenCodeRuntime(maxShared = openCodeSessionLimit, maxActiveTurns = openCodeTurnLimit, ...)
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
localLookup = Tool(
  name = "localLookup",
  input = ...,
  output = ...,
  run = input -> localApplication.lookup(input)
)
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
    MCPTools("crm", alias = "crm", select = ["search"]),
    localLookup
  ]
)
```

`CodexRef` selects an installed adapter; it does not run Codex in the caller. `SupabaseBinding` configures an installed provider; importing it does not install server code. A `MyLambda(...)` client helper can produce an authorized remote reference, but importing a function from a local source file does not deploy it to Lambda.

Omitting `credential` selects the tool's authorized server default. `SessionCredential(...)` illustrates an optional private override, not a model argument. The helper may accept a direct value or an authorized reference. A direct managed-tool override is encrypted and persisted before session creation succeeds; closing the browser or restarting Brain does not require the user to supply it again.

The SDK accepts `localLookup` as an ordinary tool declaration. It retains `run` and its captured objects in the client, establishes the execution binding, and sends only the descriptor and routing metadata to Brain. The user does not write an HTTP handler, operate a separate worker service, or upload the function.

### What crosses the network

A **closure** is a function together with values it captures from the code around it. For example, a `run` function that calls a locally created database client depends on that client and its credentials. Those objects live in the process that created them.

The **session JSON** is the network request produced by the client SDK, not a JSON file the application developer must write. It can carry a tool identifier, configuration, and an authorized private credential input. It does not automatically transfer executable functions, database connections, or caller memory to the Brain server.

The selected SDK design hides registration and transport details while making lifetime explicit: runtime functions remain available only while an authorized client execution binding can serve them. Their input/output schemas are known to Brain after admission; their implementation and captured memory remain unknown. Server execution requires an application-installed implementation, not transfer of the client's closure.

## Tool definitions, providers, and session bindings

Keep three concepts distinct:

| Concept | Contents | Lifetime |
| --- | --- | --- |
| Tool definition | Model-facing name, description, input schema, optional output schema, and a handler in its declaring environment | Application registration for managed tools; client registration for runtime tools |
| Tool provider | Factory, binding-parameter schema, credential requirements, resource policy, and discovery/execution lifecycle | Groups tools and composes transport building blocks; is not a separate model-visible privilege |
| Session binding | Managed/runtime source, execution target, alias, selected operations, non-secret configuration and credential-selection metadata, and resolved descriptors | Descriptors and logical identity persist; private credential records persist separately in encrypted form. Live connections, functions, process handles, and plaintext secrets are not session configuration |

A single function is a provider with one tool and no configurable resource. S3, Supabase, a sandbox, and an MCP server can each supply several tools. The model sees the selected operations, not a generic privileged `runAnything` method.

Separate **binding parameters** from **model arguments**. A bucket, project, credential reference, sandbox limit, or approved endpoint is bound by application policy. It is not an argument the model can replace. If an operation needs a model-selected resource, its handler must authorize that resource within the binding.

### Binding and selection

Session admission authenticates the caller, resolves managed providers and runtime registrations, validates parameters and schemas, authorizes credential selection and registration scope, and resolves the exact descriptors and execution bindings. Only then does it persist the session. The SDK handles runtime registration as part of session creation; a failed creation releases its provisional registrations. Discovery and bridge establishment do not invoke a user function, start inference, or provision a sandbox.

The session's tool list is an explicit selection, not the union of every server registration and client request. Application defaults apply only when omission is explicitly defined; an explicit empty list means no tools. A request that exceeds application policy is rejected rather than silently widened or narrowed. Two bindings of the same provider, such as two S3 buckets, require distinct aliases.

Resolve model-visible names deterministically from the binding alias and operation. Keep the reverse mapping to the original provider/MCP name. Validate uniqueness and harness naming constraints before accepting the binding; do not rely on lossy name sanitization or an MCP server's self-reported name for uniqueness.

Persist the selected descriptors, their order, logical execution-binding identity, and non-secret credential-selection metadata. Restoring or redeploying must not silently reinterpret a tool name against a changed catalog. Managed tools require the matching deployed implementation and descriptor identity. Runtime tools require an authorized client to rebind the saved registration and matching descriptor; Brain cannot verify an unknown client's implementation merely from its schema. Unavailable execution bindings fail explicitly instead of being removed or replaced.

This ADR covers binding at session creation. Idle-boundary configuration changes remain separate work in [plan 05](../plans/05-api-and-sdks.md#future-configuration-updates-proposed). Revocation remains effective at call time even when a descriptor is unchanged.

### Provider lifecycle and resource ownership

The application owns managed connections and harness process lifecycles; the client SDK owns runtime callback registrations and bridge connections. Bindings own session-scoped execution context. A handler receives trusted tenant, session, and call identity, a cancellation signal, and a progress emitter separately from model arguments.

Do not store mutable per-session credentials in a shared client's global headers or environment. Shared network pools are acceptable; sharing authorization state between bindings is not.

Binding, opening, and restoring must not recreate paid resources or replay actions. A sandbox is created by an explicit tool action; subsequent calls use a scoped provider handle. Session checkpoints can preserve that handle, but do not checkpoint the external machine or promise it still exists. Expired handles produce tool errors. The provider/application owns expiry and cleanup, including after a Brain crash; a shutdown callback alone is insufficient.

S3/R2 **agent tools** and S3/R2 **checkpoint stores** are different roles, with separate configuration and credentials. Supabase data-access tools and a PostgreSQL session index are likewise distinct. A vendor name is not a reason to merge these contracts.

`FlyMachine`, `ModalSandbox`, and other compute integrations are tool providers, not new Brain environment or job resources. Their operations need not be interchangeable. A provider package may wrap a vendor SDK or an existing MCP server; choose from the required operations, authorization model, and qualification evidence rather than requiring one integration method for every vendor.

### Optional read-only sessions tool

A `sessions` tool is an optional managed integration over Brain's authorized session reads, not an implicit harness capability or a new core session authority. Recommend an official provider package that a self-hosted Brain application or a hosted platform can install. The application chooses its exposure and read policy; a platform supplies fleet-wide discovery and routing when sessions live on several Brain instances. The core continues to own session authorization, bounded reads, and passive access to stored history.

"The same client" means the same authenticated tenant/account, not a browser connection, SDK instance, or shared native process. A source session receives cross-session read access only through an explicitly selected tool binding. The provider derives the tenant and source session from trusted invocation metadata and authorizes each target within that tenant and the binding's permitted scope. A model-supplied target session ID is a lookup argument, not authorization. Sessions in another tenant remain inaccessible even when they share the host.

Limit this provider to selected read operations, such as session metadata and bounded event history. It does not expose native state files, credential records, control handles, or write operations such as sending input, interrupting, or resuming another session. Reading another session never starts its model or tools. Returned history is tool-result data, not authority to alter either session's configuration.

The ordinary call-scoped tool credential remains read-only for its source session; do not silently broaden it to the tenant. The installed provider needs an explicit authorized read capability for its permitted targets, kept outside model arguments and the harness. A local provider can use the application's authorized read interface; an external or platform provider can use a tenant-scoped read client. Neither reads the native host's shared database directly or receives blanket management authority merely because it is an official tool.

## One tool-call boundary, independent of execution location

The call path is:

```text
harness callback
  -> Brain session membership and call identity
  -> authorization, argument schema, deadline, and output policy
  -> local handler OR HTTP/SSE/RPC/MCP execution binding
  -> normalized tool result and event
  -> harness continuation
```

The application can package trusted functions with Brain without making the session coordinator responsible for S3, SQL, sandbox, or approval logic. Independent services remain available for a different language, isolation, or separate scaling. No mandatory separately deployed tool gateway is introduced.

An in-process managed handler is trusted application code, not permission to evaluate model/client-supplied programs on the host. It shares the application's failure domain. A deadline bounds how long Brain waits, not whether an arbitrary client function or external service has stopped. Handlers should honor cancellation; model-directed code execution belongs in an external sandbox service. A local subprocess or container execution tool is not an allowed substitute.

| Failure | Required behavior |
| --- | --- |
| Invalid session binding, forbidden resource, unsupported harness option | Reject admission; do not create a partly valid session |
| Tool denial, provider failure, cancellation, or malformed tool output | Return a model-visible tool error, preserving whether an action may have happened |
| Tool-transport or client-executor loss after dispatch without a definitive outcome | Mark the outcome unknown; never automatically resend the action |
| Harness protocol failure, core persistence failure, or broken coordinator invariant | Stop the affected execution as a Brain error |
| Invalid application registration or failure opening a required startup resource | Fail startup and close already-opened owned resources |

Provider SDK retry defaults must not silently reintroduce action retries. Reopening a connection or restarting a process for a future call is different from replaying the previous call. Restoring history and reconnecting event observers never execute tools.

If a runtime callback's browser/backend process or an external tool service disappears, return a tool failure to the harness. Brain does not keep that execution environment alive or reconstruct its function. The model may request another call in response; that is a new logical call, not an automatic transport retry. An unknown-outcome error must state that an action may already have happened, because another call can duplicate its effects. Configurable automatic retry policies are deferred unless a demonstrated use case requires a separate design for retry conditions and idempotency.

### SDK-owned runtime bridge

The developer supplies the normal tool declaration. The SDK owns the callback registry, authenticated connection, request/result correlation, progress, and cancellation plumbing. Brain dispatches through the same executor boundary as for managed tools; it does not load, inspect, or evaluate the client implementation.

Select three execution bindings built on the shared tool contract:

| Binding | Communication and deployment |
| --- | --- |
| Direct HTTP | Brain calls a reachable service; the response carries JSON or streamed SSE progress/results. Fits deployed backend and Lambda tools. |
| Reverse SSE plus HTTP | The executor opens an outbound SSE command stream and POSTs progress/results back over HTTP. Fits clients that cannot accept inbound connections; SSE alone is one-way. |
| Bidirectional RPC | An authenticated channel carries correlated calls, progress, cancellation, and results. Fits attached client executors. The carrier and framing remain wire-contract work. |

The application and SDK select the binding from the deployment and supported transports, without changing tool authoring or policy. All three are selected surfaces, not an automatic fallback chain; switching transports must never replay an ambiguously dispatched action. The [transport spikes](../research/application-sdk-spikes.md#tool-transport-experiment) used WebSocket for the bidirectional prototype, not a qualified generic RPC stack. They inform implementation choices but select neither gRPC nor a universal WebSocket requirement. They do not measure the future Go server or actual browsers.

The execution channel is distinct from the replayable session event stream. Replaying a `tool.call` event never invokes a callback. Reading a session grants no right to execute its tools or submit results. Each execution registration is scoped to its authenticated tenant, session, and tool binding; only the current authorized connection can answer its outstanding calls. Queues, outstanding calls, and payloads are bounded.

At session creation, the SDK establishes a usable route without executing the function. During a call, Brain assigns identity and validates arguments, the SDK invokes the local handler, and Brain validates the returned result before recording it and letting the harness continue. Cancellation is delivered to the handler's signal; it does not promise rollback or forced termination of caller code.

An unavailable bridge before dispatch produces a tool-unavailable error with no action dispatched. Loss after dispatch without a definitive result produces an unknown-outcome tool error. Reconnect must not re-execute the call; late, duplicate, or superseded results cannot complete another call. The same live SDK can rebind its retained handlers after reconnect. A restarted client must redeclare and rebind them; neither Brain nor a checkpoint reconstructs its closure.

SDK shutdown and session closure release the relevant registrations. Ordinary observers may disconnect without affecting execution, but the process serving runtime callbacks must remain available for those tools. A bridge outage does not delete session history or silently remove tools from the model's catalog. The same limits and errors apply whether a transport primitive was used by a managed or runtime tool.

### Browser, backend, and serverless lifetimes

Choose an execution binding from the tool's deployment and lifetime, not from the language of the session-creating SDK.

| Use case | Execution binding | What happens when the initiating request or browser closes |
| --- | --- | --- |
| Web app using managed tools or deployed services | Serializable provider references; Brain dispatches independently of the browser | Work can continue using persisted credentials. The browser is an observer, not an executor. |
| Browser-local function using the DOM, a selected file, or other local state | SDK-owned outbound execution connection | The function is unavailable when its browser execution environment is gone. Return a tool error; do not migrate the closure. |
| Function registered by a long-lived backend | SDK-owned outbound connection, or an explicitly exposed HTTP service | An individual web request can finish while the backend serves tools. If the backend process stops, its callbacks become unavailable. |
| Deployed Lambda used as a tool | Authorized HTTP or Lambda invocation adapter targeting the deployed function | Brain invokes the function on demand. Its availability does not depend on the browser or the process that created the session. |
| Ordinary Lambda invocation used as a session client | Only callbacks served during that invocation's lifetime | Returning from the Lambda invocation does not leave a callback server running. Use a deployed reference for later work. |

A deployed service reference contains its descriptor and authorized routing/configuration, not executable client code. The application installs the transport adapter and endpoint policy. A helper such as `MyLambda(...)` can select an already-deployed function through that adapter; it does not upload a closure or require the browser to forward every invocation.

Browser-facing connections need origin controls and scoped authentication; do not place an application-wide management token in a browser bundle. The published browser-client gap remains on the roadmap. A backend proxy and a future browser execution bridge have different authentication and lifetime responsibilities.

For a Lambda tool, prefer a request/response invocation for an ordinary call/result contract. An asynchronous acceptance response is not the function's completed result. A deliberate job-style tool can return a handle, but that is a provider contract rather than an implicit Brain worker. Lambda's asynchronous invocation and event sources can retry independently; account for those semantics rather than assuming Brain's no-retry rule controls the platform. See [Lambda lifecycle][lambda-lifecycle] and [invocation retries][lambda-retries]. No cloud timing or concurrency guarantee follows from a loopback transport spike.

### Input, output, and progress

Use JSON Schema as the wire description, with language-specific typed authoring helpers. Preserve the distinction between successful structured output, model-readable content, progress, and an error.

The proposed `output` schema describes a tool's **successful structured payload**, not Brain's complete result/error envelope. This deliberately changes the present registry's envelope-schema interpretation. MCP's `outputSchema` has the structured-payload meaning, and custom `{ input, output, run }` authoring should mean the same thing. Errors do not need to satisfy a success schema.

Preserve MCP structured content rather than only converting it to text. An adapter can render structured data as JSON text for a harness that needs text, without losing the stored structured value. Content that a harness cannot represent must have an explicit conversion policy or fail; do not silently discard it. Resource links are data, not permission to fetch a URL or read a file.

Progress does not commit an action or authorize continuation. Final results remain bounded and tied to the authenticated call. Tool output cannot inject session/control events.

## MCP is a provider, not the architecture of the whole application

For MCP connections opened by Brain, Brain owns discovery, selected-tool mapping, credentials, invocation, cancellation, and normalization once, outside the individual harnesses. Pi custom tools, Codex dynamic-tool callbacks, and OpenCode tool transforms bridge into that same dispatcher.

A runtime function may itself use an MCP client in the caller's process, just as it may use an HTTP client. Brain authorizes and records the outer tool call; it cannot claim to observe or enforce every internal operation of an unknown client implementation. A client-side MCP helper can expose its selected tools through the ordinary runtime bridge without moving its code or credentials into Brain.

Use the official MCP SDK rather than reproducing the protocol. The [2026-07-28 transport specification][mcp-transports] is request-scoped: Streamable HTTP uses POST with JSON or a request-scoped SSE response, and this revision removes protocol-level sessions and the old standalone GET stream. Do not equate a Brain session, an SDK connection object, and an MCP protocol session. External protocol support does not imply versioning Brain's own API.

**Remote MCP:** A provider holds an endpoint policy, authentication resolver, and explicit tool selection. Discovery uses the binding's authorized credentials because catalogs can differ by authorization. Discovery caches must not cross those boundaries. Catalog changes affect new bindings, not the tool set in a running session. A stored descriptor cannot prove that a third-party server's implementation has stayed identical; incompatible rediscovery must fail rather than silently replace the contract.

**Stdio MCP:** The application installs the executable, supplies an explicit environment and working directory, and owns startup, cancellation, stderr handling, and shutdown. A remote caller cannot submit a command or trigger package installation. Default to session-scoped processes when credentials or mutable state are session-specific; sharing requires an explicit provider contract. Only trusted application-installed servers run on the Brain host, under the same no-arbitrary-execution policy as managed handlers. Untrusted servers and code-execution capabilities belong outside that host; a local MCP wrapper does not make them an exception.

Do not pass the Brain API token through as an upstream MCP credential. Use credentials intended for the upstream resource, and retain tenant/session/call identity as trusted metadata where the server supports it. Third-party MCP servers may ignore Brain-specific metadata; their own authentication and resource authorization must establish access. A metadata field is not an isolation guarantee.

Tools are the supported MCP capability in this proposal. Sampling, elicitation, roots, prompts, resources, and multi-round-trip input are separate decisions, not implicit effects of attaching a server. Preserve explicit failure for unsupported requests, no automatic inference, and provider-owned approvals. Adding interactive MCP later requires a defined user interaction and continuation contract; blindly retrying `tools/call` is not that contract.

Letting each harness independently connect to arbitrary MCP servers would duplicate authentication, discovery, policy, event accounting, and recovery behavior. It is not selected. An adapter may use MCP to reach Brain's dispatcher if needed, but that must not enable ambient MCP servers or bypass the registered tool set.

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

For either a managed or runtime tool, the harness asks Brain to dispatch the action:

```text
model -> harness -> Brain dispatcher -> configured tool implementation -> harness
```

For a native tool, the harness's own implementation executes the action:

```text
model -> harness's shell/file/MCP implementation -> harness
```

The first path is selected. The second is not permitted for native execution or ambient tools. Observing a native event is not the same as authorizing and dispatching an action. Supabase and external sandbox services remain available through registered tools, while inference and context remain inside the harness.

### Proposed baseline and qualification

Install adapters at application startup and select one for each session. Separate installation settings, such as executable paths and credentials, from validated session settings, such as model and reasoning effort. An arbitrary native configuration dictionary must not override isolation, MCP ownership, process commands, or recovery policy.

Require a baseline: harness-owned inference and context; exact Brain-bound tool access; cancellation; normalized events; no ambient credentials/resources; passive native restore; context preservation after compaction; and coherent checkpoints with writes proportional to changed data. Optional features, such as steering or native structured responses, have explicit capabilities. Unsupported requests fail instead of being ignored or falling back to another harness/model.

| Harness | Upstream integration surface | Evidence and remaining architectural issue |
| --- | --- | --- |
| Pi | [Embedded SDK][pi-sdk], custom tool callbacks, native `SessionManager` | Existing reference runner. Native session state, not reconstructed event text, owns the next prompt. |
| Codex | [App Server][codex-app-server], dynamic tools and callbacks | The original 0.163.0 probes exposed `request_user_input`. A later controlled probe removed it with `tools.experimental_request_user_input.enabled = false`, leaving exactly `echo`. The adapter must enforce the control and still qualify restore, compaction, and shared-session isolation. |
| OpenCode | [Embedded SDK][opencode-sdk] and [tool transforms][opencode-plugins] | The recorded 2.0.30 host resumed suspended sessions at startup; mutable SQLite state needs a coherent incremental checkpoint adapter. The later spike still advertised generated code-mode `execute` after filtering registered tools. Tool transforms alone do not prove exact tool exclusion. |

These probe findings are version-specific evidence from 2026-10-15 and 2026-10-16, not claims that every later release has the same behavior. Reading newer documentation does not replace rerunning qualification against a selected, pinned release.

Distinguish advertisement from invocation. The performance spikes did not invoke either extra tool. OpenCode's generated `execute` has a JavaScript Code Mode implementation with HTTP `fetch`; it is not established to be an inert descriptor. Codex's question handler is a different capability, with mode-dependent invocation guards and a separately verified registration switch. A native tool that returns an error when called can still violate the exact model-facing catalog contract. See the [follow-up evidence](../research/harness-probes.md#codex-catalog-exclusion-follow-up-2026-10-16).

Wrapping Codex as a tool inside Pi is a valid nested-agent feature, but it does not implement a Codex session harness. It introduces a second agent loop and separate context. The [Agents SDK's Codex tool][agents-tools] illustrates this distinction.

The sandbox-native alternative is not selected: retaining native shell, file, local container, or code-mode execution would violate the host boundary even if guarded by a read-only setting or approval prompt. An adapter that cannot exclude or explicitly bridge native capabilities does not meet the supported baseline. External sandbox tools are not native execution on the Brain host.

## Brain supervises a local fleet of harness hosts

A **harness host** is a Brain-owned process or process tree running a native harness behind a private control channel. Pi may be an SDK embedded in a wrapper; OpenCode and Codex may expose server-like interfaces. This difference is contained in the adapter. Their native session managers are not independent authorities for Brain admission, public session status, or automatic restart of work.

The selected fleet scope is local to each PC/VM running Brain. There is no remote host provisioning, cross-machine scheduler, or new public worker resource. Multi-instance routing, placement, and failover remain with the surrounding platform as in ADR 0001.

```text
Brain application
  session authority: identity, admission, events, checkpoints
  local host supervisor
    Pi hosts         -> bounded, isolated native sessions
    OpenCode hosts   -> bounded, isolated native sessions
    Codex hosts      -> bounded, isolated native sessions
  shared tool dispatcher
    managed implementation / HTTP / MCP
    SDK bridge -> client runtime function
    external sandbox service
```

### Identity, isolation, and residency

Select shared native hosts with application-configured per-harness occupancy and active-turn limits. A host may contain sessions from different tenants/accounts within the same Brain application. This placement choice grants no cross-session access. An unloaded session is persisted data, not a permanently running server. Adapters must qualify this shared-session contract before exposing it as supported.

| State | Owner and lifetime |
| --- | --- |
| Brain session ID, tenant, harness selection, configuration, descriptors, event history | Brain; persisted independently of process residency |
| Opaque native session identity and checkpoint state | Adapter/native harness; retained with the session's native state, not reconstructed from display events |
| Association between Brain session, native session, and active host | Supervisor; binds exactly one current host to the owning session |
| Process handle, private endpoint, host/session-attachment generations, active Brain/native turn association, pending requests | Local live state; never treated as portable checkpoint identity |

Give each host a private root, an explicit environment, and separate session working/state locations or a qualified native storage partition. Do not inherit the developer's ambient credentials, extensions, plugins, MCP configuration, or workspace discovery. Pool only compatible pinned adapters and immutable host settings. Settings that require different process-global credentials or mutable configuration cannot be implemented by switching globals between calls; use a separate compatible host profile or reject the unsupported configuration.

The illustrative `maxShared` parameter caps all resident native sessions in one host, including active and idle sessions. Reserve a slot before initializing or restoring a session, so concurrent opens cannot exceed the cap; release it only after detachment and call/channel cleanup complete. `maxActiveTurns` is a separate per-host cap, no greater than `maxShared`. Each session still has at most one active turn. Both limits are positive and finite; `maxShared = 1` is the one-session configuration, not a separate topology. Numeric defaults require qualification rather than being inferred from the eight-session spike.

Separate these limits from retained session metadata, total resident hosts, startup work, queued input, and outstanding tool calls. When a compatible host is full, use another or start one only within the host/startup budgets; otherwise queue within the admission bound or reject before execution. Do not silently exceed occupancy, evict active work, or let one session run concurrent turns to absorb a burst. Keep capacity available for interrupts and tool completions when admission is saturated.

Load native sessions lazily for admitted work and preserve the Brain/native association while resident. Detach an idle session only at a coherent checkpoint boundary and only if the adapter can do so without disturbing its peers. If the adapter requires host-wide quiescence, wait for that condition rather than interrupting other sessions for eviction. Detachment frees a slot; stopping an empty or wholly idle host frees a process. Neither deletes the persisted Brain sessions.

The [native host spikes](../research/application-sdk-spikes.md#native-harness-experiment) inform per-harness caps: shared Pi saved memory with similar synthetic throughput, dedicated Codex processes were much smaller, and eight shared OpenCode sessions lost throughput. These measurements do not establish cross-tenant isolation or justify equal occupancy and concurrency settings for every harness.

### Session isolation inside a shared host

Each native session retains its own context, instructions, tool catalog, pending calls, cancellation state, and private persistence. Session A cannot list, read, mutate, resume, or dispatch through session B merely because they share a host. This applies even within one tenant; the optional authorized `sessions` tool is an explicit read path, not permission to use native cross-session APIs.

| Boundary | Required behavior |
| --- | --- |
| Context and credentials | Keep session-specific model credentials, headers, compaction state, tool bindings, and context-bearing caches separate. Session-derived content remains session-scoped even within a tenant. Shared caches may contain non-sensitive common data or separately authorized provider data, never another session's private context. Never mutate process-wide credentials to serve a session. |
| Events, callbacks, and results | Resolve the current host/session-attachment generations, native session, turn, and call association before delivery. Session IDs or tenant fields in model arguments cannot select the dispatch context. Unknown, stale, or ambiguous associations fail rather than falling back to another active session. |
| Native control and resources | Keep control channels private to the supervisor. Expose no raw native session-list/read/control methods, host files, ambient plugins, or code execution to sessions. Do not attach to unrelated existing harness services. |
| Persistence and restore | Export only the selected session's state and authorized recovery material. A shared native SQLite database is not a portable checkpoint for one session. Restoring or evicting A must not expose, overwrite, or resume B; require a coherent session-scoped export/import contract. |
| Normal cancellation | Cancel the selected session and its calls without closing or changing other sessions. A host-level failure is handled separately and must not be disguised as successful session-local cancellation. |

These requirements address data and authority isolation under the trusted-adapter model. They do not provide separate address spaces, independent CPU/memory capacity, or protection from a compromised harness or malicious installed handler. A shared process crash can affect every resident session. An adapter without demonstrated isolation does not qualify for cross-tenant sharing; placement alone is not a security control.

### No arbitrary execution on Brain or harness hosts

Trust is limited to application-installed handlers and pinned harness/adapter code. The model and remote clients cannot choose host executables, install host packages/plugins, or submit programs/container images for local execution. Validated parameters for an external sandbox remain permitted. Disable native shell/file tools, local container APIs, code-interpreter/code-mode paths that execute generated programs, and ambient extensions. Normal native state reads/writes and model requests remain part of the trusted harness implementation.

Model-visible capabilities are exactly the session's registered tools. A managed handler may call a cloud SDK or an authorized external sandbox, but must not turn model arguments into local shell/interpreter/container execution. Runtime callbacks execute only in the client's execution environment; receiving their declarations does not grant execution on the Brain host.

Before admitting work to a host, establish that its adapter enforces the tool and configuration boundary. An unexpected native capability or unbound action is an execution failure, not permission to continue with weaker isolation. A read-only sandbox or a native approval UI is not proof that local execution is disabled.

Private directories and host separation reduce accidental state sharing and contain failures between hosts, not between sessions in one host. They are not a security sandbox against compromised native binaries or malicious application-installed handlers. Run with least OS privilege and scoped credentials. Stronger OS-enforced filesystem/network isolation would require separate qualification. This ADR does not claim hostile code can run safely on the host; it excludes that workload.

### Lifecycle and process ownership

The supervisor owns launch, initialization, readiness, cancellation, idle eviction, draining, and process-tree cleanup. Startup/readiness checks use protocol and state checks, never model prompts. Match responses, events, and callbacks to the current host/session-attachment generations and the active Brain/native turn association. Detaching and reattaching a session changes its attachment identity even when the host and native session ID are reused. Late work from an earlier attachment, turn, or replaced host cannot affect a later turn.

On interruption, request cancellation for the selected native session and cancel its pending tool work. If failed cancellation requires terminating a shared host, explicitly fail every affected active execution and record uncertain tool outcomes; do not silently restart peers. On shutdown, stop admission, drain or interrupt bounded work, close owned channels, and terminate remaining owned processes within the shutdown budget. Normal shutdown and abrupt Brain death must not leave a harness continuing inference independently. Verify process-tree behavior on each supported OS; closing one child PID alone is insufficient evidence. Windows [Job Objects][windows-job-objects] provide process-group lifetime controls, not a security sandbox.

Never adopt or kill a process based only on a persisted PID: PIDs can be reused. Ownership must follow the launched process handle and OS lifetime group, with instance identity for any reconnectable channel. After Brain restarts, create new host associations from session state rather than trusting stale endpoints or an unverified process registry.

Report host startup failures, resident counts, eviction, unexpected exits, and recovery failures through logs/metrics. Health, transport keepalives, and supervisor bookkeeping do not trigger inference. Replacing a failed process is not permission to resume its interrupted turn.

### Recovery and external effects

| Event | Required behavior |
| --- | --- |
| Idle session or host eviction | Retain each session's committed checkpoint and identity; detach only with the required session/host quiescence, and reload passively for later admitted work |
| Harness crash or broken host control channel | Stop every active execution on that host, cancel reachable tool work, record failures and uncertain outcomes, and discard its live associations without losing persisted session identity |
| Brain restart | Recover metadata and classify unfinished execution as interrupted; do not start native continuation, replay queued actions, or launch model work merely by reading/restoring a session |
| Cold restore on an empty PC/VM data directory | Fetch the session checkpoint and restore its context and bindings; attach it to a compatible host only for subsequent admitted execution, without restoring other tenants' state |
| Missing/corrupt native state or incompatible deployed adapter | Fail explicitly; never create an empty native session as a successful restore |
| Runtime client unavailable after restore | Preserve its tool descriptor; require authorized SDK rebinding for new calls and return explicit unavailability while disconnected |

The checkpoint boundary couples Brain's committed session/event state with the harness's coherent native state. The adapter supplies native checkpoint/export semantics; the supervisor controls when the host is quiescent. Do not copy a live SQLite file without its coherence requirements or rewrite a growing native database every turn. OpenCode's startup auto-resume and incremental export remain qualification gates, not behavior to inherit from its server.

Restoring state is separate from executing the next turn. Native startup recovery, synthetic continuation messages, and cache warming must remain disabled. Loss of the unsynced tail remains possible under ADR 0001; an external action may already have happened even if its result or later checkpoint was lost. Neither a process restart nor event replay retries that action or rolls it back. External services own their resource lifecycle and idempotency guarantees; Brain does not promise exactly-once effects.

## Credentials and untrusted configuration

For managed tools, supply default credentials when constructing the tool/provider in the Brain application, and allow an authorized session to supply its own credentials for that binding. Both defaults and accepted overrides must survive an ordinary server restart. Defaults can use encrypted application credential records or stable deployment/vault configuration. Session bindings reference a default rather than duplicating its secret into every session. Reusing the default and supplying an override use the same installed tool implementation.

This guarantee covers explicit credential bindings. Application-owned handlers remain responsible for reconstructing private state they keep outside those bindings; Brain cannot discover arbitrary credentials captured inside their implementations.

Credentials captured by a runtime function remain in the client with that function. Its descriptor and bridge registration do not serialize captured values. This differs from explicitly supplying an override to a managed tool that executes in Brain.

An override affects only its binding and never mutates the provider's shared default. Application policy authorizes use of both defaults and overrides. An invalid or revoked override fails rather than falling back to a possibly more privileged server credential. Tool credentials go only to the execution component that needs them, not the model or harness.

The selected default for direct per-session overrides is an encrypted private record in the authoritative session SQL database. In the accepted local-first architecture, that database is SQLite; the optional PostgreSQL discovery index does not become the credential authority. The SDK accepts the credential through a protected, authenticated ingress path. Public session configuration stores only credential-selection metadata and an opaque reference, not the value or ciphertext.

Persist the session, binding, and encrypted override coherently before acknowledging creation. Encryption or persistence failure rejects admission; it must not create a session that can execute only while a browser retains a key. Browser disconnection, native host eviction, and harness restart do not delete the credential. Session-retention and explicit credential-revocation policies govern its lifetime instead.

Encrypt credential payloads before writing them to SQL, including its WAL, rather than placing plaintext in the serialized public session object. Use a standard authenticated-encryption implementation, fresh nonces, a key identifier, and authenticated ownership metadata tied to the tenant and binding; session overrides also bind to their owning session. This addresses database/backup disclosure and copying ciphertext into a different authorized binding. It does not protect against a compromised running Brain process or a malicious trusted handler.

The application supplies a stable key provider independently of the database and session archive, such as deployment-managed key material or a KMS-backed provider. Never generate a replacement key at every startup, store the decrypting key beside its ciphertext in the same archive, or fall back to plaintext. Required key configuration is a startup requirement. Key loss, failed authentication of ciphertext, and unavailable credential storage must have explicit failure paths, not a fallback to a different credential.

Keep the error layers: core SQL/decryption failures stop the affected execution as Brain errors; credential revocation and external tool/vault denials or outages become tool failures when resolving a call. Failure to establish the required credential storage during startup stops startup. None of these paths dispatches an action using a different credential.

An external credential provider can resolve authorized references from a vault instead of storing those values in Brain's SQL database. Supabase Vault is one option: it stores authenticated encrypted secrets in PostgreSQL and exposes decryption through a privileged view. Its database permissions and bootstrap credentials remain the provider's responsibility; it is not automatically available because a Supabase data tool is installed. See [Supabase Vault][supabase-vault]. Persisted references must be authorized against the tenant and resource, not trusted merely because they are opaque.

Cold restore must recover the required credential material as well as the native context. A checkpoint containing a directly supplied override needs a private, authenticated encrypted credential record, with the corresponding key available independently on the restoring host. External references need their configured vault/resolver. Application-wide defaults remain application configuration, not exported secrets in every user's session archive. The unsynced-tail allowance still applies; a committed checkpoint must not describe a usable override while omitting the record required to recover it.

Rotation, revocation, deletion, and backup retention are part of the storage contract. Restoring an older session checkpoint must not overwrite newer credential or revocation state in an existing store. Deleting a live SQL row does not erase encrypted copies in retained archives. If old backups must be unable to use a revoked credential after a cold restore, upstream credential revocation or an independently maintained key/authorization revocation mechanism is required. Do not promise immediate erasure from immutable backups. Key-provider integration and these lifecycle procedures need qualification before shipping.

Plaintext credentials must stay out of descriptors, model arguments, session events, archives, and diagnostics. Ciphertext belongs only in private credential storage and authorized recovery material, not public read responses. This prevents framework serialization of secrets; it cannot make a trusted tool safe if the tool itself deliberately returns a secret. Provider code still owns what it emits.

Credential persistence and execution continuation are separate decisions. Closing an observing browser does not interrupt a healthy Brain execution that uses managed or deployed tools. Persisting a key does not keep a browser callback alive or authorize automatic continuation after a Brain crash; passive recovery remains the selected policy.

Per-session URLs are also authority, not harmless configuration. Allow them only through a registered provider's endpoint policy, with credential audience binding, redirect rules, and network restrictions appropriate to that provider. Otherwise a session creator could cause server-side requests to internal services or send credentials to a different host. These checks address new reachable failures introduced by client-supplied bindings.

For Supabase in particular, a secret/service-role key bypasses row-level security. Tenant metadata does not restore RLS. Prefer user-scoped authorization where appropriate, or enforce resource policy in the tool implementation. See [Supabase API keys][supabase-keys] and [MCP authorization][mcp-auth].

## Decisions to finalize

The discussion selected the application/runtime split, encrypted credential custody, external-only execution, bounded shared hosts including cross-tenant placement, and the three transport bindings below. The optional read-only sessions provider belongs to application integrations rather than the core loop. RPC framing/carrier, numeric host-limit defaults, and key-provider details remain design work; adapter isolation and recovery require qualification. The overall ADR remains Proposed.

### D1: Application SDK runtime and language

| Variant | Benefit | Cost / reason to choose it |
| --- | --- | --- |
| **Selected: public Go application SDK, retain Go core** | Smallest change to the existing composition root; trusted Go functions can run directly; harnesses remain language-independent subprocesses | JavaScript/Python handlers still need an explicit process/service host. Client SDKs remain independent of the application SDK's language. |
| **TypeScript application host supervising the Go core** | JavaScript application factories and tool callbacks, with one start/stop and deployment, while retaining existing session/storage behavior | Strong choice if JavaScript authoring is a product requirement. Requires a private host/core bridge, packaged Go binary, shared admission policy, and coordinated failure/shutdown. A Node callback executes in Node, not inside Go. |
| **TypeScript runtime replacing the Go core** | One primary implementation language and direct access to the Pi/OpenCode ecosystems | Requires reimplementing and requalifying session, persistence, and API behavior. SDK syntax alone does not justify this rewrite. Codex/stdio MCP still need processes. |
| Configuration-generating SDK only | Least runtime change | Does not provide application-owned handlers or session binding; insufficient for the proposed programming model. |

Selected the Go application SDK with the existing Go engine. The TypeScript host and rewrite remain documented alternatives, not parallel implementation commitments.

### D2: Managed and runtime tools

| Variant | Execution and disconnection behavior | Decision |
| --- | --- | --- |
| **Managed application implementation or service reference** | Brain owns the installed implementation/routing; the initiating client can disappear | Selected for server-wired tools. Client constructors are references and parameter builders. |
| **Runtime function with an SDK-owned bridge** | The function stays in its declaring client process, which serves calls through shared transport primitives | Selected as part of normal session creation. Users declare input, output, and `run`; the SDK owns registration and communication. |
| **Upload/build/deploy client functions** | A deployment system packages code and dependencies and returns a callable reference | Not required or selected for runtime tools. An external sandbox tool may execute explicitly supplied code, but session creation does not deploy closures. |

The bridge is selected, not an optional separately operated worker product. Its live channel is an implementation responsibility of the SDK and server transport. The authoring contract and error policy are the same as for managed tools.

### D3: Credential custody

**Selected: server-default managed-tool credentials and authorized session overrides that survive normal server restarts.** Persist direct overrides as authenticated encrypted private records in the session SQL database; allow application-managed external vault references as an alternative. Reject memory-only custody as the default user experience. Keep encryption keys independent of session storage and define encrypted checkpoint recovery, rotation, revocation, and retention. This adds credential storage responsibility, not an identity provider or OAuth onboarding product. Runtime functions still retain their captured credentials in their own execution environment.

### D4: Harness tool ownership

**Selected: Brain-owned dispatch and no arbitrary code execution on Brain/harness hosts.** Native execution and a local sandbox/container execution profile are not selected. Explicitly authorized external sandbox services remain supported as tools. Pin and qualify each adapter's exclusion controls instead of weakening this boundary for upstream convenience.

### D5: Harness fleet management

**Selected: bounded shared local harness hosts, including sessions from different tenants/accounts.** Brain owns their lifecycle and the Brain/native session associations. Expose per-harness `maxShared` occupancy and separate active-turn limits, with lazy loading and checkpointed idle eviction. Sharing grants no access to another session; require exact tool exclusion, separate context/credentials, correctly routed events/calls, session-local cancellation, and session-scoped checkpoint recovery. Host crashes remain shared failures. Cross-machine provisioning, placement, and leader election are outside this ADR.

### D6: Tool execution transports

**Selected: direct HTTP, reverse SSE plus HTTP, and bidirectional RPC.** All use the same tool contract and authorization. The SDK owns attached callback plumbing; reachable services need no reverse channel. The RPC carrier/framing is not selected by the WebSocket prototype, and no transport fallback may replay an action.

## Other alternatives considered

- **Require MCP for every tool.** This unifies the external protocol and improves reuse with other MCP clients, but makes every local function depend on an MCP host/adapter and does not solve authorization or code deployment. Keep MCP as one provider; an optional MCP exporter can be considered when there is a consumer.
- **Retain only independently deployed HTTP/SSE/MCP services.** This preserves the strongest operational separation and is still a supported topology. It does not meet the goal of composing a complete application with ordinary trusted functions unless the application SDK also owns the tool-service lifecycle.
- **Adopt a larger agent server as Brain's core.** [LangSmith Agent Server][langsmith-server] demonstrates deployed code plus configuration/factories, but brings assistants, threads, runs, cron jobs, and database/queue ownership. That is a different public model and persistence design, not a shortcut to exposing Brain's composition root.
- **Let every harness define the public tool API.** This preserves native features but makes the same session request behave differently by harness. Use typed, validated capability-specific options rather than raw upstream configuration passthrough.

The [OpenAI Agents SDK][agents-tools] is useful prior art for familiar local function declarations alongside hosted tools and MCP. Brain preserves that authoring shape through the runtime bridge rather than serializing functions or adopting another agent loop.

## Consequences and acceptance criteria

If accepted, this ADR amends ADR 0001's external-service-only tool rule and ADR 0002's repository-internal-only extension seams. It extends the tool policy path to managed/runtime session bindings, adds encrypted credential persistence and recovery, SDK-owned callback transport, and local host supervision, and changes successful output-schema semantics. It does not authorize automatic inference, automatic action retries, host execution of supplied code, closure uploads, cross-harness state conversion, or a distributed platform scheduler.

The application SDK adds a supported composition surface and lifecycle responsibility. Session bindings add persisted descriptors and credential/resource authorization. Co-packaged handlers reduce separate deployment work but share more of the application's failure domain. Shared native hosts reduce process overhead but require logical session isolation and enlarge the host failure radius. Runtime tools add a client availability dependency, not a requirement for users to implement a service. All provider catalogs remain optional dependencies; importing the client or using one provider must not install every harness and cloud SDK.

Before implementation is considered complete, demonstrate:

1. A separately built consumer application can compose and start Brain without importing `internal/`, and stop it without leaving owned processes or pending executions behind.
2. The same provider binds independently to two tenants and to two resources in one session, with no credential, schema, name, or mutable-state crossover; invalid requests create no partial session.
3. Managed functions, client-runtime functions, and HTTP/SSE/RPC/MCP adapters pass the same authorization, input/output, cancellation, progress, and unknown-outcome cases. Provider SDKs and bridge reconnection do not replay actions.
4. MCP catalog changes, unavailable selected tools, name collisions, credential revocation, disallowed endpoints, and stdio process loss have explicit outcomes without silent tool-set changes.
5. Every advertised harness exposes the exact bound tools and preserves the next model request after compaction and cold restore, without startup model/tool calls or whole-history checkpoint rewrites.
6. Plaintext credentials are excluded from SQL/WAL, archives, public sessions, events, logs, and model input. Direct managed-tool overrides survive browser disconnection, normal server restart, and authorized cold restore using encrypted private records and independently available keys. Missing keys, tampering, revocation, rotation, and retained older backups have explicit outcomes. Overrides cannot mutate server defaults, cross bindings, or fall back to a default after authorization failure. Runtime callback registration transfers no function source or captured credentials.
7. An ordinary session-creation tool declaration executes in the client without a user-written server; failed registration, bridge loss before/after dispatch, cancellation, duplicate/stale results, and rebinding after restore have the declared outcomes. An event observer cannot execute callbacks or submit results.
8. Local host limits, idle eviction, Brain/native identity, private state, generation checks, abrupt parent/child death, and process-tree cleanup pass on supported PCs/VMs. A restart never resumes inference or executes queued actions automatically, and native execution tools cannot bypass the external-only boundary.
9. Concurrent opens/restores obey `maxShared` and active-turn caps. Sessions sharing one host, both within and across tenants, have no context, credential, cache, event, callback, cancellation, or checkpoint crossover. Exercise stale/misattributed identities, session eviction/restore while peers exist, and a host crash with several active sessions; peers never continue or replay actions automatically.
10. An optional read-only sessions provider uses an explicit binding and authorized target scope, not an expanded ordinary tool token or native host access. Same-tenant permitted reads succeed; unbound, forbidden-target, and cross-tenant reads fail, including targets on the same host. Reading performs no inference, tool action, or session mutation, and exposes no private credentials or native checkpoint files.

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
| [Supabase Vault][supabase-vault] | Authenticated encrypted SQL storage, private decryption access, and external credential-provider integration |
| [Lambda lifecycle][lambda-lifecycle] and [invocation retries][lambda-retries] | Invocation-scoped clients versus deployed tool endpoints, suspension, and platform-owned retry behavior |
| [LangSmith Agent Server][langsmith-server] | Deployed code/factories with remote configuration, and the cost of a larger server model |
| [Windows Job Objects][windows-job-objects] | Owned process-group lifecycle and limits; distinct from a security sandbox |

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
[supabase-vault]: https://supabase.com/docs/guides/database/vault
[lambda-lifecycle]: https://docs.aws.amazon.com/lambda/latest/dg/lambda-runtime-environment.html
[lambda-retries]: https://docs.aws.amazon.com/lambda/latest/dg/invocation-retries.html
[langsmith-server]: https://docs.langchain.com/langsmith/agent-server
[windows-job-objects]: https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
