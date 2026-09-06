# Sources and coverage

Compiled on 2026-09-05 against Brain revision [`c3c0dc5`](https://github.com/aexhq/brain/commit/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170).
Only Brain files are changed. Other repositories, their design archives, hosted-product
operations, and unrelated portions of mixed sessions are excluded at the user’s request.

## Evidence and dates

These are retrospective ADRs, not newly ratified architecture. Existing DECISIONS.md dates
are preserved. Earlier records use the dated discussion when available, otherwise the Brain
commit date establishing the behavior. Session timestamps are UTC; commit dates retain the
repository’s recorded day. The compilation date is separate. Same-day filename order is for
navigation and does not imply an exact sequence within that day.

User decisions establish intent; merged Brain changes and current contracts establish what
shipped. Assistant recommendations and research reports are identified as such and do not
become accepted merely by appearing in a session. Later explicit decisions override earlier
plans. Historical implementation details remain in the cited Git revision, not in a second
current specification. Historical research experiments and comparative benchmark runs were
not rerun for this compilation.

The original monolithic [DECISIONS.md](https://github.com/aexhq/brain/blob/c3c0dc5c7bf57e44c99dfe9a4e2d1e9f05020170/DECISIONS.md)
remains available in Git. Its local replacement preserves the original section anchors and
points to the ADRs below. This avoids two independently maintained decision narratives.

## Existing decision-log coverage

Every original dated entry is mapped here; related naming and configuration entries are
combined where they express one architectural boundary.

| Original DECISIONS.md entry | ADR destination |
| --- | --- |
| 2026-09-05: Standalone, ephemeral execution | [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md); [ADR-038: Defer dynamic placement, workflow durability, and advanced tenancy policy](2026-09-05-07-roadmap-boundary.md); [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md) |
| 2026-09-02: The agent loop owns what the model sees | [ADR-018: Let the Agentloop control model presentation within fixed authority](2026-09-02-01-presentation.md) |
| 2026-09-02: The journal records a model request as a diff against the last one | [ADR-019: Record transcript changes as common-prefix deltas](2026-09-02-02-prefix-deltas.md) |
| 2026-09-02: Effect records are named for what happened, not for intent | [ADR-020: Name effects by session and sequence, with one lifecycle vocabulary](2026-09-02-03-record-identity.md) |
| 2026-09-02: No request identity | [ADR-020: Name effects by session and sequence, with one lifecycle vocabulary](2026-09-02-03-record-identity.md) |
| 2026-09-02: A session has two ids, `session_id` and `sequence` | [ADR-020: Name effects by session and sequence, with one lifecycle vocabulary](2026-09-02-03-record-identity.md) |
| 2026-09-02: The kernel is one session; the server manages sessions | [ADR-021: Make the core runtime one session and let the server manage sessions](2026-09-02-04-session-runtime.md) |
| 2026-09-04: The agent loop drives the turn; Brain provides services | [ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md) |
| 2026-09-04: One session configuration, and nothing is sealed | [ADR-024: Use one SessionConfig and keep hashes local to their actual purpose](2026-09-04-02-simple-config.md) |
| 2026-09-04: Identity is an idempotency key and nothing else | [ADR-024: Use one SessionConfig and keep hashes local to their actual purpose](2026-09-04-02-simple-config.md) |
| 2026-09-04: One directory per session, two append-only logs, one sequence | [ADR-025: Store transcript state and public Events in two per-session logs](2026-09-04-03-two-logs.md) |
| 2026-09-04: Every dependency of a session is injected, and recovery is load and construct | [ADR-026: Recover saved data by loading it, without replaying the Agentloop](2026-09-04-04-load-recovery.md) |
| 2026-09-04: Idle sessions are suspended and rebuilt on demand | [ADR-027: Suspend idle session actors after an idle TTL](2026-09-04-05-idle-ttl.md) |
| 2026-09-04: Environments are resources with an optional managed lifecycle | [ADR-028: Expose independently created and shared Environment resources](2026-09-04-06-shared-environments.md) |
| 2026-09-04: One catalogue of codes | [ADR-029: Declare machine-readable Events and failures once](2026-09-04-07-codes.md) |
| 2026-09-04: The data directory is not migrated | [ADR-030: Replace pre-1.0 contracts and development data in place](2026-09-04-08-clean-break.md) |
| 2026-09-04: The Rust types are the source of the contracts | [ADR-031: Generate public contracts from Rust types and route annotations](2026-09-04-09-rust-contracts.md) |
| 2026-09-05: One canonical journal, with transcript and Events as projections | [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md) |
| 2026-09-05: Placement is explicit and execution has two forms | [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md) |
| 2026-09-05: Brain's native Environment runs Components in one worker process | [ADR-034: Run native Components in a separate capability-restricted Wasmtime worker](2026-09-05-03-wasm-worker.md) |
| 2026-09-05: Brain sends each effect once and reports the outcome | [ADR-035: Send each effect once and expose failures to the Agentloop](2026-09-05-04-send-once.md) |
| 2026-09-05: Preserve effect identity and uncertainty across interruption | [ADR-036: Preserve request claims and effect uncertainty across interruption](2026-09-05-05-interruption.md) |

## Repository sources

| Source family | Treatment |
| --- | --- |
| README.md and README.cn.md | Product framing and current architecture; implementation and contracts take precedence over examples. |
| DECISIONS.md | Every entry migrated using the map above. |
| ROADMAP.md | Accepted MVP deferrals recorded separately from unimplemented future features. |
| docs/concepts and docs/guides | Current session, loop, tool, model, Environment, embedding, and authoring semantics. |
| docs/reference/configuration.mdx | Current limits and grants; historical numerical defaults are not made permanent ADR requirements. |
| BENCHMARKS.md, docs/reference/benchmarks.mdx, tools/bench/README.md | Historical measurements, probe definitions, known limitations, and current diagnostic policy. |
| AGENTS.md, CONTRIBUTING.md, SECURITY.md, LICENSE, .github/workflows | Contract ownership, verification, preview support, and release/product conventions. |
| SDK, examples, and journey READMEs | Current user surface and executable examples; not independent architecture specifications. |
| Rust/WIT/contracts and tests cited in ADRs | Evidence for implementation boundaries, generated contract ownership, and regression coverage. |

Git history was also searched for deleted and branch-only Markdown design material.
`STANDALONE.md` at `ea73244` records the old SQLite/Docker-Hand deployment;
`contracts/abi/v1/README.md` at `ea73244` records the retired Brain/Hand ABI. They inform
the independence and Environment-neutrality records, not current tool/runtime guarantees.
Retired Agentloop/tool package READMEs describe old authoring interfaces; their evolution is
covered by the typed-authoring and step/turn records. Generic historical release notes and
cosmetic README changes are not individual architectural decisions.

`PERF-REVIEW.md` at `374bae2` and `VERDICTS.md` at `b5e08ed` were found on historical
Brain refs rather than current main. Their hypotheses, measured rejections, platform caveats,
and subsequent landed fixes are covered by the research-verdicts and growth records.
Use `git show <revision>:<path>` to retrieve these sources locally if a remote branch disappears.

## Local session provenance

The review searched available local Codex August/September 2026 rollout files and Claude
project histories for Brain-related discussions, then selected relevant parent conversations.
Forked copies and tool-output-only matches were not counted as separate decisions. The
citations below retain only IDs, timestamps, message roles, locators, and authored summaries;
raw transcripts, tool output, credentials, and unrelated product discussion are not copied.

Paths beginning `~` are local user-profile paths, not repository dependencies. These histories
are not available to a fresh clone; the public ADR prose and pinned Brain commits stand alone.
This is coverage of available local evidence, not a claim that every historical conversation
was retained. For the Wasmer/Extism experiments, session reports are the evidence reviewed;
raw artifacts in other repositories were deliberately excluded.

<a id="session-01a01c25-3a95-7fe0-8f80-bbe9f2466b13"></a>

### Codex `01a01c25-3a95-7fe0-8f80-bbe9f2466b13`

Local source: `~/.codex/sessions/2026/08/19/rollout-2026-08-19T23-29-57-01a01c25-3a95-7fe0-8f80-bbe9f2466b13.jsonl`.

- 2026-08-19T22:32:03.900Z, user message, JSONL line 9: User asks for a minimal independent product and questions why Brain depends on Hands.

Used by: [ADR-001: Make Brain an independent runtime with injected execution ports](2026-08-20-01-standalone.md).

<a id="session-51cb3cc4-e44f-4feb-9136-b0647fcdb87c"></a>

### Claude `51cb3cc4-e44f-4feb-9136-b0647fcdb87c`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/51cb3cc4-e44f-4feb-9136-b0647fcdb87c.jsonl`.

- 2026-08-21T13:41:03.243Z, user message, JSONL line 9: User asks for a composable architecture with replaceable agent algorithms.

Used by: [ADR-002: Execute replaceable Agentloops through an isolated step contract](2026-08-21-01-step-loops.md).

<a id="session-01a02b95-b874-7a80-adcb-8ada7196900c"></a>

### Codex `01a02b95-b874-7a80-adcb-8ada7196900c`

Local source: `~/.codex/sessions/2026/08/22/rollout-2026-08-22T23-27-07-01a02b95-b874-7a80-adcb-8ada7196900c.jsonl`.

- 2026-08-23T15:47:25.295Z, user message, JSONL line 280: Brain-related part of the architecture discussion replaces Hand terminology with Environment extensions.

Used by: [ADR-003: Replace the default sandbox with explicit Environment bindings](2026-08-24-01-environment-neutral.md).

<a id="session-01a035ae-d35e-7491-aec2-32a225a91722"></a>

### Codex `01a035ae-d35e-7491-aec2-32a225a91722`

Local source: `~/.codex/sessions/2026/08/24/rollout-2026-08-24T22-30-44-01a035ae-d35e-7491-aec2-32a225a91722.jsonl`.

- 2026-08-24T21:31:50.297Z, user message, JSONL line 29: User clarifies that Brain is independently deployable and tools execute in their chosen Environment.

Used by: [ADR-003: Replace the default sandbox with explicit Environment bindings](2026-08-24-01-environment-neutral.md).

<a id="session-01a036ab-3be6-7cd3-a6cd-f869e412fcce"></a>

### Codex `01a036ab-3be6-7cd3-a6cd-f869e412fcce`

Local source: `~/.codex/sessions/2026/08/25/rollout-2026-08-25T03-06-26-01a036ab-3be6-7cd3-a6cd-f869e412fcce.jsonl`.

- 2026-08-25T03:03:24.128Z, user message, JSONL line 344: Brain customization discussion covers shared resources, telemetry, and extension boundaries.

Used by: [ADR-004: Route all four extension kinds through Component worlds](2026-08-25-01-four-worlds.md).

<a id="session-01a03dd3-4217-7cc3-973c-8336841a6a26"></a>

### Codex `01a03dd3-4217-7cc3-973c-8336841a6a26`

Local source: `~/.codex/sessions/2026/08/26/rollout-2026-08-26T12-27-30-01a03dd3-4217-7cc3-973c-8336841a6a26.jsonl`.

- 2026-08-26T15:06:43.272Z, user message, JSONL line 9: User presents the external-persistence ephemeral-kernel proposal.
- 2026-08-26T16:52:06.882Z, user message, JSONL line 317: User clarifies that standalone Brain keeps its journal on disk.
- 2026-08-26T19:33:16.940Z, assistant message, JSONL line 1634: Assistant reports the initial Wasmer Component/bridge spike and its limits.
- 2026-08-26T21:10:28.715Z, assistant message, JSONL line 2709: Assistant corrects the conclusion after the successful canonical-core Linux bakeoff.
- 2026-08-26T22:07:55.979Z, user message, JSONL line 3246: User accepts the narrowed boundary and requests the final design and implementation specifications.
- 2026-08-26T22:57:59.039Z, user message, JSONL line 3515: User rejects pre-launch compatibility paths and asks to modify contracts in place.
- 2026-08-27T08:41:28.153Z, user message, JSONL line 16629: User supplies a typed session-composition sketch and rejects manual string bookkeeping.
- 2026-08-27T10:29:27.989Z, user message, JSONL line 18808: User requests canary image and SDK/HTTP E2E verification before promotion in Brain itself.
- 2026-08-27T11:16:14.075Z, user message, JSONL line 19597: User distinguishes client-owned queues from Brain’s bounded best-effort telemetry.

Used by: [ADR-001: Make Brain an independent runtime with injected execution ports](2026-08-20-01-standalone.md); [ADR-005: Keep WIT Components and use Wasmtime for the native runtime](2026-08-26-01-runtime-choice.md); [ADR-006: Make external event listeners the only session persistence](2026-08-26-02-external-history.md); [ADR-007: Compose sessions with typed extension factories](2026-08-27-01-typed-authoring.md); [ADR-008: Keep Brain documentation and release verification with the product](2026-08-27-02-repo-gates.md); [ADR-015: Keep telemetry bounded and best effort, and use cursors for durable consumers](2026-08-31-01-telemetry.md); [ADR-030: Replace pre-1.0 contracts and development data in place](2026-09-04-08-clean-break.md).

<a id="session-fcfbf2fc-a95f-45ae-8e3c-b7fcf86b7994"></a>

### Claude `fcfbf2fc-a95f-45ae-8e3c-b7fcf86b7994`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/fcfbf2fc-a95f-45ae-8e3c-b7fcf86b7994.jsonl`.

- 2026-08-27T12:42:55.624Z, user message, JSONL line 5: Launch documentation session later selects MIT and documentation close to code.

Used by: [ADR-008: Keep Brain documentation and release verification with the product](2026-08-27-02-repo-gates.md).

<a id="session-f39792fc-dc4f-46cc-9a96-f985a092c48d"></a>

### Claude `f39792fc-dc4f-46cc-9a96-f985a092c48d`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/f39792fc-dc4f-46cc-9a96-f985a092c48d.jsonl`.

- 2026-08-28T00:05:58.393Z, user message, JSONL line 7: User requests competitor coverage across performance dimensions.

Used by: [ADR-039: Keep performance claims historical until representative baselines are rebuilt](2026-09-05-08-benchmark-policy.md).

<a id="session-f90a511b-6b7b-4e96-a762-5cb6a5101d0a"></a>

### Claude `f90a511b-6b7b-4e96-a762-5cb6a5101d0a`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/f90a511b-6b7b-4e96-a762-5cb6a5101d0a.jsonl`.

- 2026-08-28T00:42:52.056Z, user message, JSONL line 422: User explicitly chooses performance over the old durability fence during the append-only storage review.

Used by: [ADR-009: Replace SQLite with a write-behind segment journal](2026-08-28-01-write-behind.md).

<a id="session-01a045f6-f246-70b0-925d-6c63a03b9bf8"></a>

### Codex `01a045f6-f246-70b0-925d-6c63a03b9bf8`

Local source: `~/.codex/sessions/2026/08/28/rollout-2026-08-28T02-23-26-01a045f6-f246-70b0-925d-6c63a03b9bf8.jsonl`.

- 2026-08-28T01:24:58.008Z, user message, JSONL line 9: User requests a comprehensive performance and memory review.

Used by: [ADR-010: Bound journal growth and measure the complete execution path](2026-08-28-02-growth.md).

<a id="session-e3e5d8ee-bc63-4389-b0bd-333c904870ba"></a>

### Claude `e3e5d8ee-bc63-4389-b0bd-333c904870ba`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/e3e5d8ee-bc63-4389-b0bd-333c904870ba.jsonl`.

- 2026-08-28T01:54:15.502Z, user message, JSONL line 7: Claude independent performance investigation.

Used by: [ADR-010: Bound journal growth and measure the complete execution path](2026-08-28-02-growth.md); [ADR-011: Keep measured performance verdicts separate from hypotheses](2026-08-28-03-research-verdicts.md).

<a id="session-e47f469f-637e-43e2-b35f-4acb7506f708"></a>

### Claude `e47f469f-637e-43e2-b35f-4acb7506f708`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/e47f469f-637e-43e2-b35f-4acb7506f708.jsonl`.

- 2026-08-28T07:57:43.639Z, user message, JSONL line 5: User requests reconciliation with concurrent Codex and journal optimization sessions.

Used by: [ADR-010: Bound journal growth and measure the complete execution path](2026-08-28-02-growth.md); [ADR-011: Keep measured performance verdicts separate from hypotheses](2026-08-28-03-research-verdicts.md).

<a id="session-7074c005-e236-4d1a-8671-1ddfbeb1a27c"></a>

### Claude `7074c005-e236-4d1a-8671-1ddfbeb1a27c`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/7074c005-e236-4d1a-8671-1ddfbeb1a27c.jsonl`.

- 2026-08-28T15:48:12.216Z, user message, JSONL line 7: User requests provider normalization after comparing Brain with ZeroClaw.

Used by: [ADR-012: Store provider-neutral messages and normalize model transports](2026-08-30-01-provider-model.md).

<a id="session-b4871cce-1706-4bb3-8140-531592e60799"></a>

### Claude `b4871cce-1706-4bb3-8140-531592e60799`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/b4871cce-1706-4bb3-8140-531592e60799.jsonl`.

- 2026-08-30T20:24:52.580Z, user message, JSONL line 7: User asks for unified provider support primarily through configuration.

Used by: [ADR-013: Make compatible providers reviewed deployment data](2026-08-30-02-provider-catalog.md).

<a id="session-01a055b3-dee6-7362-b8a0-e244cd46b3c4"></a>

### Codex `01a055b3-dee6-7362-b8a0-e244cd46b3c4`

Local source: `~/.codex/sessions/2026/08/31/rollout-2026-08-31T03-44-06-01a055b3-dee6-7362-b8a0-e244cd46b3c4.jsonl`.

- 2026-08-31T02:46:07.379Z, user message, JSONL line 9: User asks to distinguish in-memory context, append-only journal, and asynchronous publication.

Used by: [ADR-014: Rebuild session views from recorded history](2026-08-30-03-journal-projections.md).

<a id="session-d052a0cb-6a05-467f-89a3-fc10945062d9"></a>

### Claude `d052a0cb-6a05-467f-89a3-fc10945062d9`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/d052a0cb-6a05-467f-89a3-fc10945062d9.jsonl`.

- 2026-09-01T11:10:38.522Z, user message, JSONL line 9: User asks for tool dispatch to browsers, servers, and local computers; subsequent corrections remove backend and third-party-call assumptions.

Used by: [ADR-016: Terminate application Tool channels in Brain](2026-09-01-01-client-channels.md).

<a id="session-56115bda-1ba4-40dc-8c47-28b8e6273e24"></a>

### Claude `56115bda-1ba4-40dc-8c47-28b8e6273e24`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace-brain/56115bda-1ba4-40dc-8c47-28b8e6273e24.jsonl`.

- 2026-09-02T16:10:09.940Z, user message, JSONL line 151: User asks to write from the first changed position while preserving append-only history.
- 2026-09-02T16:16:24.467Z, user message, JSONL line 174: User rejects intent/request-identity vocabulary and asks why another operation ID exists.
- 2026-09-02T17:26:04.688Z, user message, JSONL line 204: User selects session_id and sequence consistently and expects reconstruction from the journal.
- 2026-09-02T17:29:24.403Z, user message, JSONL line 221: User defines the per-session/server split and requires dependency injection.
- 2026-09-02T18:42:27.478Z, user message, JSONL line 1258: User clarifies that the creator still chooses the initial system prompt.

Used by: [ADR-018: Let the Agentloop control model presentation within fixed authority](2026-09-02-01-presentation.md); [ADR-019: Record transcript changes as common-prefix deltas](2026-09-02-02-prefix-deltas.md); [ADR-020: Name effects by session and sequence, with one lifecycle vocabulary](2026-09-02-03-record-identity.md); [ADR-021: Make the core runtime one session and let the server manage sessions](2026-09-02-04-session-runtime.md).

<a id="session-0fc0a00c-3776-4a6c-9383-c5a8fe9bd556"></a>

### Claude `0fc0a00c-3776-4a6c-9383-c5a8fe9bd556`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/0fc0a00c-3776-4a6c-9383-c5a8fe9bd556.jsonl`.

- 2026-09-03T16:40:17.639Z, user message, JSONL line 61: User asks for recovery to be loading persisted data and constructing a session with injected dependencies.
- 2026-09-03T17:29:42.378Z, user message, JSONL line 104: User removes duplicate sealed configuration types and unused digests while retaining idempotency as a valid hashing use.
- 2026-09-03T21:47:56.428Z, user message, JSONL line 258: Discussion separates model-visible session Events from operational telemetry; the resulting decision log standardizes codes.
- 2026-09-03T23:13:37.844Z, user message, JSONL line 504: User accepts a centralized writer rather than an unmeasured thread per session.

Used by: [ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md); [ADR-024: Use one SessionConfig and keep hashes local to their actual purpose](2026-09-04-02-simple-config.md); [ADR-025: Store transcript state and public Events in two per-session logs](2026-09-04-03-two-logs.md); [ADR-026: Recover saved data by loading it, without replaying the Agentloop](2026-09-04-04-load-recovery.md); [ADR-029: Declare machine-readable Events and failures once](2026-09-04-07-codes.md).

<a id="session-b8149887-7a5f-45fc-a05a-d0a4816508d5"></a>

### Claude `b8149887-7a5f-45fc-a05a-d0a4816508d5`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/b8149887-7a5f-45fc-a05a-d0a4816508d5.jsonl`.

- 2026-09-03T23:45:13.084Z, user message, JSONL line 5: User requests real Pi/OpenCode/Codex comparisons.

Used by: [ADR-039: Keep performance claims historical until representative baselines are rebuilt](2026-09-05-08-benchmark-policy.md).

<a id="session-754faa6b-5689-4155-a619-39f11a52bf6e"></a>

### Claude `754faa6b-5689-4155-a619-39f11a52bf6e`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/754faa6b-5689-4155-a619-39f11a52bf6e.jsonl`.

- 2026-09-04T12:17:22.335Z, user message, JSONL line 144: User asks why handwritten protocol duplicates and unused digests exist.
- 2026-09-04T12:36:50.480Z, user message, JSONL line 184: User explicitly chooses Rust-first generation and concise AGENTS.md guidance.

Used by: [ADR-031: Generate public contracts from Rust types and route annotations](2026-09-04-09-rust-contracts.md).

<a id="session-eb81e1ec-79e7-490f-9797-a836946f8d2f"></a>

### Claude `eb81e1ec-79e7-490f-9797-a836946f8d2f`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/eb81e1ec-79e7-490f-9797-a836946f8d2f.jsonl`.

- 2026-09-04T13:28:22.843Z, user message, JSONL line 70: User highlights compaction calls that can wait seconds or minutes.
- 2026-09-04T13:33:27.505Z, user message, JSONL line 87: User asks for arbitrary HTTP calls subject to an explicit allowlist.

Used by: [ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md); [ADR-034: Run native Components in a separate capability-restricted Wasmtime worker](2026-09-05-03-wasm-worker.md).

<a id="session-01a06cea-6715-7f33-826c-4ee3ed98b5eb"></a>

### Codex `01a06cea-6715-7f33-826c-4ee3ed98b5eb`

Local source: `~/.codex/sessions/2026/09/04/rollout-2026-09-04T15-54-56-01a06cea-6715-7f33-826c-4ee3ed98b5eb.jsonl`.

- 2026-09-04T16:30:42.764Z, user message, JSONL line 676: User separates extension authoring from application configuration and asks for explicit env factory arguments.
- 2026-09-04T16:58:12.297Z, user message, JSONL line 1031: User weighs authoring simplicity, language/runtime inference, dependency installation, and deterministic preparation.
- 2026-09-04T17:23:08.894Z, user message, JSONL line 1177: User distinguishes app-resident DOM/database functions from packaged tools placed in a chosen Environment.
- 2026-09-04T19:17:52.232Z, user message, JSONL line 1863: User chooses one small native Wasm path and rejects a second native MicroVM product.
- 2026-09-04T20:13:40.128Z, user message, JSONL line 2460: User confirms Brain runs compiled Wasm rather than JavaScript/Python/Rust source runtimes.
- 2026-09-04T21:41:54.639Z, user message, JSONL line 3170: User selects no runtime retries and lets the Agentloop/model decide another action.
- 2026-09-04T22:06:36.612Z, user message, JSONL line 3280: User chooses ctx.emit as the general event operation.
- 2026-09-04T22:45:14.029Z, user message, JSONL line 3651: User rejects extra acceptance/delivery/digest protocols for the MVP.

Used by: [ADR-007: Compose sessions with typed extension factories](2026-08-27-01-typed-authoring.md); [ADR-022: Let Environments execute implementations using their own platform APIs](2026-09-02-05-resources.md); [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md); [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md); [ADR-034: Run native Components in a separate capability-restricted Wasmtime worker](2026-09-05-03-wasm-worker.md); [ADR-035: Send each effect once and expose failures to the Agentloop](2026-09-05-04-send-once.md).

<a id="session-01a0705d-503e-7a53-a65f-a1e9ca13e23c"></a>

### Codex `01a0705d-503e-7a53-a65f-a1e9ca13e23c`

Local source: `~/.codex/sessions/2026/09/05/rollout-2026-09-05T07-59-18-01a0705d-503e-7a53-a65f-a1e9ca13e23c.jsonl`.

- 2026-09-05T07:19:00.079Z, user message, JSONL line 37: User defines distributed execution and prioritizes startup, memory, CPU, and cheap suspend/resume.
- 2026-09-05T08:19:20.691Z, user message, JSONL line 273: User separates ephemeral execution from journal recovery, asks for cold history reads and pre-create artifact preparation, and leaves resource lifecycle to providers.
- 2026-09-05T08:49:17.093Z, user message, JSONL line 348: User accepts the proposed process-and-OS/power-failure local guarantee and defers external commit services.
- 2026-09-05T08:52:25.865Z, user message, JSONL line 373: User accepts interrupted-turn Events and leaves the next activation to the user.
- 2026-09-05T09:01:13.831Z, user message, JSONL line 442: User accepts dynamic bindings and optional automatic placement as future work.
- 2026-09-05T09:05:01.771Z, user message, JSONL line 467: User explicitly defers the whole tool-env tool and retains required MVP bindings.
- 2026-09-05T09:08:02.133Z, user message, JSONL line 500: User accepts the final decision list and requests an implementation plan.
- 2026-09-05T11:24:41.570Z, user message, JSONL line 1974: User explicitly defers benchmark leakage checks during rapid iteration.
- 2026-09-05T13:20:11.466Z, user message, JSONL line 2919: User explicitly selects brain-data/1 instead of /2.
- 2026-09-05T13:25:53.678Z, user message, JSONL line 3018: User requests regular-user SDK journeys with parallel CI execution.
- 2026-09-05T14:17:10.872Z, user message, JSONL line 3453: User moves roadmap content to ROADMAP.md and selects the current product headline.

Used by: [ADR-030: Replace pre-1.0 contracts and development data in place](2026-09-04-08-clean-break.md); [ADR-032: Use one canonical journal and commit before exposing records or effects](2026-09-05-01-canonical-journal.md); [ADR-035: Send each effect once and expose failures to the Agentloop](2026-09-05-04-send-once.md); [ADR-036: Preserve request claims and effect uncertainty across interruption](2026-09-05-05-interruption.md); [ADR-037: Release execution at turn boundaries and prepare artifacts before creation](2026-09-05-06-ephemeral.md); [ADR-038: Defer dynamic placement, workflow durability, and advanced tenancy policy](2026-09-05-07-roadmap-boundary.md); [ADR-039: Keep performance claims historical until representative baselines are rebuilt](2026-09-05-08-benchmark-policy.md).

<a id="session-9bed54a9-c1f9-48eb-aba8-de8bcbcbe55f"></a>

### Claude `9bed54a9-c1f9-48eb-aba8-de8bcbcbe55f`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/9bed54a9-c1f9-48eb-aba8-de8bcbcbe55f.jsonl`.

- 2026-09-06T11:36:25.433Z, user message, JSONL line 223: User asks whether the 128-key kv cap is arbitrary.
- 2026-09-06T11:42:37.985Z, user message, JSONL line 245: User deletes the cap because Brain does not know the machine hosting it and should not set such limits.
- 2026-09-06T11:46:06.394Z, user message, JSONL line 277: User asks for a complete review of similar limits; the assistant's sweep is the inventory ADR-043 starts from.

Used by: [ADR-043: Make every limit a deployment default injected at server start](2026-09-06-02-deployment-limits.md).

<a id="session-18c350ff-58c3-400d-b851-0877627b8a8d"></a>

### Claude `18c350ff-58c3-400d-b851-0877627b8a8d`

Local source: `~/.claude/projects/C--Users-luowe-workspace-aex-workspace/18c350ff-58c3-400d-b851-0877627b8a8d.jsonl`.

- 2026-09-06, user message, JSONL line 1: User asks that every limit be injected at the brain server level as a default overridable by environment variable, and requests an ADR.
- 2026-09-06, user message: User confirms all four open positions, adding one shared environment-reading component and that model limits are not Brain's to configure.

Used by: [ADR-043: Make every limit a deployment default injected at server start](2026-09-06-02-deployment-limits.md).
