<h1 align="center">Brain</h1>

<p align="center"><strong>A minimal and extensible kernel for AI workloads.</strong></p>
<p align="center">
  <a href="https://aex.dev">Aex</a> ·
  <a href="contracts/session/v1/openapi.yaml">Session API</a> ·
  <a href="https://github.com/aexhq/extensions">Extensions</a> ·
  <a href="https://discord.gg/Qk2YnHMHVb">Discord</a>
</p>

## What it is
Brain is a minimal session kernel that hosts four replaceable component kinds: Agentloop, Tool,
Environment, and Model.
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
## TypeScript client

```sh
npm install @aexhq/brain @aexhq/loop-pi
```

```ts
import { Brain } from "@aexhq/brain";
import { pi } from "@aexhq/loop-pi";

const brain = new Brain({ token: process.env.BRAIN_TOKEN! });
const session = await brain.sessions.create({
  model: {
    dialect: "openai",
    baseUrl: "https://api.openai.com/v1",
    name: "gpt-4.1-nano",
    apiKey: process.env.OPENAI_API_KEY!,
  },
  agentloop: pi(),
});

console.log(await session.send("Echo hello."));
```

Omitting `tools` exposes no model tools. Components are ordinary immutable package values; the
official packages use the same public contract as third-party components.

Brain speaks two model dialects natively — the OpenAI and Anthropic request and response shapes —
and knows nothing else about providers. A session wires the dialect and the endpoint that speaks
it, so any endpoint of either shape works without a component, a registry or a catalog. Which
provider names a platform resolves to an endpoint is that platform's policy, not Brain's.

## Components

| Component | Purpose |
| --- | --- |
| [`brain-protocol`](crates/brain-protocol) | Session API and Brain-to-Environment contracts |
| [`brain`](crates/brain) | Session engine, component routing, recovery, and adapter ports |
| [`brain-standalone`](crates/brain-standalone) | SQLite journal, encrypted local custody/storage, and an explicit local Environment |
| [`brain-aws`](crates/brain-aws) | Neutral DynamoDB, KMS, and S3 adapters |
| [`brain-server`](crates/brain-server) | Standalone server and development composition |
| [`@aexhq/brain`](packages/brain) | TypeScript client, Tool API, customer Environment, schemas, and builder |
| [`packages/agentloop`](packages/agentloop) | Private conformance fixture for the Agentloop host ABI |

Extensions implement Brain's public component worlds. Brain does not import an official
implementation or require components to have been authored in JavaScript.

## Run standalone

```sh
export BRAIN_MODE=local
export BRAIN_DATA_DIR="$PWD/brain-data"
cargo run --release -p brain-server --bin brain
```

Brain binds `127.0.0.1:3210` by default. Set `BRAIN_API_TOKEN`, or read the generated mode-0600
token from `$BRAIN_DATA_DIR/operator.token`. Local mode deliberately executes managed Tool bundles
as unsandboxed host Node 22 subprocesses; use a hosted Environment for untrusted workloads.

Provider-backed Environment components use an optional same-host adapter configured with
`BRAIN_ENVIRONMENT_DISPATCH_URL`, `BRAIN_ENVIRONMENT_DISPATCH_TOKEN`, and
`BRAIN_ENVIRONMENT_DISPATCH_TIMEOUT_MS`. The URL must be a literal loopback HTTP address. If it is
absent, Environment host operations fail closed; pure Environment components continue to work.

Production mode uses Brain's AWS journal, custody, and session-storage adapters and requires
`BRAIN_API_TOKEN`, `BRAIN_DATA_DIR`, `AWS_REGION`, `BRAIN_JOURNAL_TABLE`, `BRAIN_KMS_KEY_ID`, and
`BRAIN_SESSION_STORAGE_BUCKET`. `BRAIN_DATA_DIR` must be persistent because it is the
content-addressed store for admitted component binaries. Hosted requests require an explicit
`x-brain-tenant-id`; no product-specific composition is embedded in the image.

A trusted HTTP Tool executor is configured with `BRAIN_EXTERNAL_TOOL_EXECUTOR_URL`, optional
`BRAIN_EXTERNAL_TOOL_EXECUTOR_TOKEN`, and `BRAIN_EXTERNAL_TOOL_POLICIES_JSON`. The policy value is
a bounded JSON array of `{ capability, scope, completion, effect, max_input_bytes }` objects; it is
deployment configuration, never customer session input. Hosted application callbacks additionally
set the three `BRAIN_CUSTOMER_ENVIRONMENT_{WEBSOCKET_URL,OBSERVATION_BASE_URL,CALLBACK_URL}` values
together. The callback endpoint is an HTTPS AWS API Gateway Management endpoint.

Structured logs always go to stderr. Setting `OTEL_EXPORTER_OTLP_ENDPOINT` enables OTLP/HTTP trace,
metric, and log export; `OTEL_EXPORTER_OTLP_PROTOCOL`, when present, must be `http/protobuf`.
Component workers inherit only `OTEL_*` and `RUST_LOG`, while guest components retain no ambient
process environment.

Set `BRAIN_COMPONENT_CACHE_DIR` to an absolute, Brain-owned directory to share Wasmtime's validated
compiled-component cache across worker processes. The cache changes startup cost only; component
bytes and digests remain the runtime identity.

## Embed Brain

Implement the public Environment, journal, custody, storage, or trusted-tool ports your environment needs,
then compose them with `Brain::with_parts_and_services`.

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p brain-bench --release -- ci
npm ci
npm test
npm run package-smoke
```

The schemas, OpenAPI document, protocol semantics, examples, generators, and conformance fixtures
in this repository are the source of truth. See [BENCHMARKS.md](BENCHMARKS.md) for the methodology
and reference measurements.

Licensed under [Apache 2.0](LICENSE).
