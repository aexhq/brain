# Registered agentloops

Brain registers named agentloop executables at startup. Sessions select an installed
name and supply behavior configuration; requests cannot install or replace code.
Pi, Codex and OpenCode adapters and their upstream dependencies belong to extensions.

Implementation and release:

1. Add the named session selection and startup registry, with configurable worker
   counts and active-run limits. Keep each open session on its assigned worker.
2. Preserve durable callbacks, per-session serialization, native state and the rule
   that uncertain effects are never retried. Reject unknown registrations at create.
3. Move upstream adapters, configuration types and conformance tests to extension
   packages. Client helpers return selection data; executable entry points serve the
   neutral Brain protocol. Ship an extension image composing the public Brain image.
4. Migrate SDK examples, fixtures and documentation together. Test mixed concurrent
   sessions, worker isolation, capacity, close/resume and deletion after restart.
5. Run all local and CI gates, release immutable Brain artifacts, then the matching
   extensions and composed image. Update the public documentation deployment.

This replaces the single-harness prelaunch data layout. Use a new data directory;
existing histories must not be deleted to make an upgrade pass.
