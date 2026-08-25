<h1 align="center">Brain</h1>

<p align="center"><strong>A minimal, extensible kernel for agents</strong></p>

> This repo is in heavy early development, interfaces and contracts might change

## What it is
Brain is minimal server that manages a set of primitives and environments for running agent sessions.
The term _Brain_ is originated from Anthropic engineering blog [Scaling Managed Agents: Decoupling the brain from the hands](https://www.anthropic.com/engineering/managed-agents) 
Brain is inspired by [Pi Agent Harness](https://github.com/earendil-works/pi), we believe modern framework should be minimal and open for extension so everyone can build upon it.

## Architecture
### Brain and hands
Brain is where agent session, agent loop and tools are managed, it invokes tools but actual execution belong to the hand (environment such as sandbox, browser etc), it outlives the hands, therefore it could manage multiple hands, resilient to sandbox failures and offer higher level of flexibility.

### Environment-neutral
Brain does not assume the environment or tools the agent is working with This mean you could define one tool to be executed within your app running on client side while having another tool in the same agent session to execute some script in a sandbox  

### Language-neutral
Brain does not assume the language you are working with, you could write one tool in rust and another tool in node.

### Agent-neutral
Brain is not an agent, but you can easily build an agent with it. This allow you to run your favorite agent runtime like pi/codex/opencode as agentloop.

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
