# @aexhq/loop-codex

The official codex-style agentloop for Brain sessions: a semantic port of codex's loop
policies (environment preamble, strictly sequential tool execution, loop-owned conversation
memory, a summary mark per turn) onto `contracts/agentloop/v1`. See
`aex-research docs/codex-loop-semantics.md` for the port's decision record.

This package is built exactly like any external loop: `build.mjs` calls the public
`buildLoopBundle` from `@aexhq/agentloop` and emits

- `dist/loop.bundle.mjs` — the deterministic source bundle (what uploads or seeds), and
- `dist/identity.json` — the sealed identity `{ name, version, toolchain,
  source_bundle_sha256, bytes }`.

Compositions seed it through the same admission path customer uploads take; there is no
private build or load path.
