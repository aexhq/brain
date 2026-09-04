# Benchmarks

> [!NOTE]
> Current headline figures are in the [README](README.md#benchmarks), measured with the harness in
> [`tools/bench`](tools/bench) against sixteen other agent runtimes, including the pi, Codex and OpenCode coding agents driven through their own integration surfaces.

## What the benchmark measures

The engine, not a model. It drives the real HTTP and SSE paths with an instant scripted provider and
an in-process echo environment, so no model latency reaches the numbers. First-byte and turn
measurements include HTTP, SSE, request construction, writing to the session log, and dispatch.

Density and reclaim measurements need Linux `/proc/*/smaps_rollup`. Other platforms run the portable
latency and correctness arms only. The harness refuses to substitute RSS for private memory, because
that would double-count shared pages.

## Coding agents

pi, Codex and OpenCode were measured on 2026-09-04 (runs `run-1788481380908` and, for first
token, `run-1788481939624`) on one `c7g.xlarge`, 450 samples per latency probe after the warmup
drop. The first-token run delays the scripted provider's first token by 100 ms and subtracts it,
so first byte and turn end are separable. Each agent is the shipped binary at its defaults, driven
through its own integration surface — pi's RPC mode, `codex app-server`, `opencode serve` — in an
empty git repository with its tools enabled. The manifests under
[`tools/bench/subjects`](tools/bench/subjects) (`pi`, `codex`, `opencode`) carry every probe's
definition and every configuration difference from a stock install; pi and Codex speak only
stdio and are reached through the runner's [stdio bridge](tools/bench/README.md#adding-a-subject).

| Subject | Version | New session | First token | Round trip | Cold start | Recovery | Written per 100 turns |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| pi | 0.84.4 | 7.0 ms | 6.3 ms | 5.1 ms | 398 ms | 394 ms | 0.07 MiB |
| Codex | 0.153.1 | 14 ms | 39 ms | 47 ms | 293 ms | 194 ms | 0.46 MiB |
| OpenCode | 1.18.27 | 2.1 ms | 99 ms | 155 ms | 4.84 s | 4.22 s | 0.14 MiB |
| Brain, same run | `1187ade` | 0.64 ms | 8.6 ms | 15 ms | 654 ms | — | 0.15 MiB |

Cold start is process launch on a fresh data directory until the first turn is served, with
installation untimed. Recovery is `kill -9` after 50 turns, relaunch on the same data, until the
same session serves a turn whose model request carries all 50 — pi through `switch_session`,
Codex through `thread/resume`, OpenCode by session id. Brain's recovery is withheld: the session
comes back but its context restores empty ([#140](https://github.com/aexhq/brain/issues/140)).

pi completes a text turn faster than Brain does. Its loop runs in-process in Node and appends
the turn to one JSONL file; Brain's turn activates a Wasm component, journals every decision,
and dispatches environment lifecycle over HTTP. What each does per turn is the difference, and
the harness reports it rather than explaining it away.

Idle footprints, private memory across the process tree after settling: pi 239 MiB (bridge
included), Codex 101 MiB, OpenCode 459 MiB. OpenCode's fixture count runs one call per session
above its turn count, because it titles every new session with its small model, which here is
the scripted provider too.

## What CI enforces today

Every push runs a resource bound against a live server: after 10,000 requests, resident memory must
stay under 256 MiB and must not have grown by more than 16 MiB. See the `benchmark-leakage` job
in [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

The same job bounds journal growth, on both axes. A context envelope grows with every decision
*and* across every turn, so anything written per decision or per turn costs the sum of every
intermediate size rather than the final one. On the decision axis the kernel caps it at
`BRAIN_MAX_DECISIONS=128`; on the turn axis nothing caps it, and 64 turns holding a 1 MiB context
once wrote 34 MB of journal — every byte of it read back at each restart.
`crates/brain/tests/journal_growth.rs` holds both, and one page of the event stream, to a small
constant multiple of the final context.

## What the journal costs

`crates/brain/tests/journal_throughput.rs` reports the cost of the journal itself, apart from HTTP,
the model and the loop:

```sh
cargo test --release -p brain --test journal_throughput -- --ignored --nocapture
```

It reports rather than asserts, and it is ignored by default. A threshold that passes on a laptop
says nothing about a server, so the numbers quoted in the README are from one machine and are worth
exactly what re-running them on yours is worth. The shape is the durable part: an append is a
serialise, a hash of those bytes and a channel send, with no syscall on the turn's path, and a
restart pays for the log that was kept rather than for every record ever written.

That job is a leak guard, not a benchmark, and it does not check leakage between sessions;
the name is older than what it does. Latency, throughput, and the cross-session isolation test are
being rebuilt — see **Roadmap** in the [README](README.md).
