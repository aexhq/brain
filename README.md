<h1 align="center">Brain</h1>

<p align="center"><strong>A minimal, environment-neutral kernel for agent sessions.</strong></p>

Brain seals a session's model, imported agent loop, tools, and named environment bindings; journals
effects before dispatch; and exposes neutral ports for execution, durability, custody, and storage.
It does not select a default loop or default environment.

## Components

| Component | Purpose |
| --- | --- |
| [`brain-protocol`](crates/brain-protocol) | Session, agent-loop, and environment contracts |
| [`brain`](crates/brain) | Session engine, tool router, recovery, and adapter ports |
| [`brain-loophost`](crates/brain-loophost) | Isolated host for imported agent-loop components |
| [`brain-standalone`](crates/brain-standalone) | SQLite journal, encrypted local custody/storage, and an explicit local environment |
| [`brain-aws`](crates/brain-aws) | Neutral DynamoDB, KMS, and S3 adapters |
| [`brain-server`](crates/brain-server) | HTTP server and development composition |

Environment extensions implement Brain's public environment port. Tool execution always names its
environment binding; Brain never searches for a default environment. An SDK may resolve an omitted
binding before session creation only when exactly one declared environment is compatible.

## Run standalone

```sh
export BRAIN_MODE=local
export BRAIN_DATA_DIR="$PWD/brain-data"
cargo run --release -p brain-server --bin brain
```

Brain binds `127.0.0.1:3210` by default. Set `BRAIN_API_TOKEN`, or read the generated mode-0600
token from `$BRAIN_DATA_DIR/operator.token`. Local mode deliberately executes prepared Tool
artifacts as unsandboxed host Node 22 subprocesses; use an isolated environment extension for
untrusted workloads.

## Embed Brain

Implement the public environment, journal, custody, storage, or trusted-tool ports required by the
composition, then supply them to `Brain::with_parts_and_services`.

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p brain-bench --release -- ci
```

The schemas, OpenAPI document, protocol semantics, generators, and conformance fixtures in this
repository are the source of truth. See [BENCHMARKS.md](BENCHMARKS.md) for benchmark methodology.

Licensed under [Apache 2.0](LICENSE).
