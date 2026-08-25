<h1 align="center">Brain</h1>

<p align="center"><strong>A minimal and extensible kernel for AI workloads.</strong></p>
<p align="center">
  <a href="https://aex.dev">Aex</a> ·
  <a href="contracts/session/v1/openapi.yaml">Session API</a> ·
  <a href="https://github.com/aexhq/environments">Environments</a> ·
  <a href="https://discord.gg/Qk2YnHMHVb">Discord</a>
</p>

## What it is
Brain is minimal server that manages a set of primitives and environments for running agent sessions.
The term _Brain_ is originated from Anthropic engineering blog [Scaling Managed Agents: Decoupling the brain from the hands](https://www.anthropic.com/engineering/managed-agents) and minimalistic concept is inspired by [Pi Agent Harness](https://github.com/earendil-works/pi).

## TypeScript client

```sh
npm install @aexhq/brain zod
```

```ts
import { Brain, tool } from "@aexhq/brain";
import { z } from "zod";

const echo = tool(
  z.object({ text: z.string() }),
  async function echo({ text }) {
    return { text };
  },
)
  .describe("Return the supplied text.")
  .returns(z.object({ text: z.string() }))
  .server(import.meta.url);

export default echo;

const brain = new Brain({ token: process.env.BRAIN_TOKEN! });
const session = await brain.sessions.create({
  model: {
    provider: "openai",
    name: process.env.MODEL_NAME!,
    apiKey: process.env.OPENAI_API_KEY!,
  },
  tools: [echo],
});

console.log(await session.send("Echo hello."));
```

`.server(import.meta.url)` bundles a function for the session's Environment. Use `.client()` with a stable
`Brain({ client: { id } })` identity when the callback must stay in the application process.
Omitting `tools` exposes no model tools.

## Components

| Component | Purpose |
| --- | --- |
| [`brain-protocol`](crates/brain-protocol) | Session API and Brain-to-Environment contracts |
| [`brain`](crates/brain) | Session engine, providers, tool router, recovery, and adapter ports |
| [`brain-standalone`](crates/brain-standalone) | SQLite journal, encrypted local custody/storage, and an explicit local Environment |
| [`brain-aws`](crates/brain-aws) | Neutral DynamoDB, KMS, and S3 adapters |
| [`brain-server`](crates/brain-server) | Standalone server and development composition |
| [`@aexhq/brain`](packages/brain) | TypeScript client, Tool API, customer Environment, schemas, and builder |
| [`@aexhq/brain-tools`](packages/brain-tools) | Portable Tool values selected by an application |

Environments implement Brain's public ports. Brain never imports a Environments implementation.

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
