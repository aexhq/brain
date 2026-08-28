# Benchmarks

> [!NOTE]
> The benchmark harness is being rebuilt against the current kernel. The figures in the archive
> below were measured before that rebuild and are **not current**. Do not quote them as present-day
> numbers.

## What the benchmark measures

The engine, not a model. It drives the real HTTP and SSE paths with an instant scripted provider and
an in-process echo environment, so no model latency reaches the numbers. First-byte and turn
measurements include HTTP, SSE, request construction, writing to the session log, and dispatch.

Density and reclaim measurements need Linux `/proc/*/smaps_rollup`. Other platforms run the portable
latency and correctness arms only. The harness refuses to substitute RSS for private memory, because
that would double-count shared pages.

## What CI enforces today

Every push runs a resource bound against a live server: after 10,000 requests, resident memory must
stay under 256 MiB and must not have grown by more than 16 MiB. See the `benchmark-leakage` job
in [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

The same job bounds journal growth. A turn's context envelope grows with every decision, so
anything written per decision costs the sum of every intermediate size; at the production ceiling
of `BRAIN_MAX_DECISIONS=128` that is the difference between a megabyte of conversation and tens of
megabytes of permanently retained journal. `crates/brain/tests/journal_growth.rs` holds a turn's
journal, and one page of its event stream, to a small constant multiple of the final context.

That job is a leak guard, not a benchmark, and it does not check leakage between sessions;
the name is older than what it does. Latency, throughput, and the cross-session isolation test are
being rebuilt — see **Status** in the [README](README.md).

## Pre-rebuild archive (not current)

Measured 2026-08-18 on a c7g.xlarge (4-vCPU Graviton3, Ubuntu, glibc) release build, against the
kernel as it stood before the architecture reset. Kept for reference only.

| Measurement | Result |
| --- | ---: |
| First visible byte, one session | p50 1.4 ms · p99 2.2 ms |
| Complete text turn, one session | p50 2.3 ms · p99 3.2 ms |
| Admission to `turn.started` | p50 0.22 ms |
| Throughput, 64 sessions | 2,002 turns/s · p99 58 ms |
| Throughput, 256 sessions | 2,100 turns/s · p99 254 ms |
| Tool loop, 64 sessions, 2 rounds × 4 calls | 429 turns/s · about 3,430 tool calls/s |
| Resident session, excluding the in-process log | 21–31 KiB private memory |
| Memory returned after deleting all sessions | 82–86% in one cycle |
| Steady-state change across delete cycles | −0.6 MiB per cycle over 6 cycles |

The resident-session figure is the private-memory delta between live actors and the post-discard
state; production logs are off-process. An in-process log at four 8-KiB turns used roughly
450–470 KiB per session.

The one-cycle reclaim percentage reflects allocator fragmentation. The stronger long-running
invariant is that the post-delete floor stops rising, and the measured floor plateaued.
