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

```text
TypeScript application
        |
        | @aexhq/brain over HTTP
        v
+---------------------------- brain process ----------------------------+
| brain-http -> brain-server -> brain session kernel                    |
|                         |         |                                   |
|                         |         +-> SQLite journal                  |
|                         |         +-> in-memory context               |
|                         |                                             |
|                         +-> bounded Loophost worker                   |
|                         |      +-> Agentloop Component                |
|                         +-> model binding -> remote model API         |
|                         +-> Environment directory/cache               |
+--------------------------------------|--------------------------------+
                                       v
                         Environment adapter/provider
                         setup / attach / call / execute
                         cancel / detach / teardown
                                       |
                                       v
                         Tool runtime, sandbox, browser,
                         application, or user machine
```

## TypeScript client

```sh
npm install @aexhq/brain @aexhq/loop-pi @aexhq/env-aws-microvm @aexhq/tools
```

```ts
import { Brain } from "@aexhq/brain";
import { awsMicroVm } from "@aexhq/env-aws-microvm";
import { pi } from "@aexhq/loop-pi";
import { bash, read, write } from "@aexhq/tools";

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080" });
const workspace = awsMicroVm({ region: "eu-west-2" });

const session = await brain.createSession({
  model: {
    provider: "vercel-ai-gateway",
    name: "openai/gpt-5-mini",
    apiKey: process.env.VERCEL_AI_GATEWAY_API_KEY!,
  },
  agentLoop: pi(),
  tools: [read().runIn(workspace), write().runIn(workspace), bash().runIn(workspace)],
});

await session.send("Read README.md and summarize it.");
for await (const event of session.events()) console.log(event);
```

Omitting `tools` exposes no model tools. Components are ordinary immutable package values; the
official packages use the same public contract as third-party components.

## Run standalone

```sh
cargo build --release -p brain-server --bin brain -p brain-loophost --bin brain-loop-worker
BRAIN_DATA_DIR="$PWD/brain-data" \
BRAIN_LOOP_WORKER="$PWD/target/release/brain-loop-worker" \
./target/release/brain --listen 127.0.0.1:8080
```

## Run the examples locally

On Linux, with the standalone server running, install the repository dependencies and provide a
Vercel AI Gateway key:

```sh
npm ci
export VERCEL_AI_GATEWAY_API_KEY="..."
npm run example:basic
```

The [`examples/`](examples/) folder also covers event cursors, session lifecycle, and the raw HTTP
API. See [`examples/README.md`](examples/README.md) for every command and optional setting.

## Embed Brain

Supply the Agentloop, Model, Tool, and telemetry ports, then open the same session kernel used by
Brain Server:

```rust,ignore
let kernel = brain::Kernel::open(
    brain::KernelConfig {
        data_dir,
        max_decisions_per_turn: 128,
        loop_executor,
        model_executor,
        tool_executor,
    },
    telemetry,
)?;

let session = kernel.create_session(sealed_session_config).await?;
session
    .message(brain_protocol::MessageRequest {
        content: serde_json::json!("Explain the current changes."),
    })
    .await?;
```

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
