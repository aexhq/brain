# @aexhq/loop-pi

The official pi agentloop for Brain sessions: the pinned `@earendil-works/pi-agent-core`
driving turns, adapted onto `contracts/agentloop/v1`. The kernel owns provider execution,
custody and journaling; pi owns the policy.

This package is built exactly like any external loop: `build.mjs` calls the public
`buildLoopBundle` from `@aexhq/agentloop` and emits

- `dist/loop.bundle.mjs` — the deterministic source bundle (what uploads or seeds), and
- `dist/identity.json` — the sealed identity `{ name, version, toolchain,
  source_bundle_sha256, bytes }`.

Compositions seed it through the same admission path customer uploads take; there is no
private build or load path. The StarlingMonkey compatibility rewrites for pi's dependency
tree live in `src/compat.mjs` and ride the public `plugins` hook.
