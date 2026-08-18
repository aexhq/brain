# Benchmarks — the slice-5 record

The published numbers behind the platform. Method before numbers: every figure measures the
PLATFORM, never a model — the provider is the scripted fake (instant), the hand is an
in-process echo, the journal is in-memory, and the drive path is the real public HTTP API with
SSE, because that is what production serves. Reproduce with `cargo run -p brain-bench
--release -- <arm>`; the same suite gates every push in CI (thresholds are loose backstops for
the shared runner; this file is the record).

**Environment:** c7g.xlarge (4 vCPU Graviton3, Ubuntu, glibc), release build — the production
target family. Windows/macOS runs are smoke only (memory arms refuse without `smaps_rollup`).

## The brain

| Number | Value |
| --- | --- |
| Platform-added TTFT (`POST /messages` → first `assistant.delta` at the client, K=1) | **p50 1.4 ms · p99 2.2 ms** |
| Whole-turn overhead (pure text turn over HTTP+SSE, K=1) | p50 2.3 ms · p99 3.2 ms |
| Admission (`POST` → `turn.started`, K=1) | p50 0.22 ms |
| Unpaced throughput, K=64 sessions | **2,002 turns/s** · turn p99 58 ms |
| Unpaced throughput, K=256 sessions | 2,100 turns/s · turn p99 254 ms |
| Tool loop, K=64, 2 rounds × 4 parallel calls/turn | 429 turns/s (**≈3,430 tool calls/s**) |
| Resident session (fold cached between turns; journal-neutral) | **21–31 KiB private** (≈5 KiB with `MALLOC_ARENA_MAX=1`) |
| Memory returned on delete-all, single cycle | 82–86 % (glibc `malloc_trim`; the rest is arena fragmentation) |
| Steady-state creep (create/turns/delete cycles, tail) | **−0.6 MiB/cycle over 6 cycles** — a plateau, not a leak |

Notes that keep these honest:

- **Resident** = the private-byte delta between "actors alive, folds cached" and "actors
  idle-discarded, journal retained". The in-memory journal is excluded on purpose: production
  journals live in DynamoDB, off-process. (Journal-inclusive: ~450–470 KiB/session at 4 turns
  × 8 KiB text with the dev journal in-process.)
- **Reclaim** is two numbers because one would lie. The single-cycle percentage is capped by
  glibc arena fragmentation (`malloc_trim` cannot compact interior holes; `MALLOC_ARENA_MAX`
  trades hot-path throughput for a lower floor — a deploy-time knob). The invariant a
  long-lived brain needs is the second number: the post-delete floor stops rising. It does.
- The TTFT gate caught a real bug while being built: Nagle + delayed ACK put a hard **46 ms
  floor** under every turn on Linux (SSE frames are small writes). `TCP_NODELAY` on the serve
  path took K=1 p50 from 46 ms to 5 ms; the CI gate exists so it cannot come back.

## The hand (real Lambda MicroVM, measured in-region, eu-west-1)

| Number | Value |
| --- | --- |
| Endpoint round trip (raw HTTP probe through the JWE proxy) | **p50 2.1 ms** · p99 3.4 ms |
| Tool call round trip (bash `true`: start → poll complete over the ABI), image v2.0 | p50 50.0 ms · p95 60.1 ms |
| Tool call round trip, image v3.0 (`TCP_NODELAY` on both WebSocket ends) | **p50 4.4 ms · p99 5.3 ms** — 11× |
| IMDS from inside the guest | **no IAM role, no credentials** (role list 404, creds unreachable; identity doc exposes the region only) |

The 50 ms tool-call floor against a 2 ms network baseline was the same Nagle signature the
brain had, in the ABI WebSocket hops. Client-side nodelay alone changed nothing (the delayed
leg was the guest's responses); with image v3.0 fixing both ends the platform adds ~2 ms over
the raw endpoint round trip. Reproduce with
`hand-lambda gate --image aex-hands-dev-1gb --version <v>` — IMDS is a hard pass/fail, the
latency row is the record. The security gate and this latency measurement run on demand (they
launch a real MicroVM and need AWS credentials); the brain suite runs in CI on every push.

## Cross-session leakage

`crates/brain/tests/leakage.rs`, on every push: two tenants on one brain, real files on disk,
interleaved turns, dialect-split scripted providers. Workspaces are disjoint on disk, a probe
for the other tenant's file by path finds nothing, no foreign content or model identity in
either journal, provider keys appear in NO journal (not even the session's own), every event
names its own session, and every wire request carries exactly its own sealed system prompt.
