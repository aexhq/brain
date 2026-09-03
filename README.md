<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>A minimal, blazing fast, extensible agent runtime.</strong></p>

<p align="center">
  <a href="https://github.com/aexhq/brain/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/aexhq/brain/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://www.npmjs.com/package/@aexhq/brain"><img alt="npm" src="https://img.shields.io/npm/v/%40aexhq%2Fbrain?label=%40aexhq%2Fbrain" /></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-1.97%2B-orange" />
</p>

<p align="center">
  <a href="https://aex.dev/brain/docs"><strong>Docs</strong></a> ·
  <a href="https://aex.dev/brain/docs/reference/api">API Reference</a> ·
  <a href="https://aex.dev/brain">Website</a> ·
  <a href="https://github.com/aexhq/extensions">Official extensions</a> ·
  <a href="README.cn.md">中文</a>
</p>

> [!NOTE]
> **Early preview.** The API and functionality may change without backward compatibility or
> notice until we cut 1.0.0.

## What is it

**Brain** is a minimal, and extensible agent runtime server. You assemble or write your own
extensions to control every aspect of the runtime.

### Agentloop Extensions
The core mechanism that bridge LLM, full control of context and dispatch tools.  [Write an agent loop](https://aex.dev/brain/docs/guides/write-a-loop).
- Pi
- Opencode
- Codex

### Tool Extensions
The hand for LLM to actually do work, it declares resources it needs and provide ability to interact. [Write a tool](https://aex.dev/brain/docs/guides/write-a-tool).
- Bash
- Inline function
- Web_search/Web_fetch

### Environment Extensions
An environment provide the resources a tool needs to complete its tasks. [Write an environment](https://aex.dev/brain/docs/guides/write-an-environment).
- Sandbox
- Browser
- Filesystem

### Official Extensions
We provide a number of official extensions, written in the same way you would: [aexhq/extensions](https://github.com/aexhq/extensions).


## Benchmarks
Brain is minimal, so it runs faster than alternatives on the market.
Be-aware that you normally add extensions to the runtime so that it can be useful, which means you don't normally see these benchmarks number in real use cases.
★ marks Brain in each chart.

**Turn round-trip**

```text
Brain      █                                        40 ms ★
ZeroClaw   ███                                      53 ms
OpenFang   ██████████                              128 ms
AgentScope ██████████████████                      338 ms
Letta      ███████████████████████                 678 ms
LangGraph  ████████████████████████████            1.22 s
Awaken     █████████████████████████████████       2.23 s
OpenClaw   ████████████████████████████████████    3.33 s
```

**Time to first token**

```text
Brain      █                                       2.9 ms ★
ZeroClaw   ████████                                 11 ms
OpenFang   ██████████████████                       70 ms
LangGraph  ████████████████████████                207 ms
AgentScope ███████████████████████████             332 ms
Letta      ████████████████████████████            407 ms
OpenClaw   ██████████████████████████████████      1.33 s
Awaken     ████████████████████████████████████    1.93 s
```

**New session**

```text
LangGraph  █                                      0.66 ms
Brain      ██                                     0.76 ms ★
ZeroClaw   ██████████                              2.2 ms
Awaken     ████████████████                        5.1 ms
OpenClaw   █████████████████                       5.4 ms
OpenFang   █████████████████████                   9.8 ms
Letta      ████████████████████████████████████     67 ms
```

**Memory per idle session**

```text
Brain      █                                        14 KiB ★
OpenFang   ██████████████                          0.6 MiB
ZeroClaw   ████████████████████████████             50 MiB
OpenClaw   ████████████████████████████████████    490 MiB
```

<sub>Each number is the median from the harness in <a href="tools/bench">tools/bench</a>, run on
the same AWS <code>c7g.xlarge</code> for every subject. Bars use a log scale. The chart includes
only agent runtimes that own sessions behind an API. <a href="BENCHMARKS.md">BENCHMARKS.md</a> has
the method and the subject versions.</sub>


## How it works

This is one turn from start to finish. The agent loop decides what to do next. Brain does the
I/O, writes the intent to the journal before acting, and streams the result while the turn is
still running.

```text
                    +-------------------------------+
   your app ------->| session state, in memory      |
            <-------| live event feed               |
                    +---------------+---------------+
                                    | activate
                                    v
                    +-------------------------------+
                    | agent loop, a Wasm component  |   
                    +---------------+---------------+
                                    | 
                                    v
                    +-------------------------------+        +-----------------+
                    | Brain does the I/O            |<------>| append-only log |
                    | for the loop                  | intent | off the turn's  |
                    +---------------+---------------+ result | hot path        |
                                    |                        +-----------------+
                                    +--> model provider, streaming
                                    |
                                    +--> tool, in any environment
```

Brain owns the session. You supply the agent loop, the model, the tools, and the environment.
Four design choices make it fast:

- **Isolated WebAssembly agent loops.** A loop compiles from any language to a
  [Wasmtime](https://wasmtime.dev/) component. It is compiled once and activated for each
  decision at native speed, fully sandboxed. Because Brain does the I/O, each decision is
  deterministic and can be replayed from its position in the log.
- **Write-ahead log.** The only durable state is an append-only log, written after the turn so
  it stays off the hot path. Sessions live in memory and rebuild from the log at boot, so a
  restart resumes the conversation where it stopped.
- **Everything is observable.** Every decision, model call, token, and tool
  result is an event in one feed. Watching live and reading history use the same records, so
  tracing a session is the same as replaying it.

Brain comes with a server, one native Rust binary on [Tokio](https://tokio.rs/). It serves the session API over
HTTP and SSE with [Axum](https://github.com/tokio-rs/axum) and needs no external store.

## Quick start

In this example the tool is a plain function in your own process. You declare it once and
pass it to the session. The SDK answers the model's calls from the session's event feed, so
your app needs no server, no open port, and no extra channel.

Run a server:

```sh
docker run --rm -p 127.0.0.1:8080:8080 \
  -e BRAIN_LISTEN=0.0.0.0:8080 -e BRAIN_API_TOKEN=quickstart \
  -v brain-data:/var/lib/brain ghcr.io/aexhq/brain:latest
```

```sh
npm install @aexhq/brain @aexhq/agentloop-pi zod
```

Save as `order.mjs` and run with `node order.mjs`:

```js
import { Brain, tool } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";
import { z } from "zod";

const orders = { "A-1001": { status: "shipped", eta: "Thursday" } };
const lookupOrder = tool({
  name: "lookup_order",
  description: "Look up an order's status by id.",
  input: z.object({ id: z.string() }),
  execute: ({ id }) => orders[id] ?? { status: "unknown order" },
});

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080", token: "quickstart" });
const session = await brain.sessions.create({
  model: { provider: "openai", name: "gpt-5-mini", apiKey: process.env.OPENAI_API_KEY },
  agentloop: pi(),
  tools: [lookupOrder],
});

await session.send("Where is order A-1001?");
for await (const event of session.events()) console.log(event.sequence, event.type);

await session.end();
await session.delete();
```

## Roadmap

- [x] The four-part runtime: agent loop, model, tools, environment
- [x] Unified `brain`, `tool`, and `environment` authoring with `brain build`
- [x] Append-only segment log with restart recovery
- [x] Typed content identity
- [x] HTTP/SSE session API and the `@aexhq/brain` SDK
- [x] Remote environment contract with `env-app` and `env-aws-microvm`
- [ ] Cross-session isolation test
- [ ] Native subagent support, parent and child links between sessions
- [ ] Multimodal input, images and files on `send`
- [ ] Freeze a v1 API with tagged releases
- [ ] File access and workspace sync
- [ ] crates.io publication
- [ ] Sessions spread across machines sharing environments
- [ ] `checkpoint` and `restore`
- [ ] Custom images with scoped credentials and network metering
- [ ] Local environment: run tools in a directory or container on your own machine
- [ ] Browser environment and DOM tools: a page as the place tools run
- [ ] An agent loop written in Rust against the same `agentloop.wit` contract

## Contact

For support and bug reports, open an [issue](https://github.com/aexhq/brain/issues) or write
to [support@aex.dev](mailto:support@aex.dev). For collaboration and partnerships, write to
[admin@aex.dev](mailto:admin@aex.dev).
