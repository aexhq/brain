# Benchmarks

Brain's benchmark suite measures the engine, not a model. It drives the public HTTP and SSE paths
with an instant scripted provider, an in-process echo Hand, and an in-memory journal. Reference
measurements below were recorded on 18 August 2026 using a release build on a c7g.xlarge
(4-vCPU Graviton3, Ubuntu, glibc).

## Reference results

| Measurement | Result |
| --- | ---: |
| First visible byte, one session | p50 1.4 ms · p99 2.2 ms |
| Complete text turn, one session | p50 2.3 ms · p99 3.2 ms |
| Admission to `turn.started` | p50 0.22 ms |
| Throughput, 64 sessions | 2,002 turns/s · p99 58 ms |
| Throughput, 256 sessions | 2,100 turns/s · p99 254 ms |
| Tool loop, 64 sessions, 2 rounds × 4 calls | 429 turns/s · about 3,430 tool calls/s |
| Resident session, excluding the in-process journal | 21–31 KiB private memory |
| Memory returned after deleting all sessions | 82–86% in one cycle |
| Steady-state change across delete cycles | −0.6 MiB per cycle over 6 cycles |

The first-byte and turn measurements include HTTP, SSE, request construction, journaling, and
dispatch. The provider itself is instant. The resident-session figure is the private-memory delta
between live actors and the post-discard state; production journals are off-process. An
in-process journal at four 8-KiB turns uses roughly 450–470 KiB per session.

The one-cycle reclaim percentage reflects allocator fragmentation. The stronger long-running
invariant is that the post-delete floor stops rising; the measured floor plateaued. CI thresholds
are deliberately loose regression guards for shared runners, not replacements for the reference
record.

## Reproduce

```sh
cargo run -p brain-bench --release -- ci
cargo run -p brain-bench --release -- --help
```

Density and reclaim measurements require Linux `/proc/*/smaps_rollup`; other platforms run the
portable latency and correctness arms only. The benchmark refuses to substitute RSS because it
would double-count shared pages.

`crates/brain/tests/leakage.rs` independently checks cross-session isolation on every push: files,
conversation content, model identity, provider keys, events, and journals must remain scoped to
their owning session.
