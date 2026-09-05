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
An environment provides the resources a tool needs to complete its tasks. [Write an environment](https://aex.dev/brain/docs/guides/write-an-environment).
- Sandbox
- Browser
- Filesystem

### Official Extensions
We provide a number of official extensions, written in the same way you would: [aexhq/extensions](https://github.com/aexhq/extensions).

`brainWasm(options)` is Brain's built-in Wasmtime Environment. Network targets, secret names, and
writable `scratch` or `workspace` roots must appear in both the session request and the server
deployment policy; every server grant is empty by default.
Each native invocation is bounded by 10 billion Wasmtime fuel units for guest work; suspended I/O
does not consume fuel, while the session's wall-time limit still bounds the complete turn.


## How it works

This is one turn from start to finish. The Agentloop decides what to do next. Brain durably
commits each external-effect intent before dispatch, sends it once, and streams live output while
the turn is still running. Committed records and private state changes share one canonical journal;
session status, transcript, Agentloop state, and the public event feed are projections of it.

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
                    | Brain does the I/O            |<------>| canonical       |
                    | for the loop                  | commit | journal         |
                    +---------------+---------------+ result |                 |
                                    |                        +-----------------+
                                    +--> model provider, streaming
                                    |
                                    +--> placed tool, in its environment
                                    `--> resident tool, over host SSE
```

Brain owns the session. You supply the agent loop, the model, the tools, and the environment.
Three design choices make it fast:

- **Isolated WebAssembly agent loops.** Brain receives a precompiled
  [Wasmtime](https://wasmtime.dev/) Component; compilation and source-language tooling are outside
  its runtime contract. The Component runs each turn in a capability sandbox and calls back into
  Brain for model calls and tool calls. Because Brain does the I/O, every effect is in the log
  before it happens.
- **One write-ahead journal.** The journal is the only durable session truth. Brain commits an
  effect's intent before dispatch and never retries it automatically. An uncertain remote result is
  recorded as unknown. In-memory status, transcript, Agentloop state, and event indexes rebuild as
  projections after a restart.
- **Everything is observable.** Model calls, Tool results, lifecycle changes, transcript
  replacements, and records the loop appends are committed Events projected from the journal. The
  live feed also carries transient token deltas; reconnecting resumes at a committed sequence and
  receives the completed result.

Brain ships a native Rust server and an isolated Loophost worker on [Tokio](https://tokio.rs/).
The server exposes HTTP and SSE with [Axum](https://github.com/tokio-rs/axum) and needs no external
store for a local deployment.

## Quick start

In this example the tool is a plain function in your own process. You declare it once and
pass it to the session. The SDK registers one application host and answers commands over SSE, so
your app needs no inbound server or open port.

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
import { Brain, brainWasm, tool } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";
import { z } from "zod";

const orders = { "A-1001": { status: "shipped", eta: "Thursday" } };
const lookupOrder = tool({
  name: "lookup_order",
  description: "Look up an order's status by id.",
  input: z.object({ id: z.string() }),
  run: ({ id }) => orders[id] ?? { status: "unknown order" },
});

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080", token: "quickstart" });
const wasm = brainWasm();
const session = await brain.sessions.create({
  model: { provider: "openai", name: "gpt-5-mini", apiKey: process.env.OPENAI_API_KEY },
  agentloop: pi({ env: wasm }),
  tools: [lookupOrder()],
});

await session.send("Where is order A-1001?");
for await (const event of session.events()) console.log(event.sequence, event.type);

await session.end();
await session.delete();
```

## Benchmarks
Brain is minimal, so it runs faster than alternatives on the market. No model latency in any number. ★ marks Brain in each chart.

**Turn round-trip**

```text
pi         █                                       5.1 ms
Brain      ████████████                             40 ms ★
Codex      █████████████                            47 ms
ZeroClaw   ██████████████                           53 ms
OpenFang   ██████████████████                      128 ms
OpenCode   ███████████████████                     155 ms
AgentScope ████████████████████████                338 ms
Letta      ███████████████████████████             678 ms
LangGraph  ███████████████████████████████         1.22 s
Awaken     ██████████████████████████████████      2.23 s
OpenClaw   ████████████████████████████████████    3.33 s
```

**Time to first token**

```text
Brain      █                                       2.9 ms ★
pi         █████                                   6.3 ms
ZeroClaw   ████████                                 11 ms
Codex      ███████████████                          39 ms
OpenFang   ██████████████████                       70 ms
OpenCode   ████████████████████                     99 ms
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
OpenCode   ██████████                              2.1 ms
ZeroClaw   ██████████                              2.2 ms
Awaken     ████████████████                        5.1 ms
OpenClaw   █████████████████                       5.4 ms
pi         ███████████████████                     7.0 ms
OpenFang   █████████████████████                   9.8 ms
Codex      ████████████████████████                 14 ms
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

## Roadmap

- [x] The four-part runtime: agent loop, model, tools, environment
- [x] Raw WebAssembly Components supplied with `component(...)`
- [x] Explicit Agentloop and Tool placement with `{ env, ...options }`
- [x] One canonical per-session journal with disposable projections
- [x] Effect-after-commit with no automatic retries
- [x] Session-owned Environments with a managed idle lifecycle
- [x] Typed content identity
- [x] HTTP/SSE session API and the `@aexhq/brain` SDK
- [x] Remote Environment contract and `env-aws-microvm`
- [x] `brainWasm` placement with deployment-granted HTTP, secrets, scratch, and workspace access
- [x] Resident Tool host over SSE with durable `ctx.emit`
- [x] Cross-session native workspace isolation test
- [ ] Native subagent support, parent and child links between sessions
- [ ] Multimodal input, images and files on `send`
- [ ] Freeze a v1 API with tagged releases
- [ ] File access and workspace sync
- [ ] crates.io publication
- [ ] Sessions spread across machines sharing environments
- [ ] Session export and import
- [ ] Custom images with scoped credentials and network metering
- [ ] Local environment: run tools in a directory or container on your own machine
- [ ] Browser environment and DOM tools: a page as the place tools run
- [ ] Post-MVP external `SessionStore` for shared ownership, node-loss durability, backup, and
  regional recovery; no storage integration catalogue in the MVP
- [ ] Post-MVP bounded client reorder buffer if parallel delivery can emit committed Events out of
  journal sequence; replay repairs gaps

## Contact

For support and bug reports, open an [issue](https://github.com/aexhq/brain/issues) or write
to [support@aex.dev](mailto:support@aex.dev). For collaboration and partnerships, write to
[admin@aex.dev](mailto:admin@aex.dev).
