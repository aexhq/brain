# Application SDK architecture spikes

Measured 2026-10-16 for [ADR 0003](../adr/0003-application-sdk-and-session-bindings.md). These are local architecture experiments, not production capacity figures or qualification of new Brain adapters.

The results support different host occupancy/concurrency limits by harness and different execution transports by deployment. Sharing Pi hosts saved substantial memory with similar throughput in this fixture. Sharing eight OpenCode sessions saved memory but approximately halved throughput. WebSocket messages performed well for small reverse callbacks; direct HTTP performed better for the larger result payload.

## Environment and boundaries

| Item | Setup |
| --- | --- |
| Machine | Intel Core i7-12700K, 12 cores / 20 logical processors, approximately 64 GiB RAM |
| OS | Windows 11, build 26200 |
| Runtimes | Node 24.17.0; Python 3.12.10 measurement driver |
| Native harnesses | Pi coding-agent/AI 1.2.0, Codex App Server 0.163.0, OpenCode SDK/core/plugin 2.0.30 |
| Transport implementation | Separate Node controller and executor processes; `ws` 8.18.3 for WebSocket |
| Networking | IPv4 loopback; persistent connections; no TLS, proxy, WAN, or real browser |
| Workload | Synthetic text and a deterministic `echo` tool; no real account credentials, paid model calls, or cloud resources |
| Scope excluded | Brain's coordinator, authorization, schema policy, event persistence, encrypted credential storage, checkpoint recovery, and external service quotas |

The PC was shared with other work. The experiment series did not run simultaneously with each other. CPU and filesystem cache state were not controlled as in a dedicated performance lab. Treat differences as evidence for the next design step, not service-level objectives.

Memory below is Windows **private committed memory**, not uniquely resident physical RAM. Shared mapped pages make summed working sets a different measurement. Native burst peaks were sampled every 50 ms and can miss shorter peaks. Contexts were small; these figures do not bound the memory of long conversations.

## Native harness experiment

Compared one native process per session with one native process containing several sessions. Both used the same adapter fixture for that harness, private homes/state directories outside the repository, explicit environments, and the same model/tool workload. The shared variants are prototypes; they do not establish cross-tenant isolation.

Each model turn made two local provider requests with a requested 40 ms delay each, with one 20 ms tool wait between them. Pi used the completions protocol; Codex and OpenCode used Responses. Native context and persistence machinery remained in the harness. Cross-harness turn times therefore are not a ranking of LLM performance.

Measured one session as a common baseline, then dedicated and shared topologies at 4 and 8 sessions, in three runs with alternating topology order. Started at most four hosts/session openings concurrently. Verified that fresh startup made no provider calls, warmed each session with a complete tool turn, then submitted 16 inputs per session together. A per-session queue allowed only one active turn in each session. Every measured turn completed two provider requests and exactly one external fixture tool call.

There were 45 main cases and 9 additional startup-limit cases: **4,752 measured turns**, excluding warmups. All 207 recorded root-process launches were reaped. This does not establish abrupt-parent-death or descendant-process cleanup on any OS.

Tables show medians across the three main runs. A reported p95 is the median of the three per-run p95 values, not a pooled quantile.

### One session

| Harness | Fresh process and session ready | Memory after one warmup turn |
| --- | ---: | ---: |
| Pi | 769 ms | 165 MiB |
| Codex | 204 ms | 22 MiB |
| OpenCode | 1,514 ms | 363 MiB |

Fresh process does not mean cold OS filesystem cache. Readiness includes importing/initializing the harness and opening its native session, not just creating an OS process.

### Eight resident sessions

| Harness | Dedicated warm memory | Shared warm memory | Dedicated / shared observed burst peak | Dedicated / shared all-ready time |
| --- | ---: | ---: | ---: | ---: |
| Pi | 1,322 MiB | 166 MiB | 1,348 / 248 MiB | 1.63 / 0.79 s |
| Codex | 176 MiB | 39 MiB | 218 / 73 MiB | 0.66 / 0.20 s |
| OpenCode | 2,718 MiB | 396 MiB | 2,791 / 540 MiB | 3.38 / 1.53 s |

The shared Pi host's memory grew during the burst; its initial 166 MiB snapshot is not a steady-state or unlimited-session guarantee. Garbage collection and native caches also change these snapshots over time.

### Eight active sessions with queued input

| Harness | Dedicated / shared turns per second | Dedicated / shared active-turn p95 | Dedicated / shared completion p95 including queue wait |
| --- | ---: | ---: | ---: |
| Pi | 65.1 / 67.7 | 127 / 139 ms | 1.92 / 1.82 s |
| Codex | 27.7 / 23.9 | 320 / 403 ms | 4.50 / 5.06 s |
| OpenCode | 43.2 / 20.6 | 217 / 484 ms | 2.87 / 6.13 s |

These are synthetic-turn rates, not predictions for real models. Queue wait dominated the end-to-end latency of later inputs even when individual turns were short.

At four sessions, shared OpenCode reached 16.3 turns/s versus 22.0 dedicated. At eight, its shared host consumed about 1.1 CPU cores on average during the burst, versus approximately 4.0 cores across the dedicated hosts. This is evidence of a shared-host concurrency tradeoff; the internal bottleneck was not profiled or attributed to a particular subsystem.

### Cold-start bursts

Time until all eight dedicated hosts were ready:

| Harness | Startup limit 1 | Limit 2 | Limit 4 | Limit 8 |
| --- | ---: | ---: | ---: | ---: |
| Pi | 5.73 s | 2.87 s | 1.63 s | 0.98 s |
| Codex | 1.39 s | 0.93 s | 0.66 s | 0.63 s |
| OpenCode | 11.46 s | 5.95 s | 3.38 s | 2.02 s |

Limits 1, 2, and 8 were single exploratory runs; limit 4 is the median of the main runs. They do not establish an optimal universal startup limit.

Higher startup concurrency finished the batch sooner, but delayed the first ready host and increased startup CPU use. With limits 1 versus 8, first readiness moved from 770 to 932 ms for Pi, 179 to 495 ms for Codex, and 1,444 to 1,935 ms for OpenCode. Pi and OpenCode used about 30% more aggregate CPU time in the eight-at-once startup run.

Use separate budgets for resident hosts, active turns, startup work, and queued inputs. A cold-start burst is different from a burst against already-resident sessions.

## Tool transport experiment

Compared three execution paths with identical JSON call identities and results:

| Path | Messages |
| --- | --- |
| Direct HTTP | Controller POSTs to a reachable tool; progress and the final result use its streamed response |
| Reverse SSE + HTTP | Executor opens an outbound SSE command stream; it POSTs progress and results back |
| WebSocket | Executor opens one outbound bidirectional connection carrying calls, progress, and results |

An initial sweep covered concurrency 1, 8, 32, and 64, three workloads, and three runs: 17,280 measured calls. A longer follow-up used concurrency 64, eight warmup batches, and 350,208 measured calls across three runs. It checked that short-run initialization and timer/accounting effects were not driving the recommendation.

The longer runs used 32,768 calls per case for the 1 KiB immediate result, 2,048 for the delayed/progress case, and 4,096 for the 64 KiB result. The table reports the median per-run p95:

| Workload, 64 concurrent calls | Direct HTTP | Reverse SSE + HTTP | WebSocket |
| --- | ---: | ---: | ---: |
| 1 KiB result, no handler delay | 3.64 ms | 3.38 ms | 0.97 ms |
| 1 KiB result, requested 20 ms work and two progress updates | 33.17 ms | 48.53 ms | 32.15 ms |
| 64 KiB result, no handler delay | 13.33 ms | 15.18 ms | 25.23 ms |

The delayed handler used two 10 ms waits. Windows timer behavior made its observed baseline approximately 31 ms; do not subtract a guaranteed 20 ms to derive a precise transport-only cost.

The reverse SSE prototype awaited each progress POST acknowledgment. WebSocket progress was queued on its ordered channel. HTTP batching, unacknowledged progress, HTTP/2, native WebSocket accelerators, real browser implementations, and Go server implementations were not compared. The large-payload crossover is specific to these implementations, not a rule that WebSocket is always slower for large messages. Short-case CPU accounting was too coarse to interpret zero readings as zero work.

### Bounded overload burst

A separate prototype dispatcher offered 128 calls at once with 8 active slots and 32 queued slots. Every path admitted 40, rejected 88 before dispatch, and observed at most 8 executing handlers and 32 queued calls. This was an experimental dispatcher, not Brain's existing admission implementation.

In the longer-run series, the median p95 completion including queue wait was 165 ms for direct HTTP, 170 ms for reverse SSE, and 161 ms for WebSocket. Excess calls were rejected in under 1 ms at p95. The admitted work drained; rejected calls did not execute later or get retried automatically.

Transport choice does not replace admission limits. Nor does a loopback endpoint model a Lambda cold start, reserved concurrency, throttling, or platform retry policy.

## Qualification observations

The native fixtures invoked only `echo`. Their advertised tool lists exposed separate integration work:

- **Pi:** the outgoing request advertised exactly `echo`.
- **Codex 0.163.0:** with execution/delegation features and web search disabled, the performance fixture still advertised `request_user_input` alongside `echo`. A [separate follow-up](harness-probes.md#codex-catalog-exclusion-follow-up-2026-10-16) verified that `tools.experimental_request_user_input.enabled = false` removes it. That closes the specific catalog-control gap, not the remaining adapter qualification.
- **OpenCode 2.0.30:** removing registered native tools and setting the custom tool's `codemode: false` still advertised a generated `execute` tool alongside `echo`. The published core's code-mode tool accepts JavaScript and exposes HTTP `fetch`; ordinary tool-registry filtering alone is not proof of the external-only execution boundary. No `execute` call was made in the spike. A supported exclusion control or qualified adapter change is still required.

OpenCode's recorded startup auto-resume and incremental SQLite checkpoint issues also remain. Fresh empty hosts making no model calls do not prove passive recovery of suspended sessions.

OpenCode installation encountered a native grammar build failure with the experiment's Node 26 setup and a Windows path-length failure in the long session-artifact directory. Node 24 installation and execution succeeded under a short dependency path. A transitive UI dependency's Node >=26.4 declaration remains a separate compatibility qualification issue; these measurements do not establish general Node 24 support.

## Recommendations for the ADR

**Credentials:** preserve managed-tool credentials independently of an observing browser and native process residency. The selected encrypted SQL/private checkpoint design addresses that lifetime requirement. Encryption, key recovery, and vault integrations were not implemented or benchmarked here.

**Tool deployment:** use direct HTTP/service references for already-deployed backend or Lambda tools. Use an SDK-owned reverse connection for browser/backend-local callbacks. A serverless invocation that merely creates a session does not become a lasting callback host.

**Transports:** ADR 0003 selects direct HTTP, reverse SSE plus HTTP, and bidirectional RPC. The WebSocket prototype provides evidence for one possible RPC carrier, not a finalized RPC protocol. Use the binding suited to the deployment, and qualify it against the intended Go server and supported clients before treating these numbers as implementation targets.

**Host topology:** the subsequent design decision selects bounded shared hosts, including cross-tenant occupancy, subject to logical session-isolation qualification. The measurements inform different limits per harness: Pi shows substantial memory savings, Codex's dedicated-process cost is much smaller, and OpenCode sharing needs an active-turn cap and investigation of the concurrency loss. The performance results do not establish safe tenant isolation, session-scoped export/restore, or independent failure domains.

**Capacity:** bound startup separately, evict only idle checkpointed state, and report queue wait separately from active execution. Retained session count is not resident-process count. Fleet sizing, long-history memory, saturation-time interruption, crash recovery, and cloud behavior remain the work described on the [roadmap](../roadmap.md).

The scripts and raw measurements are retained as local experiment artifacts under `files\architecture-spikes` in the authoring session. This report preserves the workload and results; it is not a maintained benchmark suite or a runtime implementation.
