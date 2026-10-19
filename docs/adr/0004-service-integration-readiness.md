# ADR 0004 Service integration readiness

- **Status:** Accepted for implementation on 2026-10-19; implementation pending.
- **Extends:** [ADR 0003](0003-application-sdk-and-session-bindings.md).
- **Baseline:** Brain `850f90b1`. Pi packages are pinned to `1.2.0`; existing feasibility probes cover Codex `0.163.0` and OpenCode `2.0.30`.

## Decision

Before other services adopt the new integration contract, support Pi, Codex and OpenCode; accept model and tool credentials at server startup and from authorized clients; provide both attached-tab and remote-browser integrations; and persist usage evidence suitable for platform accounting. Keep unversioned routes and make a clean contract and storage break. Platforms such as Aex own session inspection tools, discovery and product policy.

Keep shared native harness servers with their existing occupancy and active-turn caps. A common host-pool abstraction manages those servers across harnesses. Additional pool-wide utilization limits are optional and impose no new restrictions when omitted. This is local lifecycle composition, not a requirement for containers per session, a goroutine pool, or a distributed scheduler.

The SDK authoring surface uses models, tools and their configuration. Binding records, executor registration and command frames are lower-level routing details. Preserve explicit execution lifetime: a tool declared in a client needs that client to remain connected.

These requirements change the target contract; they do not claim the baseline implements it. [Plan 12](../plans/12-service-integration-readiness.md) owns the remaining work. Existing deletion, passive recovery, credential protection and authorization requirements still apply.

## Shared harness servers and host pooling

### Ownership and isolation

A **host** is one supervised native Pi, Codex or OpenCode server process and its descendants. A **session attachment** associates a Brain session with native state inside a host. A **host pool** acquires, tracks, drains and closes hosts through the installed harness factories.

Multiple sessions may share a host, including different tenants, only where the adapter preserves independent context, credentials, exact tool catalogs, event/call routing, cancellation and checkpoints. The pool does not manufacture that isolation. If a native authentication or registry setting is process-global, the adapter must use separate hosts for incompatible sessions rather than mutate that setting while peers run. Never use raw secrets as pool keys or labels. A host crash affects its attached sessions and never authorizes automatic continuation or tool replay.

Private directories and logical session routing do not sandbox a compromised native process. Arbitrary customer code remains in external tool services. Operators choose an OS/container boundary when their deployment requires filesystem, network or security isolation beyond the trusted installed adapters.

### Go and operating system mechanisms

[Go's `os/exec`](https://pkg.go.dev/os/exec) supplies process startup, environment, pipes, waiting and cancellation. It does not provide a portable CPU/memory sandbox. Context cancellation also does not establish a descendant-process lifetime policy; retain Brain's existing process-tree ownership code.

For optional hard limits, [Linux cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html) provides CPU, memory and process-count controls. [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) provide process-tree limits and accounting. Brain already uses a Windows job for lifetime cleanup, but does not configure its CPU or memory ceilings. Linux's existing process-group guardian is likewise lifetime control, not a resource quota. OCI/container runtime integration can be supplied at the application/deployment boundary; do not build a container runtime into Brain.

Factor the existing host acquisition/supervision code into one small internal owner shared by harness factories. Expose only the application-level options needed to install it. No generic pool dependency or public worker-management HTTP resource is needed. Keep the default implementation simple and add another launcher only when a supported deployment needs it.

| Control | Scope and default | Reachable failure and action |
| --- | --- | --- |
| Existing occupancy and active-turn caps | Per native host; preserve the selected adapter's settings | Concurrent opens/turns exceed configured capacity: reserve before attachment/dispatch and reject excess work explicitly. |
| Additional host/start/turn ceilings | Optional aggregate settings across harness servers; absent means no additional pool limit | A deployment needs a utilization budget: refuse excess acquisition/admission without silently changing harness or queuing unbounded work. |
| Idle eviction and drain | Reuse current lazy loading, checkpoint and shutdown behavior | Resident idle state or rollout needs a host released: detach settled sessions, preserve native checkpoints and close the owned host. |
| OS CPU/memory/process limits | Optional host-launch policy; off by default | A selected deployment requires a hard ceiling: apply before accepting work, fail if unavailable, and classify host termination for every affected attachment. |

A shared process has a shared memory/CPU enforcement boundary. Neither per-session concurrency caps nor a pool-wide cap promises per-session RSS isolation. Operators needing independently enforced budgets must select separate host groups or an external isolation boundary. Fixed numeric budgets, fairness algorithms, prewarming and feedback-based utilization control are not selected by this ADR; measure first.

## Harness support

| Harness | Integration path | Work before service integration |
| --- | --- | --- |
| Pi | Existing Node runner and shared native host | Keep exact-tool and passive-restore behavior; extend model credentials and usage observation to every enabled model-request path. |
| Codex | App Server over stdio, native threads/turns and dynamic tool callbacks | Finish native checkpoint/compaction restore, credential scope, request accounting and shared-host qualification. Use the pinned tool-exclusion settings from the existing probes. |
| OpenCode | Embedded SDK inside an owned Node host, bridged through Brain's runner protocol | Suppress startup continuation, export session-scoped native state incrementally, and qualify credentials, compaction, tools and shared-host behavior. |

The [existing probes](../research/harness-probes.md) verified exact catalogs for selected Codex/OpenCode settings, not complete adapters. OpenCode 2.0.30 startup auto-resume and incremental SQLite export remain concrete adapter work. A newer upstream release is adopted only with a new pin and the same tests. No adapter may add local execution, title generation, warming or other inference outside the configured policy merely because its native application enables it by default.

The external session/tool contract stays common; native runtime-specific capabilities are exposed only through validated adapter options. Unsupported selections fail before execution. All three adapters must demonstrate empty/exact catalogs, two independent customer keys, concurrent peers, local cancellation, host failure, passive cold restore and the next request after compaction.

## Models tools and credentials

### Server and client authoring

Both the application SDK (the server role, called BrainServer here) and client SDK (Brain) support tool declarations and model credential configuration. This specifies roles, not a commitment to rename Go packages or publish a new package called `brainserver`.

- At startup, trusted application code installs models/providers, default credentials, ordinary managed tool handlers and HTTP/MCP integrations. The stock executable's configuration uses the same validation and runtime.
- At session creation, an authorized client selects a model and can supply its own private credential or an authorized credential reference. It can select managed tools with permitted resource settings/credentials and declare ordinary runtime functions.
- Server and client tool declarations share input, output, progress, cancellation and result semantics. Their handler runs where it was declared. A client declaration never deploys a closure to the server.

Selected authoring shape, in pseudocode; exact language-specific signatures are implementation details:

```text
server = BrainServer(
  models = { openai: ModelProvider(apiKey = serverKey) },
  tools = { lookup: Tool(input = ..., output = ..., run = serverLookup) },
  harnesses = { pi: Pi(...), codex: Codex(...), opencode: OpenCode(...) },
  hostPool = HostPool()  // no additional aggregate utilization cap
)

brain = Brain(url, accessToken)
session = brain.create(
  harness = "codex",
  model = Model("openai/<model>", credential = ApiKey(customerKey)),
  tools = [ManagedTool("lookup"), Tool(name = "local", input = ..., run = clientLookup)]
)
```

Keep the session's model, instructions and tool selection fixed at creation. Model credentials belong to private admission input, never instructions, public config, model arguments, event data or logs. Use a private model-credential field distinct from tool aliases rather than overloading the existing tool `credentials[].binding` field.

Credential selection order is an explicit authorized session credential, then an authorized startup default when no override was supplied. A rejected, missing-on-restore or revoked override fails; it never falls back to a default. Persist direct overrides encrypted before admission succeeds and include encrypted recovery material in checkpoints. Authorized external references resolve in the session's tenant scope. Reuse the existing independent key-provider lifecycle and revocation rules.

Do not change process-global environment variables, native login state or a shared provider's mutable headers to switch one session's key. Where upstream cannot scope credentials per thread/session, partition native hosts by authorized credential context. Startup defaults do not grant unrestricted client-selected model endpoints; endpoint policy remains deployment-owned.

API keys are required in this slice. OAuth renewal and provider-specific non-key credential schemes need explicit adapter support; accepting an opaque token is not an OAuth onboarding implementation. Brain authentication tokens and model-provider credentials are separate authorities.

### Bindings executors and commands

The baseline exposes `config.bindings` in generated client types. It is therefore visible to low-level SDK and HTTP users today. A binding records where a selected tool runs, its alias/operations, fixed resource settings and credential selection. For example, selecting read/list on one bucket resolves several operations against that bucket; the model cannot replace the bucket authority through its arguments.

Add ergonomic tool/model constructors which lower into generated wire types. Normal SDK examples should not require constructing bindings, executor IDs or connection frames. Low-level reference documentation still names the actual wire fields; hiding plumbing does not mean claiming it is absent from the protocol.

An **executor** is an authenticated live registration for client functions. A **command** is a transport message such as `ready`, `call`, `cancel` or `ping`, not a shell command. A call names the tool, arguments, call/connection identity and trusted scope. The SDK invokes the local function and returns progress/results. These execution messages are separate from replayable session events: reading old `tool.call` events cannot execute callbacks.

Keep executor endpoints for SDK transport and advanced consumers. The ordinary user writes a tool function, not a command consumer. On restart the client redeclares and rebinds its functions; lost dispatched actions are reported as uncertain and never replayed automatically.

## Browser integrations

Support two distinct tools through the common tool contract:

| Integration | Execution and permission boundary | Lifetime |
| --- | --- | --- |
| Attached tab | A declared browser function, or an explicitly installed extension for broader browser control; use an explicit user gesture and scoped permissions | Closing the page/extension connection removes that executor. DOM access is limited by browser permissions. |
| Remote browser | Managed external browser service with session/account authorization, isolated browser contexts and service-owned cleanup | Independent of the observing browser; the remote service owns browser lifetime and resource limits. |

A page can implement tools against its own DOM; controlling another origin or the general browser needs a suitable extension or remote automation service. CORS does not grant DOM control. Chrome's [activeTab permission](https://developer.chrome.com/docs/extensions/develop/concepts/activeTab) grants temporary access following a user invocation and has navigation-related lifetime rules. Remote browser cookies, downloads and credentials remain in that service's authorized context, outside native harness hosts.

### Why the baseline browser connection fails

Cross-origin JSON requests carrying `Authorization` normally need an OPTIONS preflight and matching [CORS response headers](https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/CORS). Brain's baseline handler has no configured CORS policy. A browser cannot fix this by adding response headers itself or using `no-cors`.

Browser [EventSource](https://developer.mozilla.org/en-US/docs/Web/API/EventSource/EventSource) has no arbitrary Authorization-header option; use streaming fetch for bearer-authenticated SSE. The browser [WebSocket constructor](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket/WebSocket) likewise does not accept arbitrary request headers. WebSocket authorization requires its own design, not an HTTP CORS switch. Brain additionally rejects every executor RPC request with an Origin header, even when the app and API are same-origin.

The first supported path uses a same-origin application/platform gateway, streaming fetch for observation and reverse SSE/HTTP runtime callbacks. The backend issues short-lived, revocable authority scoped to the permitted session/composition and executor; it retains management and model-provider keys. The gateway authorizes and relays calls/results rather than turning a browser grant into a management token. This avoids requiring browser WebSocket RPC for the initial journey. It does not grant arbitrary page execution or reconstruct a closed tab.

Direct cross-origin Brain access is an optional follow-up. It requires an explicit origin allowlist and preflight handling tied to the application's authenticator. Allow only needed methods/headers, expose the request ID, and vary origin-dependent responses. CORS controls browser access to responses; authentication still authorizes operations. If cookies authenticate mutations, also enforce the application's CSRF/origin policy. Deny unknown origins; never use wildcard credentials.

Browser RPC can follow with an explicit Origin check and a short-lived executor credential exchange suitable for the WebSocket API. Do not put long-lived secrets in URLs or log authentication subprotocols. No silent transport fallback can redispatch a tool call. Platforms issue browser grants; Brain supplies neutral authentication/transport hooks, not account or login UX.

## Usage evidence and billing coverage

Brain records model execution and usage evidence. Aex and other platforms own prices, wallets, reservations, receipts and commercial policy. Provider-reported tokens are evidence, not proof that every billable upstream attempt was observed.

| Harness | Available upstream evidence | Adapter implication |
| --- | --- | --- |
| Pi | [Pinned usage type](https://raw.githubusercontent.com/earendil-works/pi/v1.2.0/packages/ai/src/types.ts) includes input/output, cache read/write, totals and optional reasoning. Reasoning is a subset of output. [Compaction](https://raw.githubusercontent.com/earendil-works/pi/v1.2.0/packages/coding-agent/src/core/compaction/compaction.ts) returns optional summary-call usage. | The Brain runner forwards assistant-message usage but does not cover every request path. Observe compaction and native retries as well; do not count reasoning twice or assume every all-zero object means measured zero. |
| OpenCode | [API schemas](https://opencode.ai/v2/docs/api) expose input/output/reasoning/cache counts and optional message/compaction usage and cost. [Hooks](https://opencode.ai/v2/docs/build/plugins/) distinguish primary, compaction, title and generate calls, retries, HTTP and experimental WebSocket traffic. | Account for enabled auxiliary calls. HTTP-only interception misses WebSocket traffic. Native messages and totals require deduplication and pinned semantic tests; fields alone do not establish physical-attempt coverage. |
| Codex | [App Server](https://learn.chatgpt.com/docs/app-server) exposes `thread/tokenUsage/updated` and native turn completion. Its API-key mode accepts caller credentials. | Treat native usage updates according to the pinned schema's cumulative/last semantics. The public notification does not establish an individually identified record for every retry or compaction request. Qualify native observation, then add upstream instrumentation where needed. |

Current upstream documentation and the pinned probes are different evidence. Recheck the selected package/binary schemas before implementing adapters; a newer hook described online is not automatically available in the pinned release.

Add durable `model.request.started` and `model.request.finished` events, extending the event set and runner protocol together. A request carries Brain session/input identity, logical request identity, purpose (primary/compaction/other enabled inference), harness, provider/model, and terminal state. Where native physical attempts are observable, retain their identity, reported provider request ID and outcomes. Where they are not, record the coverage gap rather than inventing attempt records or claiming exact provider billing.

Quantities carry explicit reported/partial/unknown availability and semantics. Preserve input/output/cache/reasoning distinctions, subset relationships, and whether a native value is cumulative or incremental. Normalize those meanings before platform rating. Store supplied provider/model identity even for errors; do not infer usage from answer length or use the native catalog's dollar estimate as an invoice.

Record the start before model dispatch; persist outcomes before settling the turn. A crash after dispatch can leave unknown usage. Observe all enabled model paths at the adapter/provider boundary, including compaction and retries; disable optional title/warming calls by default. Do not wrap only the public `prompt()` function or final assistant message. Use existing native hooks first; request a narrow upstream seam where they cannot establish the required coverage. An inference proxy is not a selected default.

Reconnect, native history recovery and repeated cumulative notifications must not duplicate receipts. Reconcile by request/event identity and retain explicit unknowns. A platform decides whether incomplete usage permits its offer; the readiness gate requires every enabled call path to be accounted for or explicitly marked unmeasured. Measuring observed requests is distinct from guaranteeing exact provider invoices after an ambiguous network failure.

## Platform ownership and clean cutover

Platforms implement session-inspection tools, cross-instance discovery and any model-visible session catalog. Brain retains ordinary authorized metadata/history reads and the enforcement of tenant/session scope. Relocate the existing optional sessions-tool provider to the platform integration in the clean release; its product-facing catalog and policy do not belong in Brain.

Keep routes unversioned. Update core, runners, generated contracts, SDKs and examples together. Do not add a legacy adapter, old-format importer or dual-runtime routing. Use fresh storage and a coordinated cutover. A platform may retain an old backup under its retention policy, but it is not promised continued execution or import into new Brain. Aex separately owns deletion/retention completion and financial reconciliation.

## Implementation defaults and boundaries

1. **SDK syntax:** use concise model/provider/tool constructors and private credentials. Implementers choose language-specific signatures and package names within these semantics; examples remain illustrative until code and generated types land.
2. **Browser access:** implement the same-origin gateway plus SSE/HTTP journey first for both browser integrations. Direct CORS and browser RPC are follow-ups, not prerequisites for that journey.
3. **Accounting precision:** permit explicitly documented unknown upstream usage. Every enabled path must be observed or identified as unmeasured; never substitute zero or claim exact provider invoices. Platforms own how unknown usage affects their offers and reconciliation.
4. **Resource enforcement:** preserve existing per-host controls and add no aggregate utilization restriction by default. Hard OS limits are an optional launcher extension when a deployment needs them, not a prerequisite for the first pool implementation.

No further product decision is required to start plan 12. Deployment-specific persistence, budgets and commercial billing policy remain platform choices.

## Verification

Each adapter passes the same real-runner contract suite, including three harness selections, empty/exact tool catalogs, per-session keys with shared peers, cancellation, host death and passive recovery after compaction. Exercise pool defaults and configured aggregate caps under concurrent admission; test OS ceilings only when that launcher is selected.

Browser journeys cover both integrations, same-origin streaming, allowed/denied cross-origin requests, scoped grants, expiry/revocation, closed tabs, late/duplicate replies and unknown outcomes. Recording-provider journeys cover multiple generations, compaction, retries, partial streams, failures before/after dispatch, native usage snapshots and crash reconciliation. No missing credentials or unsupported path counts as a passed integration test. CI gates release.
