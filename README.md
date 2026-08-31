<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>A tiny, blazing fast, extensible agent runtime.</strong></p>

<p align="center">
  <a href="https://github.com/aexhq/brain/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/aexhq/brain/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://www.npmjs.com/package/@aexhq/brain"><img alt="npm" src="https://img.shields.io/npm/v/%40aexhq%2Fbrain?label=%40aexhq%2Fbrain" /></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue" /></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-1.97%2B-orange" />
</p>

<p align="center">
  <a href="https://aex.dev/brain/docs"><strong>Docs</strong></a> ·
  <a href="https://aex.dev/brain/docs/reference/api">API Reference</a> ·
  <a href="https://aex.dev/brain">Website</a> ·
  <a href="https://github.com/aexhq/extensions">Official extensions</a>
</p>

## What is it

Brain is a tiny agent runtime that runs sessions: it holds the conversation, decides what happens
next, calls the model, hands out tool calls, and journals every step — about 7,300 lines of Rust.
The **agent loop**, the **model**, the **tools**, and the **environment** they run in all plug in
and are yours to replace, whether you run Brain as an HTTP server or embed the `brain` crate in a
Rust service you already own.

> [!NOTE]
> **Brain is under early development.** Contracts are replaced in place until the first stable
> release, and there is no upgrade path from earlier builds. APIs, package names, and wire formats
> will change without notice.

## Benchmarks

Same machine, same scripted model behind every subject — no model latency in any number. ★ marks
Brain in each chart.

**Turn round-trip**

```text
Brain      █                                     25 ms ★
ZeroClaw   ██                                    51 ms
LangGraph  ██████████████████████████████      1049 ms
OpenClaw   ████████████████████████████████████ 1257 ms
```

**Time to first token**

```text
ZeroClaw   █                                    9.6 ms
Brain      ██                                   ≤25 ms ★
LangGraph  ████                                48.6 ms
OpenFang   ██████                              75.8 ms
OpenClaw   ██████████████████████████████     874.4 ms
```

**New session**

```text
Brain      ██                                   0.6 ms ★
LangGraph  ███                                  0.7 ms
ZeroClaw   ██████                               1.9 ms
OpenClaw   ███████████                          3.7 ms
OpenFang   ██████████████████████████████      10.3 ms
```

**Cold start**

```text
ZeroClaw   █                                 10 ms
Brain      ██                                25 ms ★
OpenFang   ████                             180 ms
LangGraph  ███████████████                  2.5 s
OpenClaw   ████████████████████████         5.98 s
```

**Memory per idle session**

```text
Brain     █                                 14 KiB ★
OpenFang  ████████                         0.6 MiB
ZeroClaw  █████████████████████             50 MiB
OpenClaw  ██████████████████████████████   490 MiB
```

Disk stays flat — a 100-turn conversation leaves **0.2 MiB**, the hundredth turn writing the same
2.3 KiB as the first — and CI enforces bounds on every push: under 256 MiB resident after 10,000
requests, and a journal held to a small constant multiple of its final context.

> **Benchmark setup.** Medians, measured by the harness in [`tools/bench`](tools/bench) on an AWS
> `c7g.xlarge` (Linux) with the same instant scripted model behind every subject. The LangGraph
> figures measure LangGraph Server. Brain's first-token figure is an upper bound: the scripted
> turn completes before a delta reaches the stream, so its whole-turn median stands in. Cold-start
> figures other than Brain's come from each project's own published numbers. Memory bars are
> log-scaled, and Brain's is the marginal cost per additional idle session. A subject absent from
> a chart has no measured figure for that probe. Methodology and the bounds CI enforces are in
> [BENCHMARKS.md](BENCHMARKS.md).

## How it works

```text
your app
   ▲
   │ HTTP / SSE
   ▼
[ brain runtime ]
   │
   ├── observation ──► agent loop ──► decision
   │                   (Wasm, sealed)
   │
   ├── pinned model call ──► model API
   │
   ├── tool call (HTTP) ──► environment
   │                        (sandbox, browser,
   │                         your backend)
   │
   └── append, behind the turn ──► segment log
```

Brain owns the session; the agent loop, model, tools, and environment are yours to supply. The
loop compiles to WebAssembly from any language and runs on [Wasmtime](https://wasmtime.dev/)'s
component model, with Brain performing every effect on its behalf — which is what makes a
decision deterministic and replayable from its position in the journal. Tools and environments
speak plain HTTP, so they run wherever you want them.

The runtime itself is Rust end to end, and its speed comes from the architecture rather than
tuning:

- **Rust on [Tokio](https://tokio.rs/)** — the whole runtime is one native async binary, serving
  the session API over [Axum](https://github.com/tokio-rs/axum) HTTP/SSE.
- **Memory-resident sessions** — session state and indexes live in memory, backed by an
  append-only journal written behind the turn, off the hot path.
- **Pre-compiled agent loops** — a loop compiles once through Wasmtime and activates per decision
  at native speed.
- **Streaming end to end** — model output streams through the event feed as it arrives, token by
  token, instead of being buffered per turn.

Sessions survive a restart, rebuilt from the journal; a session interrupted mid-turn comes back
with a `turn_interrupted` event and lets the client decide.

## Features

- **Tools run anywhere** — a tool is plain HTTP in an environment you choose: a microVM sandbox,
  a browser driving the DOM, your own backend — and one session can span several at once.
- **Built for low overhead** — memory-resident session state, appends behind the turn, ~14 KiB
  per idle session, 25 ms round trips. The numbers above are measured, and CI holds them.
- **Bring your model, spawn your agents** — built-in bindings for 70+ LLM providers via
  [models.dev](https://models.dev), sealed per session, and sessions that create sessions for
  subagent work.
- **Sealed extension execution** — agent loops compile to WebAssembly and run in a standalone
  runtime with no network, filesystem, secrets, or clock; Brain performs every effect.
- **Observable end to end** — every observation, decision, model intent, and tool result is an
  event you can stream live or read back later, token by token while the turn runs.

## Quick start

Run a server:

```sh
docker run --rm -p 8080:8080 -v brain-data:/var/lib/brain ghcr.io/aexhq/brain:latest
```

Drive a session from TypeScript:

```sh
npm install @aexhq/brain @aexhq/agentloop-pi
```

```ts
import { Brain } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080" });

const session = await brain.sessions.create({
  model: {
    provider: "openai",
    name: "gpt-5-mini",
    apiKey: process.env.OPENAI_API_KEY!,
  },
  agentloop: pi(),
  system: "Answer briefly and directly.",
});

await session.send("Explain what a session runtime does, in one sentence.");
for await (const event of session.events()) console.log(event);

await session.end();
await session.delete();
```

No `tools` means the model sees none. Add them once you have somewhere to run them. Brain listens on
loopback and needs no token there; set `BRAIN_API_TOKEN` to listen anywhere else.

Four runnable scripts — a basic session, event history, the full lifecycle, and the same thing over
raw HTTP with no SDK — are in [`examples/`](examples). Building from source and embedding the crate
in Rust are covered in the [Quickstart](https://aex.dev/brain/docs/quickstart) and the
[embedding guide](https://aex.dev/brain/docs/guides/embed). Setup, the verification commands CI
runs, and how contracts change are in [CONTRIBUTING.md](CONTRIBUTING.md) — issues and pull requests
are welcome.

## Roadmap

- [x] The four-part runtime: agent loop, model, tools, environment
- [x] Unified `brain`, `tool`, and `environment` authoring with `brain build`
- [x] Append-only segment log with restart recovery
- [x] Content identity as a type rather than a digest string
- [x] HTTP/SSE session API and the `@aexhq/brain` SDK
- [x] Remote environment contract with `env-app` and `env-aws-microvm`
- [ ] Cross-session isolation test
- [ ] Multimodal input — images and files on `send`
- [ ] Freeze a v1 API with tagged releases
- [ ] File access and workspace sync
- [ ] crates.io publication
- [ ] Sessions spread across machines sharing environments
- [ ] `checkpoint` and `restore`
- [ ] Custom images with scoped credentials and network metering

## Acknowledgements

Brain stands on [Wasmtime](https://wasmtime.dev/) and the Bytecode Alliance's component model,
which is what lets an agent loop written in any language run sealed off and reproducible. The
benchmark would mean nothing without the projects it measures — LangGraph, ZeroClaw, OpenFang,
OpenClaw, Letta, CrewAI, AutoGen, the Microsoft Agent Framework, E2B, Firecracker, Daytona, and
Modal — and several of them shaped how Brain thinks about what a runtime owes its operator. Thanks
also to everyone filing issues and testing early builds.

## License

[MIT](LICENSE).
