# Comparative benchmark

Measures Brain and its rivals on the same probes, on one instance, each through its own
public surface. It links no brain crate: Brain's numbers are produced the same way every
competitor's are, because that is the only way the comparison means anything.

```sh
cargo build --release -p brain-server --bin brain -p brain-loophost --bin brain-loop-worker
cargo build --release --manifest-path tools/bench/runner/Cargo.toml

cargo run --release --manifest-path tools/bench/runner/Cargo.toml -- list          # subjects, probes, what is blocking each
cargo run --release --manifest-path tools/bench/runner/Cargo.toml -- floor         # the load generator's own cost — run this first
cargo run --release --manifest-path tools/bench/runner/Cargo.toml -- run --subject brain   --sync-command 'aws s3 cp {file} s3://bench-results/'
cargo run --release --manifest-path tools/bench/runner/Cargo.toml -- report tools/bench/results/run-<id>.jsonl
```

The runner starts each subject itself from the `launch` block in its manifest, on a port
picked per run, with an empty data directory, wired to that subject's own fixtures. That is
what makes the memory arm possible — it knows the pid it started, so it can sample that
process tree. `--base-url` drives something already running instead, and then `--pid` is
needed for memory.

A subject that fails to come up is recorded as a skip with its own log attached, and the
run moves on. On spot capacity there may not be a second chance to collect the others.

## Probes

A probe is one defined measurement. A subject declares which it can answer and what each
means *in its own terms*, so a Brain row and a Daytona row can share a table with the
difference visible rather than hidden.

| Probe | Session kernel | Sandbox | Isolation substrate |
| --- | --- | --- | --- |
| `create` | session create until it accepts work | sandbox create until it accepts an exec | instantiate or snapshot restore |
| `ttfb` | send until the first assistant delta byte | exec until the first stdout byte | call until the first result |
| `round_trip` | one complete turn | exec until the command completed | call until return |
| `tool_dispatch` | tool call until the result is recorded | — | — |
| `throughput` | turns/s at N | execs/s at N | calls/s at N |
| `resident` | private memory per session | private memory, or a proxy for a service | idle memory per instance |
| `cost` | — | price per unit-hour | — |
| `persistence` | framework class only: bytes written per turn, against turn index | — | — |

## Per-turn growth

`growth` is a diagnostic, not a probe. A probe reports a p50 over a whole run, and a p50
cannot tell a constant per-turn cost from one that rises: a longer run simply moves the
median, which is exactly what Brain's `round_trip` p50 did across runs of 270, 540 and 900
turns. This walks one conversation turn by turn and records what every side spent on each
turn — the client's clock, the scripted provider's own service time, the session state file
the subject rewrites, the bytes under its data directory, and the write counters of its
process.

```sh
cargo run --release --manifest-path tools/bench/runner/Cargo.toml --   growth --subject brain --turns 1000 --repeats 3 --out tools/bench/results/growth.csv
```

The provider fixture is handed the whole transcript on every turn, so **its** parse and
serialise cost grows with the conversation too. Timed from request arrival rather than from
inside the handler — the body read and the JSON parse are the fixture's cost and both grow
— and written to the CSV per turn, so it can be subtracted. Without that column a growth
curve measured at the client would be charged entirely to the subject, and some of it is
not the subject's.

Nothing in it changes the subject. Point `TMPDIR` at `/dev/shm` to run the same
conversation with the data directory on tmpfs: a curve that survives losing the disk
entirely is CPU on the request path, not I/O.

## What it refuses to publish

Three gates, because each is how a benchmark stops being evidence.

**A latency it cannot separate from its own jitter.** `floor` drives a server that does
nothing and reports what this process costs. Any subject value below `5 x floor` keeps its
raw record but is withheld from generated tables with the reason attached. On a quiet
machine the floor is around 0.04 ms, so the bar sits near 0.2 ms — Brain's turn latency
clears it by more than an order of magnitude, which is why the runner is Rust and not a
script. A Node client's own jitter would sit at roughly the same magnitude as the numbers
under test.

**A percentile too few samples support.** No p99 below n=100, no p50 below n=20. A missing
percentile renders as an em dash, never as a zero.

**RSS in place of private memory.** Memory means `Private_Clean + Private_Dirty +
Private_Hugetlb` from `smaps_rollup`, summed over the whole process tree, and the runner
refuses to run anywhere it cannot read that. Summing the tree matters: Brain runs a server
plus a loop worker, Letta a server plus Postgres, OpenFang forks Wasm workers, and
measuring one pid would reward whichever subject pushed the most memory into a child.

## Memory

Memory is not a probe of its own. A sampler runs in the background for **whichever probe
is executing**, so one mechanism answers every memory question:

- **per session** — the `resident` probe ramps sessions in steps and fits memory against
  live session count. The quotable figure is the **slope**, which drops the runtime's fixed
  floor automatically — tens of megabytes for anything on Python or Node. It ships with an
  **r²**, and a ramp below 0.95 gets a note saying the slope must not be quoted alone: a
  subject whose memory grows non-linearly has no single per-session cost.
- **under load** — the same sampler running during the `throughput` probe.
- **over time** — the series itself, on any probe.
- **reclaim** — the tail after the deletes, watched for a minute.

**No allocator trim is forced, for any subject.** No competitor exposes a trim hook, so a
forced number would be a Rust-and-glibc-only column that could only be compared to itself.
What a runtime gives back on its own schedule is its policy — glibc on its threshold,
Python only when an arena empties, V8 when GC runs, Go on a scavenger timer — so the tail's
*shape* is that policy and explains itself, while only the trend across repeats is a leak
signal.

This means the pre-reset "82–86% returned in one cycle" figure is **not reproducible and
not comparable**. The claim that survives is the one the archive itself called stronger:
the post-delete floor stops rising.

## Fixtures

Every session-kernel subject that can accept them is wired to the same two, which is what
makes those subjects comparable at all:

- a **scripted provider**, an OpenAI-compatible `/chat/completions` that answers instantly
  with a fixed-length reply, so no model latency reaches any number;
- an **echo environment** speaking the remote environment contract, so a tool-dispatch
  number is the kernel's dispatch and journal cost and nothing else.

After each subject the runner prints how many times the fixtures were actually hit. A
count that does not match the turns driven means the subject answered from a cache or a
replay path, and the latency measured is not the one under test.

A subject that **cannot** take the scripted provider — Claude Managed Agents has no BYOK —
sets `model_included` in its manifest. Its numbers are still collected, still published,
and automatically excluded from engine comparisons.

## Running on spot capacity

Built for it. Records are appended and flushed one at a time, so an instance reclaimed
mid-run loses only the probe in flight:

- `spot/instance-action` is polled throughout; a termination notice ends the run cleanly
  and writes a footer;
- `--resume <run_id>` continues, skipping every probe already recorded;
- `--budget-minutes` stops the run rather than leaving an instance billing on a probe that
  will not finish, and it is enforced **inside** a probe as well as between them, so one
  hung subject cannot eat the budget one client timeout at a time;
- `--sync-command` copies the results file off the box after every record. Per-record
  flushing survives the *process* dying; it does not survive the *machine* going away,
  which on spot is the likelier ending. Without it a reclaimed instance takes the run with
  it, and the runner warns when it is missing;
- every run records instance type, AZ, lifecycle, kernel, THP, and CPU governor. Two runs
  on different instance types are not comparable, and spot hands you whatever it has.

**CPU steal is the real hazard.** These numbers are single-digit milliseconds; a p99
measured while the hypervisor is taking CPU is the neighbour's p99. Each probe is bracketed
against `/proc/stat`, and anything above 1% steal is recorded and kept out of comparison
tables. Pin the subject and the generator to different cores (`BENCH_SUBJECT_CPUS`,
`BENCH_GENERATOR_CPUS`) — on a small instance they otherwise fight, and the generator wins.

`.metal` capacity is required for anything needing nested KVM: Firecracker, forkd,
self-hosted E2B. Ordinary EC2 cannot run them, and the manifest says so rather than
producing a blank row.

## Adding a subject

Create `subjects/<name>/subject.json`. Every probe needs a `definition` — the loader
refuses a manifest without one, because a number that cannot say what it measured cannot
be compared to anything.

The `launch` block tells the runner how to start it. Every string in it may interpolate
`{port}`, `{model_base_url}`, `{environment_base_url}` and `{data_dir}`:

```json
"launch": {
  "command": "target/release/brain",
  "env": {
    "BRAIN_LISTEN": "127.0.0.1:{port}",
    "BRAIN_DATA_DIR": "{data_dir}",
    "BRAIN_MODEL_BASE_URL": "{model_base_url}",
    "BRAIN_ENVIRONMENT_BASE_URL": "{environment_base_url}"
  },
  "base_url": "http://127.0.0.1:{port}",
  "ready_url": "http://127.0.0.1:{port}/health/ready"
}
```

The data directory is created empty and removed afterwards. Letting a subject inherit a
previous run's session log would move both its memory and its latency numbers, and would
move them by a different amount than it moves the next subject's. On Unix the subject runs
in its own process group and the group is signalled on teardown, so loop workers and other
children cannot outlive it and hold ports or memory into the next measurement.

Only `brain` has a driver so far. Any other subject is recorded as a skip rather than
driven with Brain's client, which would produce numbers that look real and mean nothing.

A project with both an open-source build and a hosted service is **two subjects**
(`e2b-selfhosted` and `e2b-cloud`): one measures their code on our instance, the other
measures their service across a network, and merging them would be a category error. The
same split applies to `langgraph` the library and `langgraph-server` the running server.

**Declare only the probes a subject can honestly answer.** zeroclaw and openclaw are real
session daemons, but both document themselves as single-operator tools — so they declare
`create`, `ttfb` and `round_trip`, and deliberately not `throughput` or the per-session
memory ramp. Running those would measure a design goal the projects never had, and
publishing the result would be a straw man. The memory sampler still records their
footprint over time on every probe, which is the memory question that is fair to ask.

The `framework` class is for libraries you build an agent *with* — CrewAI, AutoGen,
LangGraph, Microsoft Agent Framework. They hold no sessions behind a server, so they answer
no latency probe until someone writes a harness around them, and any number published then
has to say we wrote that harness. What they can answer is `persistence`: bytes written per
turn against turn index, which is where snapshot-per-step diverges from an append-only log
and keeps diverging as the conversation grows.

Measuring someone else's project fairly is harder than measuring our own. Run every subject
at its documented defaults, record the config, and ship it with the results — a rival tuned
worse than Brain produces a number that is worth nothing and costs credibility.
