# Implementation plan: ADR-040 and ADR-041

Compiled 2026-09-05, revised 2026-09-06, against the working tree at `c3c0dc5` plus the two Proposed records. A plan for review, not a commitment.

## 0. How this plan is cut

Two rulebooks shape every item below. The platform's code design rules (`platform/docs/code.md`): explicit over implicit, one source of truth, illegal states unrepresentable, low coupling and high cohesion, fail fast, open for extension and closed for modification, stable public contracts with evolving internals, and documentation kept minimal so it cannot go stale. And ponytail's ladder, applied before anything is written: does it need to exist, does it already exist here, does the standard library or the platform do it, does an installed dependency do it, can it be one line, and only then write the minimum. Lazy about the solution, never about reading.

Applied to this work the ladder mostly says delete. Two ADRs remove eleven concepts and add three: a tagged environment entry, an adapter interface that carries services, and a callback surface for turns that run elsewhere. Every stage below is marked **delete**, **rename**, **reuse**, or **write**, and only the **write** items get design attention. No new dependency is needed anywhere: `url` is already a workspace crate, Wasmtime's own preopen and HTTP hooks already enforce grants, the SDK validates URIs with `new URL()`, and the credential store, the registration table, the SSE pump, the schema generator, and the route macros all exist.

## 1. Decisions taken in review, and what they resolve

1. **An environment instance carries its own address.** An extension defines its options and the application configures each instance, for example `awsMicroVm({ url, region, token })`. Brain reads the `url` and an optional credential; the credential is sealed beside the model key and never journaled; everything else is the extension's configuration, opaque to Brain. The server-side routes file, default endpoint, default key, `with_route`, and their endpoint validation go. Note under ADR-038 for later: a session can now make the server call any URL it names with the session's own credential, the same trust as the model key today; a hosted multi-tenant deployment may want an egress ceiling on environment URLs.
2. **The environment provides resources and does not know what runs until it runs it.** Setup carries the environment's configuration and one flat list of needs from every tool and the loop placed there. Each invoke carries the tool's implementation descriptor, its own needs, the input, and the deadline. Each turn carries the loop's id, configuration, needs, and input. Brain's environment computes grants per call from those needs. A component never admitted, or an environment that cannot run a loop, fails at the first call or first message rather than at create, and Brain journals it like any failure.
3. **Loops run anywhere.** The loop declares needs like a tool, and a loop may run in an HTTP environment on another server. That needs Brain's turn services reachable from another process: stage B8. Decide whether B8 ships in this batch or immediately after; B4 is designed for it either way.
4. **Clean cut.** Contracts, the WIT world, and the data directory change in place. Identifiers and the format marker stay; existing data directories are deleted; nothing is versioned for compatibility.
5. **Vocabulary.** Environments; two that Brain ships, the brain env and the host env, the latter being whatever process registered as a host, a browser tab, a Node process, or a server; tools and the loop are placed in an environment. Retired in prose: resident, app, route, transport, driver, binding, attach. Wire names that already exist and stay: the `driver` tag, the `/v1/hosts` routes and `host_id`. SDK methods are named after what they do.
6. **Per-call grants in the brain env.** The worker already receives a fresh grant set per invocation, so each tool gets only its own needs. Free and strictly tighter.
7. **What the brain env honours.** `file:///workspace` (persistent per session) and `file:///scratch` (per invocation), read-only unless `?access=write`; `https:` and `wss:` destinations with the CSP leading-label wildcard; `pkg:` refused. Anything else refused at setup, naming the URI. Deployment allow-lists remain the ceiling and are renamed `BRAIN_ENV_*`.

Still open, with a recommendation:

8. **Rust type names for the content address.** `Sha256` for the value, `AgentloopId` and `ToolId` for the wrappers; the field is `id`.
9. **SDK names, decided: name them after what they do.** `residentHost()` becomes `register()`, `residentHostCredentials()` becomes `credentials()`, the `residentHost` client option becomes `credentials`, `ResidentHostCredentials` becomes `HostCredentials`, the internal `ResidentHostPump` becomes `HostPump`, and `inspectResidentTool` and `inspectPlacedTool` become one `inspectTool` since there is one tool form.
10. **Staging.** Two PR groups, ADR-040 then ADR-041, so contracts break twice rather than once per PR. Inside a group the order in section 3 keeps CI green.

## 2. Shapes that make illegal states unrepresentable

These are the only new types. Each replaces a set of optional fields plus cross-field rules with a shape that cannot be wrong, and each renders to the wire decided in review because serde tags and schemars render enums as `oneOf`.

- **Environment entry.** `Environment { name, driver: Driver, configuration }` with `#[serde(tag = "driver")] enum Driver { Brain, Host { host_id }, Http { url, credential: Option<Secret> } }`. The wire reads `{ "name": "sandbox", "driver": "http", "url": "…", "configuration": {…} }`. A `Brain` entry cannot carry a URL; an `Http` entry cannot lack one; an unknown driver fails at parse. Adding a fourth kind is a contract change and a new enum arm, which is the right cost for a contract Brain owns.
- **Tool.** One `Tool { name, description, input_schema, output_schema, environment: EnvironmentName, needs: Vec<String>, implementation }` in the configuration, with `Tool::definition()` as the pure projection the loop sees. The `tools` and `tool_bindings` lists merge, `ToolBinding`, `ToolHosting`, `BoundTool`'s schema transform, and the six cross-field rules go; the remaining rule is "every tool names an environment of this session", which a lookup at create enforces and which stays a rule rather than a type only because the environment list is data.
- **Content address.** `Sha256([u8; 32])`, unchanged in behaviour, renamed to say what it is; the field is `id`.
- **Environment operation.** `Setup { configuration, needs }`, `Invoke { implementation, needs, input, deadline_ms }`, `Turn { agentloop: { id, configuration, needs }, input }`, `Call`, `Cancel`, `Detach`, `Teardown`. `Attach`, `Provision`, `ToolManifest`, `EnvironmentBinding`, and the receipt's `resources` go; `Accepted` becomes a unit variant.
- **Names.** `EnvironmentName` for the caller-chosen per-session name, replacing `EnvironmentId`, because it is a name.

The `EnvironmentAdapter` trait is the one abstraction the server keeps for extension. It gains the session's services as a parameter; three files implement it in the same shape and method order so a grep for `impl EnvironmentAdapter` shows the whole story: `environment/brain.rs`, `environment/host.rs` (today's `resident.rs`), `environment/http.rs` (today's `adapter.rs`). The dispatcher and the loop executor resolve a name to an adapter and call it; nothing else in the server knows which kind it has.

## 3. Work breakdown, with the ladder applied

### Group A: ADR-040

- **A1, delete `event_id`.** `SessionRecord::event_id`, `Event.event_id`, `EventId`, `journal/cursor.rs`, `journal/feed.rs`, SDK `SessionEvent.id`, the events journey assertion, the sessions concept page. One line written: `sequence: Option<u64>` on `TelemetryRecord`, and the `LogSink` field beside it.
- **A2, delete `binding_id`.** `ModelBinding` becomes `{ provider, name }`; `ModelExecutor::execute` takes the session id; the credential store is keyed by session id, with the AAD following (reuse: the sealing code is unchanged, only its key). `metadata.rs`, `model_binding.rs`, create and delete in `service.rs`.
- **A3, rename the content address.** Compiler-driven: type, wrappers, fields, `/v1/agentloops/{id}`, `brain_component.id`, contract examples, conformance test.
- **A4, rename environment ids to names.** `EnvironmentId` becomes `EnvironmentName`, fields become `environment`; SDK factories take `{ name, …options }` at instantiation, mirroring `{ env, …options }` for placement; `collectEnvironments` stops hashing the create key (delete); the environment call route parameter renamed. The server keys its rows by `(session, name)` for now; the rows disappear in B4.
- **A5, delete `attachment_id`, `directory_generation`, `EnvironmentBinding`.** Protocol, registry, dispatcher, `SessionConfig`, contract examples, `lazy-environment` and its test, `brain/tests`.
- **A6, slim the records; drop `call_id` from the wires.** `tool_call_started` becomes `{ tool, invocation, deadline_ms }`, `tool_call_ended` drops the call id, `Invoke`, `HostOperation`, `NativeToolInput`, the worker wire, and `tool.wit` lose it; the diagnostic tool fixture stops emitting it; the SDK pump and registry key by sequence; `ctx.callId` becomes `ctx.sequence`. The reference loop is untouched.
- **A7, regenerate and sweep.** Contracts and SDK types by `npm run gen`; the five hand-written contract examples; the bench driver and release scripts that build raw bodies; docs pages for sessions, tools, environments, and the `app-tools` guide; both READMEs.

### Group B: ADR-041

- **B1, write the `Driver` enum; delete routes.** The entry shape from section 2; the SDK `environment()` contract gains `url` and optional `credential` functions of the parsed options and stops merging `driver` into configuration (reuse `new URL()` to validate); `brainWasm` becomes `brainEnv`; the server seals the credential with the existing store keyed by session and environment name; the routes file, default endpoint and key, `with_route`, `validate_environment_endpoint`, their config fields and CLI flags, `examples/environment-routes.json`, and the routes paragraph in the examples README go; the journey and release harnesses pass URLs to their reference environments instead of writing a routes file.
- **B2, delete bindings and resources.** `bindings`, `binding_names`, `BindingValues`, `SessionBindingValues`, redaction; `Resources`, `ResourcePolicies`, `RESOURCE_NAME_PATTERN`, `resource_name_valid`, receipt `resources`, per-environment `resources` in the configuration, the bind check; `needs` becomes a bounded list of strings Brain never reads; the SDK's exported resource types go; the docs' resources and authority sections describe what an environment does with needs.
- **B3, write the new operation shapes; delete attach.** From section 2. `AgentloopRef` gains `needs`; the three `environment_attach_*` codes and the `call::ENVIRONMENT_ATTACH` prefix go from `codes.rs`, `codes.json`, and the journal store's unclosed-effect prefix list; `create_for_session` and `prepare_session` merge; `lazy-environment` keeps its implementations per call.
- **B4, write the adapter interface with services; delete the special cases.** `EnvironmentAdapter::execute(entry, operation, services)`; `Turn` answered in-process now and over HTTP after B8; `environment/brain.rs` wraps `WorkerPool` (reuse: `native_environment` moves in and reads needs instead of configuration; setup refuses needs it cannot honour; invoke checks admission, computes grants, runs; turn the same for the loop; detach and teardown remove the workspace); `environment/http.rs` is today's HTTP adapter with the services parameter ignored; the server's `LoopExecutor` resolves the loop's environment and calls its adapter, so `LoopExecutor::turn` takes the entry instead of a JSON value; the seven `brain_wasm` string checks, `brain_wasm_resources`, `validate_native_environment`'s configuration parsing, `EnvironmentResources`, and the `environments/` directory go; reachability lives in registry memory keyed by session and name. Verify during this stage that a failed teardown awaiting a later delete is readable from the journal, which is the one job of the deleted store with an ADR-036 obligation behind it.
- **B5, the host env; delete the two-form rules.** `resident.rs` becomes `environment/host.rs` implementing the trait (setup checks the host is connected and binds the session with the existing registration log; invoke and cancel forward; detach releases); `hosting`, tool-level `host_id`, `bound_tool_rules`, and the six cross-field rules go; SDK `hostEnv({ name })`, `run` tools take `{ env }`, `compileSession` emits the host env entry with the registered `host_id`, `sessions.get` re-attaches by environment name; the SDK names in decision 9; the conformance test for the old rule rewritten; the host-tool journeys in `resident.test.mjs`, renamed with them.
- **B6, write grants from needs.** In `environment/brain.rs`: URI to preopen or allow entry, bounded by policy, per invoke and per turn; the wildcard extension to `network_allowed` (reuse the existing matcher, add the leading-label case); `brainEnv({ secrets })` keeps only that option; `BRAIN_WASM_*` become `BRAIN_ENV_*` in `config.rs`, `main.rs`, the docs configuration table, both harnesses, and the SDK README; placement journeys declare `file:///workspace?access=write` on the tool instead of a filesystem option on the environment.
- **B7, sweep.** Section 4.
- **B8, write turn over HTTP.** Per-turn token minted at activation and carried in the `Turn` operation with a callback base URL; session routes `/v1/sessions/{id}/turns/{sequence}/{model,dispatch,emit,events,telemetry}` that resolve the open activation's `TurnServices` and refuse anything else (reuse the route macros and the bearer check already used by the host routes); a per-activation services table in the server; `environment/http.rs` posts the turn and waits under the turn's liveness bound; `cancel` fails outstanding callbacks with `cancelled`; a reference environment that runs a loop so the journeys exercise it the way `lazy-environment.mjs` exercises tools. The one stage with a design of its own; write it as an amendment to ADR-041 before the code.

## 4. Follow-up sweep

Everything that describes or consumes the changed surface, grouped by where it lives. Documentation stays minimal: each page says what a thing is and how to use it, and the ADRs carry the why.

**Docs site** (`docs/`, shipped to aex.dev/brain/docs; the API reference page is rendered from the OpenAPI file and needs no hand edit).
- `concepts/tool.mdx`: one tool form; `needs` as URIs; `ctx.sequence`. `concepts/environment.mdx`: the entry shape, `brainEnv` and `hostEnv`, per-instance URL and credential, what an environment does with needs, no routes. `concepts/sessions.mdx`: records without `event_id`, the slimmer `tool_call_started`. `concepts/agent-loop.mdx`: the loop declares needs and may run in any environment.
- `guides/write-a-tool.mdx` absorbs `guides/app-tools.mdx`, since a `run` tool is a tool placed in the host env; remove `app-tools` from `guides/meta.json`. `guides/write-an-environment.mdx`: setup with needs, invoke with implementation, turn, per-instance configuration, and after B8 the loop-running half. `guides/embed.mdx`: the changed `ModelExecutor` and `LoopExecutor` signatures. `guides/write-a-loop.mdx` and `guides/subagents.mdx`: `brainEnv`. `quickstart.mdx` and `index.mdx`: snippets. `reference/configuration.mdx`: the `BRAIN_ENV_*` rows, the three environment rows removed, the data layout list without `environments/`.

**Repository front matter.**
- `README.md` and `README.cn.md`: the architecture diagram loses its "Resident Tools" and "brainWasm · Wasmtime worker" boxes in favour of the host env and the brain env as two environments beside any other; the `brainWasm(options)` paragraph; the quickstart snippet. Keep the two files in step.
- `ROADMAP.md`: the shipped lines that name `brainWasm`, logical setup and attachment, resident hosts, and typed content identity; the post-MVP lines that speak of mutable bindings say placements.
- `AGENTS.md`: the invariant sentence "A session's Tool catalogue and bindings do not change after create" says placements; nothing added.
- `CONTRIBUTING.md` and `SECURITY.md`: no mentions; unchanged.

**Extensions and examples.**
- SDK: the factories in `extensions.ts` (`tool`, `environment`, `agentloop`, `brainEnv`, `hostEnv`, `component`), the public index and types, the names in decision 9, `packages/brain-sdk/README.md`, and the WIT files copied into the package by `npm run gen`.
- `examples/`: the four scripts use `brainEnv()`; `lazy-environment.mjs` is the reference environment for tools and, after B8, a sibling runs a loop; `environment-routes.json` deleted; `examples/README.md` loses the routes paragraph and gains the URL option; `examples/package.json` unchanged.
- Fixtures: the diagnostic tool stops emitting the call id; the diagnostic loop and the reference loop are untouched; all three are built in CI from source.
- Downstream: `hands` consumes tags. One "breaking changes" list in the release notes for the first tag after Group B, naming removed and renamed API, is enough; there is nothing to migrate.

**Harnesses, tooling, CI.**
- `tests/journeys/README.md` table rows for resident, `brainWasm`, and bindings; `support.mjs` passes URLs and sets `BRAIN_ENV_FILESYSTEM_ALLOW`.
- `tests/release/*.mjs`: hand-built create bodies and the routes file in `runtime.mjs`.
- `tools/bench`: the `environment_base_url` plumbing in `launch.rs`, `probes.rs`, `main.rs`, `subjects/brain/subject.json`, and the README; `drivers/brain.rs` builds a create body. Under ADR-039 the numbers stay historical; the runner only has to run.
- `tools/package-smoke.mjs`: two mentions.
- `.github/workflows`: no environment variables to rename; the contract diff gate and the fixture builds cover the rest. `Dockerfile` and `compose.yaml` set none of the renamed variables.

**Decision records.**
- ADR-040 and ADR-041 move to Accepted when merged and into the README's current-architecture list.
- Reversals to record in the README: ADR-022's create-time needs check → ADR-041; ADR-033's two tool forms → ADR-041; ADR-024's "random attachment identifiers retain their own meanings" → ADR-040; ADR-036's "session-owned model bindings" and "resident token hashes" wording → ADR-040 and ADR-041; ADR-003's consequence line about resident tools → ADR-041. Their status fields stay Accepted with a successor link, since each still stands in the main.
- `DECISIONS.md` and `SOURCES.md` need nothing: new records cite their session directly.

## 5. Size

| Area | Files touched | Net direction | Tests to rewrite or add |
| --- | --- | --- | --- |
| `brain-protocol` | 9 of 15 source files, conformance test | delete | 7 conformance, 3 to 4 new |
| `brain` core | `session/mod.rs`, `actor.rs`, four `journal/` files, `model/mod.rs`, `tool.rs`, `agentloop.rs` | delete | 22 in `tests/session.rs`, `tests/common`, 5 unit |
| `brain-server` | `service.rs`, `environment/{registry,adapter,resources}.rs`, `resident.rs`, `tool_dispatcher.rs`, `metadata.rs`, `model_binding.rs`, `main.rs`, `config.rs` | delete, except B4 and B8 | 5 service, 5 host env, 5 model binding, 3 session, registry unit |
| `brain-http` | `router.rs`, `openapi.rs`, `service.rs`; B8 adds one route group | write in B8 | 12 route tests, plus B8 |
| `brain-loophost` | `wire.rs`, `runtime.rs`, `supervisor.rs`, `client.rs`, `service.rs` | move | 7 worker process, 4 runtime |
| `brain-telemetry` | `record.rs` | one line | 2 |
| Contracts | all rendered files, 5 hand-written examples, `tool.wit` | rendered | by CI |
| SDK | `extensions.ts`, `client.ts`, `client-pump.ts`, `app.ts`, `types.ts`, `index.ts`, generated | delete | 14 unit |
| Fixtures and examples | diagnostic tool, `lazy-environment.mjs` and test, 4 scripts, routes example deleted | delete | 1 |
| Journeys, release, bench | 8 journey files (35 tests), 6 release scripts, bench runner | rename | most journeys touch a changed field |
| Docs and front matter | 13 pages and one nav file, both READMEs, ROADMAP, AGENTS, SDK README, journeys and bench READMEs | delete | n/a |

Group A is compiler-driven. Group B writes three things, the `Driver` enum with its adapters (B1, B4, B5), grants from needs (B6), and turn over HTTP (B8), and deletes the rest. Expect the server and protocol crates to end smaller than they start; `resources.rs` alone is 253 lines that go.

## 6. Deferred, harvested rather than lost

- An egress ceiling on environment URLs for hosted multi-tenant deployments (ADR-038 territory).
- A `needs` list on the host env's tools, if a host ever wants Brain to record what its own functions touch; today it is empty by construction.
- Renaming the data directory's `agentloops/` folder, which holds both loop and tool artifacts, to `artifacts/`. A one-line constant and a docs row; harmless to do in A7, harmless to leave.
- A callback channel for HTTP environments beyond turns, if a remote tool ever needs Brain services mid-call; today it emits through the receipt stream.

## 7. What does not change

The worker process and its capability policy (ADR-034). The journal format and prefix deltas. Idempotency claims and their retention. SSE framing, which already uses `sequence` as its id. Model transports and the provider catalogue. Host registration and its three `/v1/hosts` routes. The reference agentloop.
