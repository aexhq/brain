# Verification and release implementation plan

Status: Draft  
Applies to every release gate in the [roadmap](../roadmap.md)

CI is the release gate. Tests must establish the guarantees advertised by that release, including failure behavior. Do not skip required checks or treat missing provider credentials as a successful integration test. Future adapters do not require tests before implementation, but an advertised adapter must have its required suite enabled.

Implemented local coverage: the [user journey suite](../../tests/journeys/README.md) exercises the actual server, Pi runner, external example tools, and both SDKs on Windows and Linux. Its coverage map distinguishes local guarantees from the later release gates below. `node scripts/check.mjs` runs the journeys together with the existing checks; unavailable prerequisites fail the suite.

| Work item | Implementation | Acceptance |
| --- | --- | --- |
| V1 Repository checks | Add builds for supported platforms, formatting/static checks, generated-contract verification, and focused unit tests for state transitions and boundary validation. Set up protected CI for integration credentials. | A clean checkout reproduces the checks. Generated contracts cannot drift silently. A supported platform or required credentialed check cannot be absent from release evidence. |
| V2 Contract and integration suites | Reuse harness, storage, tool, and API contract tests across implementations. Add real Pi smoke coverage initially; add actual S3/R2/PostgreSQL and other harness/provider checks when advertised. | Both deterministic failure cases and real integration boundaries pass. Every selected adapter runs its required contract suite. |
| V3 Failure and isolation | Exercise process death, interrupted writes/uploads, ambiguous tool outcomes, tool waits, distinct fatal errors, forbidden scope, malformed input, slow clients, queue/disk exhaustion, and shutdown deadlines. | Results match tail-loss, error, isolation, and admission contracts. No test requires hidden strong durability or automatic replay of uncertain mutations. |
| V4 Performance and recovery | Add reproducible local benchmarks, multi-instance load/soak jobs, real-harness resource measurements, and a fresh-infrastructure restore drill. Store reports with the candidate build. | Capacity statements identify workload/topology and measurable results. Restore succeeds from the claimed checkpoint without relying on old caches. Long sessions stay within configured resource budgets. |
| V5 Release assembly | Build artifacts identified by source revision, package pinned runner dependencies, record the project license/dependency notices, run current client examples, and publish limitations and clean-break notes. | All required suites pass for the exact candidate and dependency set. No compatibility/migration promise is introduced by release naming or dependency pinning. |

| Failure or behavior | Required assertion |
| --- | --- |
| Process dies after local acceptance but before external sync | Surviving local state may restore; a lost disk may lose the unsynchronized tail. External restore uses a complete checkpoint and never fabricates missing history. |
| Crash leaves an incomplete final transcript record | Recover only a proven valid prefix. Interior corruption, including damaged framing followed by intact records, must not be silently treated as a torn tail. |
| Segment/native upload succeeds but publication is interrupted | An incomplete checkpoint is not advertised as usable; retry is idempotent and a previous complete checkpoint remains available. |
| Transcript upload is ahead of the native checkpoint | Status and flush responses distinguish uploaded history from the actual resume boundary. |
| SSE subscription races with new events or a client reconnects | Retained events are not missed; duplicate events keep their identity. A lost-tail cursor returns a typed reset/recovery response. |
| Tool performs a mutation but its result is not received | Deliver an unknown-outcome tool result where possible. Brain does not replay the invocation or pretend cancellation rolled it back. |
| Tool denies approval, fails, times out, or returns invalid data | Report the tool failure through the adapter's model-visible result path; Brain does not decide the next model action. Business fields cannot impersonate protocol status. |
| Brain local append or runner protocol fails | Halt affected execution through the control path even if the harness catches ordinary tool exceptions. |
| All active runs wait for tools | Interruption, inspection, result delivery, and unrelated-session progress remain possible; no permit/lock deadlock. |
| Idle time, a quiet tool wait, health checks, or SSE keepalives | No synthetic model request or context message is generated. Exercise upstream cache-warming timer paths with a recording provider and controlled settings. |
| Native state is loaded after an unclean shutdown | Restore alone invokes no model or tool, including upstream automatic startup recovery paths. Lost work is not silently resumed. |
| Queued input or steering arrives during a tool wait | Delivery matches the advertised native boundary. Unsupported steering rejects explicitly; it is not silently queued or simulated by interrupt/restart. |
| A rejected input is retried with the same key | Definite non-admission has not consumed the key. Accepted-event identity and actual completion/failure remain distinct. |
| A late, duplicate, or forged tool result arrives | It cannot become system/internal control or satisfy another call after rewind/restore. |
| Tenant/session IDs or artifact references are forged | API, inspection tools, resource reads, and execution bindings deny access outside authenticated grants. |
| A native harness tool bypasses remote bindings | The configuration fails or the bypassing tool is disabled. Brain never silently executes it locally or adds an approval gate. |
| A dependency, browser, or telemetry exporter disconnects | Execution/tool status is explicit; retries and buffers remain bounded; telemetry failure alone does not stall sessions. |
| Disk, subscriber buffers, or sync backlog fill | New admission stops before silent loss of canonical data. Cancellation and completion still have bounded capacity. |
| Native state restores but its next request changes meaning | Fail conformance for lost branch/compaction, call-result identity, media, or required prompt state. Tests use recorded requests, not a live prompt-cache hit. |
| Long sessions append, compact, and rewind | Prior records remain immutable; total transcript writes grow with new data. Measure native checkpoint/upload amplification separately. Rewind does not undo remote effects. |
| A tool references missing remote state | The tool reports its failure; Brain does not claim to restore external jobs or browser/compute resources. |

Use deterministic fixtures for ordering, clock/deadline behavior, cancellation, and injected failures. Use real SDK/harness/storage boundaries to detect protocol and provider mismatches. Do not use one kind of test as evidence for the other. Prefer assertions about observable behavior and failure contracts over tests that reproduce implementation details.

When configuration updates are implemented, require tests for authorization, unsupported fields, idle/busy boundaries, application ordering, and partial adapter failure. Verify that changing settings alone starts no inference; earlier requests retain their tool definitions/bindings; restore pairs the correct settings with native state; explicit rewind retains current settings; and a shared runner cannot leak a change to another session. These are future feature gates, not extra R0 scope.

Integration checks that need secrets run in protected CI; untrusted pull-request code must not receive those secrets. Before release, the protected checks must pass for the candidate revision. Missing credentials, unavailable required providers, or disabled required checks leave the release blocked. Document setup and cleanup for provider tests so failures do not leave unbounded paid resources running.

Run the complete required suite for the release profile. Larger soak and recovery jobs may be separate CI jobs, but their results remain required candidate evidence. Changing a relevant implementation invalidates its previous evidence. Do not add manual waivers that convert a failed required test into a passing release.

R0 requires local lifecycle, scope, external tool/approval examples, error boundaries, wait responsiveness, reconnect, shutdown, and overload checks plus real Pi. R1 adds both cloud stores, external SQL, sync, and restore. R2 adds each advertised harness/MCP combination and both SDKs. R3 adds catalog provider/security suites. R4 adds declared capacity/soak and disaster-recovery evidence.

Keep examples executable and publish limitations alongside capabilities. Change tests and generated contracts with the implementation, without historical schema fixtures or migration tests unless compatibility is explicitly requested. Choose representative cross-component end-to-end cases and require contract suites for every advertised adapter.
