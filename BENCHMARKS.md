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
