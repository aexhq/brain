<h1 align="center">Brain</h1>

<p align="center"><strong>The durable session kernel for AI agents.</strong></p>

<p align="center">
  <a href="https://github.com/aexhq/brain/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/aexhq/brain/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://www.npmjs.com/package/@aexhq/brain"><img alt="npm" src="https://img.shields.io/npm/v/%40aexhq%2Fbrain?label=%40aexhq%2Fbrain" /></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue" /></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-1.97%2B-orange" />
  <a href="https://discord.gg/Qk2YnHMHVb"><img alt="Discord" src="https://img.shields.io/badge/discord-join-5865F2" /></a>
</p>

<p align="center">
  <a href="https://aex.dev/brain/docs"><strong>Docs</strong></a> ·
  <a href="https://aex.dev/brain">Website</a> ·
  <a href="https://aex.dev/brain/docs/reference/api">API Reference</a> ·
  <a href="https://github.com/aexhq/extensions">Extensions</a> ·
  <a href="https://discord.gg/Qk2YnHMHVb">Discord</a>
</p>

> [!NOTE]
> **Brain is under early development.** Contracts are replaced in place until the first stable
> release, and there is no upgrade path from earlier builds. APIs, package names, and wire formats
> will change without notice.

## What it is

Brain runs agent sessions. It holds the conversation, decides what happens next, calls the model,
hands out tool calls, and writes all of it to a durable log. That is the entire job.

Four things plug in, and all four are yours to replace: the **agent loop**, the **model**, the
**tools**, and the **environment** tools run in. The packages we ship use the same interface you
would — nothing built in gets a shortcut.

The name comes from Anthropic's split of
[the brain from the hands](https://www.anthropic.com/engineering/managed-agents). Brain is the
brain: it decides. Environments are the hands — a sandbox, a browser, your backend, someone's
laptop — where the work actually happens. The small-and-extensible shape follows
[Pi](https://github.com/earendil-works/pi).

## Features

| | |
| --- | --- |
| **Sessions survive crashes** | Brain writes each step to disk before it runs. Kill the process mid-turn, start it again, and the session carries on from where it stopped. |
| **Tools run wherever you want** | Brain never executes tool code. It calls whatever you bind the tool to — a sandbox VM, a browser tab, your own backend, the user's laptop. |
| **Any language** | Agent loops compile to WebAssembly. Tools and environments talk to Brain over plain HTTP. One tool in Rust and another in Node, in the same session. |
| **Any agent loop** | Pi, Codex-style, or your own. Brain is not an agent — it is what agents run on, and the loop we ship has no privileges yours doesn't. |
| **Any model** | Anthropic and OpenAI wire formats, gateways, your own keys. The model is pinned when the session starts, so nothing swaps it out mid-conversation. |
| **The loop is sealed off** | An agent loop gets an observation and returns a decision. No network, no filesystem, no secrets, no clock. Brain performs every effect. |
| **More than one machine** | Environments are addressed by a stable name, so two sessions on two servers can share one workspace when you want them to. |
| **Server or library** | Run the binary against a log directory, or embed the `brain` crate in your own Rust service and supply your own storage and transport. |
| **Everything is an event log** | A session is an ordered, replayable log of what happened. Live streaming sits on top and drops events rather than stalling a turn. |

## Benchmark

> **{N}× faster first byte, {N}× smaller sessions, and it survives a crash.**
> _Numbers below are placeholders pending re-measurement on the current kernel — see
> [BENCHMARKS.md](BENCHMARKS.md) for methodology._

<!-- Pre-architecture-reset reference (2026-08-18, c7g.xlarge): TTFB 1.4 ms p50, turn 2.3 ms p50,
     2,002 turns/s at 64 sessions, ~3,430 tool calls/s, 21-31 KiB per resident session.
     Do not publish these as current: the kernel was rebuilt after they were taken. -->

| | Brain | LangGraph Server | Temporal-backed loop | Plain in-process loop |
| --- | ---: | ---: | ---: | ---: |
| First visible byte, one session | TBD | TBD | TBD | TBD |
| Complete text turn, one session | TBD | TBD | TBD | TBD |
| Throughput, 64 sessions | TBD | TBD | TBD | TBD |
| Tool calls per second, 64 sessions | TBD | TBD | TBD | TBD |
| Memory per live session | TBD | TBD | TBD | TBD |
| Survives process death mid-turn | **yes** | TBD | TBD | no |
| Tool code isolated from the kernel | **yes** | TBD | TBD | no |

The benchmark measures the engine, not a model: it drives the real HTTP and SSE paths with an
instant scripted provider and an in-process echo environment, so nothing here is model latency.
The harness is being rebuilt against the current kernel — see [BENCHMARKS.md](BENCHMARKS.md).

## Architecture

Brain owns the session. Four kinds of component plug into it.

| Kind | You supply | Brain does |
| --- | --- | --- |
| **Agent loop** | The policy: given what just happened, what next | Runs it in a WebAssembly sandbox and carries out the decision |
| **Model** | A binding: provider, model name, key | Pins it for the life of the session and makes the call |
| **Tool** | A name, description, schema, and where it runs | Logs the call and sends it to the bound environment |
| **Environment** | Somewhere tool calls actually execute | Sets it up, attaches, calls, cancels, tears it down |

```text
TypeScript application
        |
        | @aexhq/brain over HTTP/SSE
        v
+---------------------------- brain process ----------------------------+
| brain-http -> brain-server -> brain session kernel                    |
|                         |         |                                   |
|                         |         +-> session log on disk             |
|                         |         +-> context in memory               |
|                         |                                             |
|                         +-> loop workers -> agent loops (Wasm)        |
|                         +-> shared model clients -> model APIs        |
|                         +-> environment lookup/cache                  |
|                                      |                                |
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

| Crate / package | What it does |
| --- | --- |
| `brain-protocol` | The session, loop, model, tool, environment, event, and error contracts |
| `brain-telemetry` | Logs, traces, metrics, and the live event stream |
| `brain-loophost` | Loads Brain Components, compiles and caches Wasm, isolates workers, enforces limits |
| `brain` | The session log, the context, the turn loop, recovery, and dispatch |
| `brain-http` | HTTP and SSE routing, validation, error mapping |
| `brain-server` | The runnable server, session lifecycle, environment adapters |
| `@aexhq/brain` | TypeScript client for any Brain URL |

The schemas and OpenAPI document under [`contracts/`](contracts) are the source of truth, and the
[API Reference](https://aex.dev/brain/docs/reference/api) is generated from them.

## Roadmap

| | | |
| --- | --- | --- |
| ✅ | Four-part kernel: agent loop, model, tool, environment | Shipped |
| ✅ | Unified `brain`, `tool`, and `environment` authoring with `brain build` | Shipped |
| ✅ | Append-only segment log, written behind the turn, replayed on restart | Shipped |
| ✅ | HTTP/SSE session API and the `@aexhq/brain` TypeScript SDK | Shipped |
| ✅ | Remote environment contract with `env-app` and `env-aws-microvm` | Shipped |
| 🚧 | Storage split apart from sandboxing | In progress |
| 🚧 | Benchmarks and the cross-session isolation test, rebuilt on the current kernel | In progress |
| 🚧 | A frozen v1 API and tagged releases | In progress |
| ☐ | MCP client | Next |
| ☐ | Subagents | Next |
| ☐ | File access and workspace sync | Next |
| ☐ | `web_search` and `web_fetch` | Next |
| ☐ | crates.io publication | Next |
| ☐ | Sessions spread across machines, sharing environments | Later |
| ☐ | `checkpoint` and `restore` | Later |
| ☐ | Custom images, scoped credentials, network metering | Later |
| ☐ | Hosted Brain at [aex.dev](https://aex.dev/brain) | Later |

## Getting started

### Drive a session from TypeScript

```sh
npm install @aexhq/brain @aexhq/brain-pi @aexhq/env-aws-microvm @aexhq/tools
```

```ts
import { Brain } from "@aexhq/brain";
import { awsMicroVm } from "@aexhq/env-aws-microvm";
import { pi } from "@aexhq/brain-pi";
import { bash, read, write } from "@aexhq/tools";

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080" });
const workspace = awsMicroVm({ region: "eu-west-2" });

const session = await brain.sessions.create({
  model: {
    provider: "vercel-ai-gateway",
    name: "openai/gpt-5-mini",
    apiKey: process.env.VERCEL_AI_GATEWAY_API_KEY!,
  },
  brain: pi(),
  tools: [read().useIn(workspace), write().useIn(workspace), bash().useIn(workspace)],
});

await session.send("Read README.md and summarize it.");
for await (const event of session.events()) console.log(event);
```

Leave out `tools` and the model sees none.

### Run Brain

```sh
docker run --rm -p 8080:8080 -v brain-data:/var/lib/brain ghcr.io/aexhq/brain:latest
```

Or from source:

```sh
cargo build --release -p brain-server --bin brain -p brain-loophost --bin brain-loop-worker
BRAIN_DATA_DIR="$PWD/brain-data" \
BRAIN_LOOP_WORKER="$PWD/target/release/brain-loop-worker" \
./target/release/brain --listen 127.0.0.1:8080
```

Brain listens on loopback by default. Set `BRAIN_API_TOKEN` to listen anywhere else.

### Embed Brain in Rust

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

Guides, concepts, and the API reference: **[aex.dev/brain/docs](https://aex.dev/brain/docs)**.
Setting up to contribute: [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE).
