# Roadmap

Brain is a standalone runtime. Applications and extensions supply product policy and infrastructure.
The MVP keeps Tool and Agentloop placements explicit and fixed at session creation.

- [x] The four-part runtime: agent loop, model, tools, environment
- [x] Raw WebAssembly Components supplied with `component(...)`
- [x] Explicit Agentloop and Tool placement with `{ env, ...options }`
- [x] One canonical per-session journal with disposable projections
- [x] Effect-after-commit with no automatic retries
- [x] Logical Environment setup with the needs of everything placed there; Environments own lazy allocation and resource TTL
- [x] Content-addressed Agentloop and Tool Components
- [x] HTTP/SSE session API and the `@aexhq/brain` SDK
- [x] Remote Environment contract and `env-aws-microvm`
- [x] One Environment protocol for Brain's own brain env, the host env, and Environments reached over HTTP, each instance configured by the application
- [x] Tools and Agentloops declare what they need as URIs; the brain env grants exactly that, bounded by deployment allow-lists
- [x] The host env over SSE with durable `ctx.emit`
- [x] Cross-session native workspace isolation test
- [x] Public SDK user journeys against real servers and workers, with isolated suites running in parallel
- [ ] Native subagent support, parent and child links between sessions
- [x] Agentloops running in an Environment reached over HTTP: Brain's turn services as session
  routes with a per-turn token, so a loop can run on another server
- [ ] Post-MVP official `tool-env` Tool extension: inspect the session's placements and Environment
  status, expose failures to the Agentloop for model-directed recovery, and request placement
  changes and supported Environment lifecycle operations such as restart within explicitly
  granted session authority; journal mutations and their outcomes
- [ ] Post-MVP mutable placements: committed changes apply to subsequent calls, including within
  a turn; already-dispatched calls retain their original target. MVP placements are explicit and
  fixed at session creation
- [ ] Post-MVP optional Brain-selected placement for Tools without an explicit Environment, within
  caller-granted authority; MVP placement remains explicit
- [x] Official Agentloop extensions expose Tool failures, Environment status, expiry, and
  resource loss to the model; Agentloop policy decides recovery without runtime retries
- [x] Record interrupted turns as session Events that Agentloops can read and include in their
  transcripts; the user decides the next action, with explicit activation and no automatic retries
- [x] Agentloop APIs to read session Events and append extension Events through the existing
  emit interface, with history readable while execution is suspended; host imports run only during an activation
- [x] Cheap suspension at turn boundaries with transcript and recorded Events readable from
  disk without activating the Agentloop; load sessions on demand after a process restart
- [ ] Evaluate releasing Agentloop memory while awaiting model or Tool results, keeping the
  extension authoring interface simple and measuring continuation costs before adopting it
- [x] Separate extension artifact admission and compilation from session creation; reuse
  compatible compiled artifacts and invocation templates
- [x] Environment extensions can prepare resources lazily on invocation and own TTL and cleanup
  policy; setup need not provision compute, and expired resources need not be restored
- [x] Per-session live subscriptions, independent of Agentloop activation
- [ ] Post-MVP configurable resource admission, memory and compiled-code cache budgets, and
  fair scheduling for deployments running mutually untrusted extensions
- [ ] Post-MVP configurable kv bounds per Agentloop, key count and serialized bytes, set by the
  deployment that knows its machine; Brain sets no fixed limit
- [ ] Optional worker isolation integrations such as gVisor or MicroVMs for deployments needing
  an additional boundary around Wasm execution
- [ ] Multimodal input, images and files on `send`
- [ ] Freeze a v1 API with tagged releases
- [ ] File access and workspace sync
- [ ] crates.io publication
- [ ] Sessions spread across machines sharing environments
- [ ] Session export and import
- [ ] Custom images with scoped credentials and network metering
- [ ] Local environment: run tools in a directory or container on your own machine
- [ ] Browser environment and DOM tools: a page as the place tools run
- [ ] Post-MVP public storage/commit interfaces for optional external stores and commit services,
  preserving acknowledged-record and commit-before-effect guarantees; the default remains local
  and self-contained. Shared ownership, node-loss recovery, backup, and regional recovery belong
  to extensions and consuming platforms
- [ ] Post-MVP bounded client reorder buffer if parallel delivery can emit committed Events out of
  journal sequence; replay repairs gaps
