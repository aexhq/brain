# Startup harness refactor

Brain will run one agent harness selected by the operator at startup. Applications
create sessions with models, tools, and Environments; they cannot upload or select
agent executable code. Harnesses own their native session state, context management,
compaction, and agent behavior. Brain retains durable model and tool execution.

## Implementation plan

1. Replace session-selected Agentloop implementations with a startup-injected
   `AgentHarness`. Define the language-neutral lifecycle and callback contract in
   Rust and generate its TypeScript bindings. Use bidirectional JSON-RPC over stdio.
2. Add the server process adapter and TypeScript harness authoring API. Scope every
   callback to an active session activation. Await Brain's existing journal barriers
   before returning model or tool results. Stop on transport loss or storage failure;
   never replay a submitted effect.
3. Import upstream agent SDKs in first-party adapters. Start with Pi's complete agent
   session SDK; cover OpenCode's provider/tool hooks and Codex's app-server/provider
   boundary with the same conformance requirements. Keep compaction and native
   persistence upstream. Do not reimplement their agent loops.
4. Remove public per-session Agentloop admission and placement. Migrate SDK examples,
   documentation, fixtures, and downstream extension packages. Retain Environment
   execution for tools. Native harness state is opaque and is not reconstructed from
   Brain's transcript projection.
5. Verify startup selection, session isolation, native resume, cancellation, callback
   ordering, uncertain outcomes, and rejection of non-admitted tools. Exercise real
   upstream SDKs with deterministic model fixtures. Run the repository's full local
   checks and Linux/platform/HTTP/image gates in CI.
6. Push and merge the reviewed implementation after CI succeeds. Tag the exact
   candidate, stage immutable SDK and image artifacts, run real media integration at
   that commit, and promote the tested artifacts. Release affected extension packages
   against the immutable Brain version and verify their publication.

## Contract decisions

- A deployment provides the harness command and arguments before readiness. The
  protocol negotiates a version and harness identity; stdout carries protocol frames
  and stderr carries diagnostics. Other implementation languages are supported.
- One active run per session. Different sessions may run concurrently. Harness
  callbacks use Brain's model and tool services; the adapter exposes no upstream
  built-in tools or direct model credentials.
- User input and committed background events enter the harness. It opens or resumes
  native state, runs, publishes transcript projections, and closes/cancels explicitly.
- Model and tool failure outcomes are returned only after Brain records them. Failure
  to commit an intent or terminal record is fatal to the activation. No observer hook
  is treated as an execution barrier.
- Native state stays under the deployment's persistent data directory. Brain does not
  promise cross-harness checkpoint portability or transparent recovery of a run that
  was interrupted after an external effect.
- Adapter-specific configuration remains opaque. Unsupported upstream features fail
  explicitly; they do not fall back to unmediated execution.

The user authorized this clean break, implementation, push, and release on
2026-10-14. Backward compatibility with prelaunch Agentloop APIs and stored sessions
is not required.
