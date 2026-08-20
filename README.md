# Brain

Brain is an independent session engine for durable model conversations, tool execution, recovery,
cancellation, and event streaming. It runs without an Aex account or control plane.

## TypeScript client

```ts
import { Brain, defineTool } from "@aexhq/brain";
import { z } from "zod";

const echo = defineTool({
  module: import.meta.url,
  name: "echo",
  description: "Return the supplied text.",
  input: z.object({ text: z.string() }),
  output: z.object({ text: z.string() }),
  async execute({ text }) {
    return { text };
  },
});

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
```

Tools run in the session's Hand by default. Use `echo.local()` only when execution should happen in
the attached application process. Sessions expose no model tools unless the application selects
them.

## Components

| Component | Purpose |
| --- | --- |
| `brain-protocol` | Session API and Brain-to-Hand wire types |
| `brain-hand-client` | Client for the public Hand protocol |
| `brain` | Session engine, providers, tool router, recovery, and adapter ports |
| `brain-standalone` | SQLite journal, encrypted local custody, files, and Docker Hands |
| `brain-aws` | Neutral DynamoDB and KMS adapters |
| `brain-server` | Standalone server binary and development composition |
| `@aexhq/brain` | TypeScript client, Tool API, worker, schemas, and builder |
| `@aexhq/brain-tools` | Portable Tool values selected explicitly by an application |

Hands implement Brain's public ports. Brain never imports a Hands implementation.

## Run standalone

```sh
export BRAIN_HAND_IMAGE='ghcr.io/aexhq/hands@sha256:<digest>'
export BRAIN_DATA_DIR="$PWD/brain-data"
cargo run --release -p brain-server --bin brain
```

Brain binds `127.0.0.1:3210` by default. Set `BRAIN_API_TOKEN`, or read the generated mode-0600
token from `$BRAIN_DATA_DIR/operator.token`. See [STANDALONE.md](STANDALONE.md) for Docker Compose,
backup, networking, and trust boundaries.

To embed Brain, implement `brain::adapter::{HandFactory, HandAdapter}` and any required journal,
key-custody, storage, or trusted-tool adapters, then compose them with `Brain::with_parts`.
`crates/brain/tests/custom_adapter.rs` is a complete third-party adapter example.

## Verify

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p brain-bench --release -- ci
npm ci
npm test
npm run package-smoke
```

Schemas, OpenAPI, protocol semantics, examples, generators, and conformance fixtures in this
repository are the source of truth. See [BENCHMARKS.md](BENCHMARKS.md) for methodology and reference
measurements.

Apache-2.0 licensed.
