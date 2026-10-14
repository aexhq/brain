# ADR-053: Select one upstream agent harness at Brain startup

- Decision date: 2026-10-14
- Status: Amended by [ADR-056](2026-10-14-02-registered-agentloops.md): register multiple implementations and select a name per session.

Amends [ADR-023](2026-09-04-01-turn-services.md),
[ADR-033](2026-09-05-02-placement.md),
[ADR-037](2026-09-05-06-ephemeral.md), and
[ADR-041](2026-09-05-10-one-execution-model.md) for agent execution.
Tool placement, Environment execution, and journal barriers continue to apply.

## Context

Maintaining Pi-like and Codex-like loops duplicates upstream context management,
tool scheduling, and compaction. Before launch, preserving those implementations
and their session-selected executable contracts costs more than a clean break.
The user authorized a startup-selected implementation, upstream compaction,
reduced feature coverage, and no compatibility with retained prelaunch sessions.

## Decision

Require one harness when starting Brain. Clients create sessions with models,
Tools, Environments, and harness options; they cannot admit or select executable
agent code. Remove the Agentloop admission endpoints, authoring WIT, runtime
loader, and manual reference implementation. Wasmtime continues to execute Tools.

The Rust core accepts an injected `AgentHarness`. The server launches a trusted
deployment command and speaks versioned, bidirectional JSON-RPC over stdio.
The lifecycle is initialize, open, run, cancel, close, delete, and shutdown.
TypeScript authors can implement the interface with `serveHarness`; other
languages implement the same generated wire types.

Bundle adapters for actual upstream implementations:

| Harness | Pinned implementation | Integration |
| --- | --- | --- |
| Pi | coding-agent and pi-ai 1.2.0, TypeScript | Complete AgentSession SDK, custom model provider and Tools |
| Codex | 0.162.1, Rust | App-server process, dynamic Tools, loopback Responses provider |
| OpenCode | SDK 2.0.28, TypeScript on Bun | Embedded SDK, LanguageModelV3 and Tool hooks |

Upstream owns native storage, context decisions, and compaction. Brain's transcript
is a journaled projection, not an upstream checkpoint. Upstream packages are
optional SDK peers; the server image installs the pinned versions and runtimes.
Codex dynamic Tools remain an experimental upstream API: pin upgrades and run
real-runtime conformance tests instead of claiming an upstream stability guarantee.

Every model and Tool call awaits Brain's existing intent and terminal journal
barriers. Callback authority is scoped to the active session and sequence. Fatal
journal, cancellation, budget, or ambiguous callback errors stop the activation,
even if upstream catches the exception. Disable upstream transport retries,
built-in tools, ambient extensions, and direct model transports.

Before native activation, durably record a reserved KV marker; remove it only
after successful completion. A retained marker refuses native resume. Upstream
automatic recovery may otherwise retry work after Brain has dispatched it but
before the upstream checkpoint records the result. Completed turns can resume;
failed, cancelled, or crashed native activations require a new session. A dead
harness process fails readiness and is not automatically restarted.

Persist harness identity and state version beside native state, refusing a
different implementation over the same directory. Back up the entire directory.
Closing releases resources; deleting removes native state and Brain history.

## Alternatives

- Reimplement each upstream loop: rejected because maintenance was the problem.
- Use only an in-process TypeScript API: rejected because Codex is Rust and custom
  harnesses should not inherit Brain's or one upstream's implementation language.
- Run unmodified agent CLIs with observer hooks: rejected for the mediated adapter
  contract. Observation after dispatch cannot enforce Brain's journal barrier.
- Reconstruct upstream state from Brain's transcript: rejected because native
  checkpoints contain control and compaction state beyond visible messages.
- Keep every old feature: rejected. Bundled Codex and OpenCode support text input;
  Pi supports canonical URL media. Bundled adapters require fixed Tool placements.
  Custom harnesses can implement additional capabilities through the same services.

## Consequences and verification

Adapters still translate provider and Tool formats; importing upstream cannot
eliminate that boundary. Upstream upgrades must pass tests using their real
runtimes, including compaction, persistence, cancellation, terminal barriers,
swallowed errors, opaque continuation blocks, and unoffered Tools.

Independent Python tests exercise the Rust stdio host. SDK protocol tests cover
concurrency and cancellation. Linux journeys exercise actual servers, workers,
native Pi, host and remote Tools, background observations, and interrupted state.
Existing platform, image, journal recovery, and real-media release gates remain.

The release is a prelaunch contract replacement and requires a new data directory.
First-party extension packages become small exports of these adapters instead of
maintaining their own agent logic.

## Sources

- [Pi coding-agent SDK](https://github.com/earendil-works/pi/tree/main/packages/coding-agent)
- [Codex app-server](https://github.com/openai/codex/tree/main/codex-rs/app-server)
- [OpenCode SDK](https://github.com/anomalyco/opencode/tree/dev/packages/sdk)
- Exact dependency versions and integrity hashes in the root lockfile.
- User decisions in this task and the [implementation plan](../changes/2026-10-14-startup-harness.md).
