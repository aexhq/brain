<h1 align="center">Brain</h1>

<p align="center"><strong>A minimal, durable, and extensible session engine for agents.</strong></p>
<p align="center">
  <a href="contracts/session/v1/openapi.yaml">HTTP API</a> ·
  <a href="https://github.com/aexhq/extensions">Extensions</a> ·
  <a href="https://discord.gg/Qk2YnHMHVb">Discord</a>
</p>

> Brain is pre-launch. Contracts are replaced in place until the first stable release; old
> development interfaces are not retained.

## What it is

Brain is an independently runnable Linux server and an embeddable Rust session kernel. It keeps an
ordered session journal on disk, holds active materialized context in memory, runs the selected
Agentloop, calls remote language models, and dispatches Tools to their bound remote Environments.

Brain owns reasoning and orchestration. Environments provide the hands: sandboxes, browsers,
applications, user machines, and other places where Tools actually run.

## Architecture

### Durable sessions, remote execution

Every execution intent is committed to SQLite before Brain calls an Agentloop, model, or
Environment. A terminal result or explicit ambiguous outcome is committed before the next
transition. A restart reconstructs state from the journal; Brain never guesses whether an
interrupted external effect happened.

### Environment-neutral

A Tool is a model-visible definition plus a sealed binding to one logical Environment. Brain Server
orchestrates setup, attachment, execution, cancellation, detachment, and teardown through a remote
adapter. Tool code never executes inside Brain.

### Language-neutral

Agentloops are portable WebAssembly Components. Authors use a language SDK and build command rather
than writing WIT or choosing Wasmtime. Environment implementations use a versioned HTTP contract
and may be written in any language.

### Agent-neutral

Pi-, Codex-, OpenCode-, and custom-style Agentloops use the same isolated extension pipeline. There
is no privileged native Agentloop path.

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
|                         +-> shared model client -> remote model API   |
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

A hosted deployment supplies a shared Environment directory and session placement. Process-local
caches improve latency but do not decide Environment identity, authority, or teardown.

## Components

| Component | Responsibility |
| --- | --- |
| `brain-protocol` | Canonical session, Agentloop, model, Tool, Environment, event, and error contracts |
| `brain-telemetry` | Bounded, nonblocking logs, metrics, traces, and live event projections |
| `brain-loophost` | Agentloop admission, Wasmtime compilation, worker isolation, and resource limits |
| `brain` | Disk journal, context, operation identity, turn state machine, and execution ports |
| `brain-http` | Versioned HTTP routing, transport validation, and error mapping |
| `brain-server` | Runnable composition, shared resources, lifecycle, and Environment routing |
| `@aexhq/brain` | TypeScript client for a caller-supplied Brain base URL |
| `@aexhq/agentloop` | TypeScript Agentloop authoring and packaging, maintained in Extensions |

There is no Toolhost, Envhost, Modelhost, cloud SDK, or second standalone runtime in this repository.

## TypeScript example

```ts
import { readFile } from "node:fs/promises";
import { Brain } from "@aexhq/brain";

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080" });
const admitted = await brain.admitAgentloop(
  new Uint8Array(await readFile("./dist/loop.brain.json")),
  crypto.randomUUID(),
);

const session = await brain.createSession({
  agentloop_digest: admitted.digest,
  model: { binding_id: "gateway", model: "openai/gpt-5-mini" },
  presentation: {
    system: "You are a concise coding assistant.",
    tools: [{
      name: "read",
      description: "Read a file from the workspace.",
      input_schema: { type: "object", required: ["path"], properties: { path: { type: "string" } } },
    }],
  },
  environments: [{
    environment_id: "workspace-main",
    configuration: { workspace: "main" },
    lifecycle_policy: "shared",
  }],
  tool_bindings: [{
    name: "read",
    environment_id: "workspace-main",
    remote_tool_id: "read",
    grant: { paths: ["**"] },
  }],
}, crypto.randomUUID());

await session.send("Read README.md and summarize it.", crypto.randomUUID());
for await (const event of session.events()) console.log(event);
```

Omit both `presentation.tools` and `tool_bindings` for a session with no Tools.

## Run standalone

Build both Linux binaries and provide a writable data directory and model credential:

```sh
cargo build --release -p brain-server --bin brain -p brain-loophost --bin brain-loop-worker
export BRAIN_DATA_DIR="$PWD/brain-data"
export BRAIN_LOOP_WORKER="$PWD/target/release/brain-loop-worker"
export BRAIN_MODEL_API_KEY="..."
export BRAIN_MODEL_BASE_URL="https://ai-gateway.vercel.sh/v1"
./target/release/brain --listen 127.0.0.1:8080
```

Set `BRAIN_ENVIRONMENT_BASE_URL` only when sessions use Tools. Standalone and hosted Brain use the
same contracts and kernel; a hosted composition injects distributed routing and storage.

## Embed a Brain session

The `brain` crate contains no HTTP or cloud policy. A Rust host supplies the same three execution
ports used by Brain Server:

```rust,ignore
let (telemetry, telemetry_worker) = brain_telemetry::telemetry_channel();
tokio::spawn(telemetry_worker.run(telemetry_sink));

let kernel = brain::Kernel::open(brain::KernelConfig {
    data_dir,
    max_decisions_per_turn: 128,
    loop_executor,
    model_executor,
    tool_executor,
}, telemetry)?;

let session = kernel.create_session(sealed_session_config).await?;
session.message(brain_protocol::MessageRequest {
    content: serde_json::json!("Explain the current changes."),
}).await?;

for event in kernel.events(session.id(), 0, 1_000)?.events {
    println!("{event:?}");
}
```

## Verification

```sh
npm ci
npm run gen
npm test
npm run package-smoke
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

CI is the release gate. No test is skipped because its required runtime belongs in another job.

Licensed under [Apache 2.0](LICENSE).
