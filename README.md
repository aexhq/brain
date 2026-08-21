<h1 align="center">Brain</h1>

<p align="center"><strong>Durable sessions for models and tools.</strong></p>
<p align="center">
  A product-neutral session engine for model conversations, tool execution, recovery,
  cancellation, and event streams.
</p>
<p align="center">
  <a href="https://aex.dev">Aex</a> ·
  <a href="contracts/session/v1/openapi.yaml">Session API</a> ·
  <a href="STANDALONE.md">Standalone</a> ·
  <a href="https://github.com/aexhq/hands">Hands</a> ·
  <a href="https://discord.gg/Qk2YnHMHVb">Discord</a>
</p>

Brain runs without an Aex account or control plane. One server owns many durable sessions while
applications decide which tools each session can use. Aex is a downstream product built on Brain.

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

`.server(import.meta.url)` bundles a function for the session's Hand. Use `.client()` with a stable
`Brain({ client: { id } })` identity when the callback must stay in the application process.
Omitting `tools` exposes no model tools.

## Components

| Component | Purpose |
| --- | --- |
| [`brain-protocol`](crates/brain-protocol) | Session API and Brain-to-Hand contracts |
| [`brain-hand-client`](crates/brain-hand-client) | Client for the public Hand protocol |
| [`brain`](crates/brain) | Session engine, providers, tool router, recovery, and adapter ports |
| [`brain-standalone`](crates/brain-standalone) | SQLite journal, encrypted local custody, files, and Docker Hands |
| [`brain-aws`](crates/brain-aws) | Neutral DynamoDB, KMS, and S3 adapters |
| [`brain-server`](crates/brain-server) | Standalone server and development composition |
| [`@aexhq/brain`](packages/brain) | TypeScript client, Tool API, customer Hand, schemas, and builder |
| [`@aexhq/brain-tools`](packages/brain-tools) | Portable Tool values selected by an application |

Hands implement Brain's public ports. Brain never imports a Hands implementation.

## Run standalone

```sh
export BRAIN_HAND_IMAGE='ghcr.io/aexhq/hands@sha256:<digest>'
export BRAIN_DATA_DIR="$PWD/brain-data"
cargo run --release -p brain-server --bin brain
```

Brain binds `127.0.0.1:3210` by default. Set `BRAIN_API_TOKEN`, or read the generated mode-0600
token from `$BRAIN_DATA_DIR/operator.token`. See [STANDALONE.md](STANDALONE.md) for Docker Compose,
backups, networking, and trust boundaries.

## Embed Brain

Implement the public Hand, journal, custody, storage, or trusted-tool ports your environment needs,
then compose them with `Brain::with_parts`. The
[`custom_adapter` test](crates/brain/tests/custom_adapter.rs) is a complete third-party example.

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
