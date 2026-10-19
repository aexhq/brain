# 12 Service integration readiness

Implements [ADR 0004](../adr/0004-service-integration-readiness.md), accepted on 2026-10-19. Slices 1–3 are implemented; continue with slice 4, then follow the dependencies below. The initial browser path is a same-origin gateway with SSE/HTTP; usage may be explicitly unknown; aggregate pool controls and hard OS limits are off by default. Language-specific SDK signatures are implementation choices within the ADR's semantics. Code and tests own implemented behavior; the remaining slices below describe outstanding work.

## Session handoff

Work in `C:/Users/luowe/workspace/aex_wksp/brain` for Brain changes. Read `AGENTS.md`, ADR 0004 and this plan first; ADR 0004 amends ADR 0003. Code and tests own the baseline. Use the existing [harness probes](../research/harness-probes.md) and [adapter plan](03-harness-adapters.md) for native integration details.

The dependent Aex checkout is `C:/Users/luowe/workspace/aex_workspace/aex`; its `docs/adr/011-brain-2-integration.md` owns gateway, product-data, billing, deletion and cutover work. Complete the neutral Brain prerequisites before switching Aex. Preserve one implementation owner for each behavior and make no backward-compatibility layer.

## Work sequence

| Slice | Changes | Completion evidence |
| --- | --- | --- |
| 1. Authoring and private admission — complete | Model/tool constructors for server and client roles; private model credentials alongside existing tool overrides; application authenticator; generated contracts and TS/Python clients | `models_test.go` covers startup defaults, client-only keys, authorization, tenant-scoped references, encrypted restart/cold restore, rotation and revocation. Installed SDK tests and Pi journeys cover authoring, overlapping independent keys, passive recovery and no plaintext in local/native/archive state. |
| 2. Common host ownership — complete | Shared acquisition, attachment reservation, drain and cleanup across installed process factories; optional aggregate ceilings with no new default limit | `internal/harness/pool_test.go` covers mixed factories, startup/host/turn budget rejection and release, drain, and cleanup. Existing shared-host and ownership tests retain occupancy, cancellation and crash coverage. Installed-client journeys cover default and capped composition across factories and released capacity. Full Windows checks pass. |
| 3. Model-request evidence — complete | Acknowledged observations, retained request/attempt snapshots, native-boundary coverage gaps, unknown recovery outcomes and generated client types | Protocol journeys cover persistence before dispatch, primary/compaction/other purposes, repeated and partial snapshots, unknown usage, omitted outcomes, stopped hosts and passive cold recovery without redispatch. Core tests retain partial usage when outcomes become unknown and reject invalid accounting evidence. Full Windows checks pass; real-adapter observation coverage belongs to slices 4–6. |
| 4. Pi coverage � complete | One native stream observation seam for ordinary generations, compaction and retries; session-local credentials and HTTP-attempt evidence | Recording-provider journeys verify completions/Responses cache and reasoning semantics, retries, rejection, partial streams, cancellation, abrupt loss, and private-key automatic compaction followed by passive cold restore. Existing exact-catalog, peer and tool journeys pass with observations enabled. Full Windows checks pass; [qualification](../research/qualification.md#pi-request-evidence-2026-10-19) separates local coverage from live billing evidence. |
| 5. Codex adapter | Owned App Server, dynamic tools, pinned exclusion settings, passive native restore, incremental checkpoints and request/usage mapping | Full conformance with two credential contexts, shared-host qualification where native scope allows it, no startup inference, compaction recovery and recorded accounting gaps/upstream changes. |
| 6. OpenCode adapter | Owned embedded host, exact tools, suppressed startup continuation, session-scoped incremental state and model-request observation | Full conformance; no whole-database copy per turn, native retry/auxiliary/WebSocket coverage, independent keys and no auto-resume after abrupt death. |
| 7. Browser integrations | Attached-tab tool and external remote-browser provider; scoped browser authentication hooks; selected first gateway SSE/HTTP journey | Real browser plus remote service journeys, no management/model keys in the page, explicit closed-tab behavior, origin/expiry/revocation and no redispatch. |
| 8. Integration release | Neutral deletion and single-owner requirements retained; update all artifacts/examples together; relocate the inspection-tool provider to platform integration | Fresh-state cutover, complete required CI/provider checks, installed-package service journey and matching published contracts. |

Usage contract work precedes final adapter qualification so three implementations do not invent three incompatible receipt formats. Browser work can proceed independently after private admission and callback authority are specified. Hard OS limits are a separate launcher slice only if a deployment needs them; do not make them a prerequisite for the common pool.

## Implementation entry points

- Authoring: `sdks/typescript/src/index.ts`, `runtime.ts`, Python client equivalents, `server.go`, public provider types and `integrations/providers.go`.
- Authority and credentials: `internal/api/auth.go`, `server.go`, `internal/core/types.go`, `internal/session/credentials.go`, `internal/credentials` and configured provider resolution.
- Host ownership: `internal/harness/process.go`, `ownership_linux.go`, `ownership_windows.go`, `internal/session/engine.go`; preserve the existing protocol and native-state owner rather than introducing a second scheduler.
- Request evidence: `internal/harness/harness.go`, `internal/core/types.go`, session event persistence and `runners/pi/src/main.ts`; new adapters use the same path.
- Browser transport: API/gateway origin handling and callback scopes, `internal/api/executors.go` and client runtime transports. Remote browser service code remains an integration, outside the harness server.

Choose the smallest change after exercising each native seam. Add no automatic prewarming, usage-based autoscaler, universal queue, public pool API or container library without a concrete consumer. Record the chosen failure and response before adding a limit or check.

## Accounting qualification matrix

Run each supported model/provider transport through normal success, multiple tool-driven generations, compaction, provider retry, request rejection, partial output then disconnect, cancellation, host crash and checkpoint recovery. Record which native calls and physical attempts were observable. A missing quantity stays unknown; never label a schema field or a local fixture as live-provider billing qualification.

Retain public native request identifiers only when supplied. Treat cumulative snapshots and final message usage as alternative observations of one request, not additive charges. Verify each provider's cache/reasoning inclusion rules against the pinned adapter before exposing normalized totals.

## Delivery

Run the repository's complete required checks for changed runtime components and selected integrations; missing provider credentials leave their evidence blocked. Documentation-only changes use documentation validation. CI remains the release gate. Do not disable old tests as a shortcut: port supported behavior to the new contract and remove retired behavior only with the selected clean scope change.

Slice 1 passed `node scripts/check.mjs` on Windows, including generated contracts, installed SDK packages, Go tests/vet, Pi and service tests, journeys, platform/release tests, and the documentation build/tests. The strengthened overlapping-key and plaintext-state checks also pass through the installed-client journey suite. This is recording-provider evidence; CI and live-provider qualification retain their separate gates. Pi's compaction/retry credential coverage remains slice 4.
