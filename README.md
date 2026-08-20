# Brain

Brain is an independent, Apache-2.0 session engine. One long-lived server owns many durable model
sessions: their immutable prefixes, provider rounds, tool-call journal, recovery, cancellation,
and event streams. Aex is a downstream product built on Brain; Brain does not require an Aex
account or control plane.

Brain starts with zero model-visible tools. Applications explicitly select ordinary `Tool` values
from `@aexhq/brain-tools`, a third-party package, or their own source. The exact order,
definitions, executor bindings, and executable checksums are frozen for the session. Core dispatch
routes from those sealed executor descriptors and never from a model-visible name.

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
    name: "gpt-5",
    apiKey: process.env.OPENAI_API_KEY!,
  },
  tools: [echo],
});
```

Deployable TypeScript tools run in the session's default Hand. The local builder creates a
deterministic Node 22 ESM bundle without importing the customer's module; its first evaluation is
inside the Hand, after Brain has durably recorded the call intent. `echo.local()` is the explicit
alternative for a callback in an attached application process. Server capabilities and remote MCP
tools use the same definition/executor model.

## Repository map

| Crate or package | Responsibility |
| --- | --- |
| `brain-protocol` | Brain-owned session API and Brain↔Hand wire types |
| `brain-hand-client` | Neutral client for the public Hand protocol |
| `brain` | Multi-session engine, providers, tool router, API, recovery, and public adapter ports |
| `brain-standalone` | SQLite journal, local encrypted custody/storage, and Docker Hand adapter |
| `brain-aws` | Neutral DynamoDB and KMS adapters; no Hands implementation |
| `brain-server` | Standalone server binary and explicit development composition |
| `@aexhq/brain` | TypeScript client, Tool API, attached worker, schemas, and local builder |
| `@aexhq/brain-tools` | Portable official Tool values, selected explicitly |

Hands implement Brain's public ports. Brain never imports a crate from the Hands repository.

## Run standalone

The default server mode is durable single-node operation with SQLite, a local AES-256-GCM master
key, local files, and one Docker Hand per active session:

```sh
export BRAIN_HAND_IMAGE='ghcr.io/aexhq/hands@sha256:<digest>'
export BRAIN_DATA_DIR="$PWD/brain-data"
cargo run --release -p brain-server --bin brain
```

It binds `127.0.0.1:3210` by default. Set `BRAIN_API_TOKEN`, or read the generated mode-0600 token
from `$BRAIN_DATA_DIR/operator.token`. Brain fails closed if Docker, the configured image, SQLite,
the custody key, or durable Hand state cannot be opened. It never falls back to an in-memory mode.

See [STANDALONE.md](STANDALONE.md) for Docker Compose, backup, network, and trust-boundary details.
Containers share the host kernel and are not advertised as hostile multi-tenant isolation.

For tests only, `BRAIN_MODE=development` selects the in-memory journal and unsandboxed host
subprocess adapter. Sessions in that mode do not survive a restart.

## Embed Brain

Implement `brain::adapter::{HandFactory, HandAdapter}` and, when needed,
`journal::JournalStore`, `keys::KeyCustody`, or the trusted `ToolExecutor` seam. Compose those
public ports with `Brain::with_parts`. The engine owns protocol and journal invariants; the Hand
implementation owns its isolation and lifecycle. `crates/brain/tests/custom_adapter.rs` is an
end-to-end third-party adapter example.

## Build and test

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p brain-bench --release -- ci
npm ci
npm test
npm run package-smoke
```

The benchmark and leakage methodology is in [BENCHMARKS.md](BENCHMARKS.md). The session schemas,
OpenAPI document, Hand protocol, examples, generators, and conformance fixtures in this repository
are the neutral source of truth.

License: Apache-2.0.
