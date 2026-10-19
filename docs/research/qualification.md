# Integration evidence

Reviewed 2026-10-15. Local tests and live qualification are separate release evidence.

| Integration | Evidence | Remaining limits |
| --- | --- | --- |
| AWS S3 archive | Live SDK object contract and full archive contract passed in eu-west-2: immutable writes, mutable pointer, cancellation, bounds, missing object, append efficiency, interrupted publication, cross-tenant rejection, restore into an empty directory, older checkpoint restore. Temporary bucket removed after the run. | This qualifies the tested standard bucket/IAM setup, not every S3-compatible service or policy. |
| R2 archive | Real AWS SDK transport fixture passes with R2 configuration. | No reusable R2 credentials available for a live run. |
| GCS archive | Official SDK multipart, precondition, read and shared object contract tests pass. | No ADC credentials available for a live run. |
| MCP | Official SDK client/server fixture: discovery, streamed progress, errors, structured content, tenant isolation, bounds, cancellation and no action replay. | No third-party server account qualified. |
| Modal, E2B, Daytona | Pinned SDK type checks; HTTP tool journey; signed handle ownership and restart; bounded remote helper; cancellation and cleanup fixtures. | Provider account secrets required for `npm --prefix services/sandbox run live -- PROVIDER`. |
| Resource tools | AWS SDK + HTTP registry journey; auth, tenant prefixes, read-only policy, delete; Brave/Tavily request fixtures; public-network and HTML extraction tests. | Live search/R2 resource-tool accounts remain unqualified. |

Run storage qualification with a dedicated bucket/prefix, `BRAIN_TEST_STORAGE_CONFIG` pointing to an absolute settings JSON file, and `go test -tags cloud -run TestCloudStorage -count=1 -v ./internal/persistence`. The explicit cloud profile fails if configuration or credentials are missing. Operators own cleanup of its isolated prefix after the run.

The Browser checks found an existing Modal token ID, but its secret is not retrievable from that page. E2B required a new GitHub OAuth grant, which was not authorized during browser automation. No new browser credentials or account permissions were created.

## Pi request evidence, 2026-10-19

The pinned Pi 1.2.0 runner uses its native stream seam for generations, automatic compaction and native retries. Each logical call is acknowledged by Brain before native dispatch; an injected HTTP fetch records physical attempts before network dispatch. No inference proxy or process-global credential switch is used.

Installed-client journeys against local recording providers cover OpenAI completions and Responses with explicit input/output, cached-input and reasoning breakdowns. Pi's normalized input excludes the reported cache quantities; output includes reasoning. Replacement snapshots preserve those relationships without additive duplicate charges. Absent provider usage stays unknown even when the native message initializes its counters to zero.

Additional journeys exercise HTTP rejection, retries, partial output followed by disconnect, cancellation, abrupt process loss, independent customer keys, native automatic compaction with a private key, and the next request after passive cold restoration. The existing tool-driven generations, peer cancellation and host ownership suites run with observations enabled. Native state, SQL, archives, public events and logs are checked for direct-key leakage in the independent-key journey.

This qualifies the tested local transports and request paths, not a live provider invoice. Physical attempts remain explicitly unobserved on transports that do not use the fetch hook; quantities remain unknown when the pinned native stream hook cannot establish provider reporting. Other model/provider transports require their own recording and live-provider qualification before claiming the same billing coverage.

## Codex adapter, 2026-10-19

The adapter pins App Server `0.163.0` and uses native threads, dynamic tool callbacks and complete native JSONL rollouts. Its exclusion settings retain the earlier exact-catalog probe controls. Local conformance journeys exercise empty/exact catalogs, an unbound native call, concurrent peers, independent keys across tenants, encrypted cold recovery, incremental checkpoints, cancellation, native automatic compaction and the next request after compaction recovery. The owned-host test kills a real shared native host with two active threads and verifies both fail and a reopened host does no inference.

Two native lifecycle seams required explicit policy. Paginated history rejects relocated paths and relies on a host-local index, so adapter-created threads select the pinned server's `historyMode: "legacy"` complete rollout format. This introduces no Brain format version or compatibility importer. `thread/unsubscribe` did not establish prompt residency release in the probe; `thread/archive` followed by `thread/read` reports `notLoaded` and preserves an independent Brain checkpoint. Duplicate interruption requests share one native operation to avoid a completed-turn race affecting peers.

The official [App Server documentation](https://developers.openai.com/codex/app-server) describes the lifecycle; the pinned binary's generated experimental schemas and recording-provider tests own the actual selections. Native token updates are session-cumulative snapshots. The adapter retains positive differences between settled-turn counters as partial evidence with cache/reasoning inclusion semantics; it does not claim visibility of every physical retry or compaction request. Receipts explicitly mark the unmeasured paths inside a native-turn boundary. This is local conformance evidence, not live-provider billing qualification.

## OpenCode adapter, 2026-10-19

Pinned SDK/core/plugin 2.0.30 and Effect 4.0.1 pass the native recording-provider journeys: exact and empty catalogs, external callbacks, independent keys across tenants in one host, peer cancellation, failed requests and retries, incremental checkpoints, automatic summary compaction, and the next request after passive cold restoration. Owned-host tests terminate concurrent native sessions and verify lazy replacement without inference. The complete Windows check passes.

The embedding override suppresses SessionRestart continuation. Session instance registries scope tools and provider routing; model hooks scope credentials. HTTP hooks observe primary and summary requests, streaming and non-streaming responses, physical retries, and raw usage. HTTP is explicitly selected; WebSocket handshakes are rejected. Title inference is suppressed. No native database is copied into checkpoints; session-export reads grow with history while checkpoint writes append changed records. The Node 24 runtime passes despite a transitive unused UI package declaring Node 26.4. The native platform dependency and Effect shared package require explicit pins to avoid incompatible transitive resolution.

This is local transport and lifecycle evidence. Live model-provider billing remains unqualified; unknown quantities and ambiguous outcomes stay explicit.

## Browser integrations, 2026-10-19

Real Chromium journeys use the installed TypeScript package through the same-origin gateway: a gesture attaches an ordinary page function, SSE carries calls and events, duplicate replies are rejected, closing a page marks dispatched work unknown, and revocation/rebind never replays late results. Negative journeys cover origin, descriptor and composition restrictions, scoped routes, expiry and absence of management/model keys in the page. The external browser journey covers navigate, fill, click, screenshot, close and tenant-scoped state.

Service tests exercise subresource/redirect network policy, authorized/denied HTTPS CONNECT tunnels, context capacity and idle cleanup, overlapping-action rejection and cancellation without redispatch. Native route hooks alone did not stop redirected egress; an owned HTTP proxy now enforces the exact origin policy. Full Windows checks and targeted real-browser checks pass. The non-root Linux browser image builds and runs its tests. The main Linux image completes inference with all three packaged harnesses. This is local evidence, not qualification of live authenticated websites or provider billing.

## Coordinated integration verification, 2026-10-19

Each implementation slice passed `node scripts/check.mjs` on Windows. The final suite includes installed npm/wheel/source consumers, generated-contract checks, Go tests/vet, three native harness journey selections, real Chromium, tool services, deployment/release checks and documentation builds/tests. The journey command reports 66 checks with no skips. Native callback regressions additionally require rejected start acknowledgements and Brain dispatch failures to stop the affected loop before another model dispatch while a compatible peer remains usable; ordinary external tool errors return to the model once. Additional independent-key scans inspect Codex/OpenCode owned native homes, databases, checkpoints and archives as well as public events/logs; no direct key appears in plaintext.

The final Linux source passes `go test -race -tags postgres -count=1 ./...` against PostgreSQL 18, including native ownership/callback tests and the relocated platform inspection policy. The packaged non-root Brain image passes `node scripts/container-smoke.mjs` with Pi, Codex and OpenCode. Sandbox image build/smoke and the browser image's three real network/lifecycle tests also pass. The three-harness example passes startup validation without creating local state.

Remote CI/CodeQL and publication have not run for these local commits. They remain the release gate. No live model-provider invoice, authenticated third-party browser account or new cloud-provider credential is qualified by the local fixtures. Aex cutover, commercial accounting, ownership placement and physical deletion/retention remain platform work. The release uses fresh state and has no old-format importer or compatibility routing.
