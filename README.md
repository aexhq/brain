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
  <a href="https://discord.gg/Qk2YnHMHVb"><img alt="Discord" src="https://img.shields.io/badge/discord-join-5865F2" /></a>
</p>

<p align="center">
  <a href="https://aex.dev/brain/docs"><strong>Docs</strong></a> ·
  <a href="https://aex.dev/brain/docs/reference/api">API Reference</a> ·
  <a href="https://aex.dev/brain">Website</a> ·
  <a href="https://github.com/aexhq/extensions">Official extensions</a> ·
  <a href="https://discord.gg/Qk2YnHMHVb">Discord</a>
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
Brain in each chart.[^bench]

**Turn round-trip**

```text
Brain             █                                     25 ms ★
ZeroClaw          ██                                    51 ms
LangGraph Server  ██████████████████████████████      1049 ms
OpenClaw          ████████████████████████████████████ 1257 ms
```

**Time to first token**

```text
ZeroClaw          █                                    9.6 ms
Brain             ██                                   ≤25 ms ★
LangGraph Server  ████                                48.6 ms
OpenFang          ██████                              75.8 ms
OpenClaw          ██████████████████████████████     874.4 ms
```

**New session**

```text
Brain             ██                                   0.6 ms ★
LangGraph Server  ███                                  0.7 ms
ZeroClaw          ██████                               1.9 ms
OpenClaw          ███████████                          3.7 ms
OpenFang          ██████████████████████████████      10.3 ms
```

**Cold start**

```text
ZeroClaw    █                                 10 ms
Brain       ██                                25 ms ★
OpenFang    ████                             180 ms
LangGraph   ███████████████                  2.5 s
CrewAI      █████████████████                3.0 s
AutoGen     ███████████████████              4.0 s
OpenClaw    ████████████████████████         5.98 s
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
requests, and a journal held to a small constant multiple of its final context. Earlier figures
are archived in [BENCHMARKS.md](BENCHMARKS.md).

[^bench]: Medians on AWS `c7g.xlarge`, Linux. Brain's first-token figure is an upper bound: under
    an instant scripted model the turn completes before a delta reaches the stream, so its
    whole-turn median stands in. Cold-start figures other than Brain's come from each project's
    own published numbers. Memory bars are log-scaled; Brain's is the marginal cost per
    additional idle session.

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
loop runs in a WebAssembly sandbox with no network, filesystem, secrets, or clock — Brain performs
every effect, so a decision replays from its position in the journal, and a crashing or hostile
tool takes down its own sandbox, not the process holding your sessions. Loops compile to Wasm from
any language; tools and environments speak plain HTTP.

What makes it fast is mostly what it refuses to do on the turn's path:

- **Journal behind the turn** — an append is a serialise, an xxh3, and a channel send; no syscall,
  no fsync. Session state and indexes live in memory.
- **Record the delta, not the state** — a turn keeps the messages it added, decisions by name, and
  model output streams once instead of being written down twice.
- **Everything unbounded got a bound** — a 64 MiB writer queue with backpressure, expiring
  idempotency records, a wall-clock deadline on the agent loop.
- **One of everything shared** — one HTTP client to the provider, one buffer per journal frame,
  one encode per activation, credentials cached.

Sessions survive a restart best effort, rebuilt from what reached the disk; a session interrupted
mid-turn comes back with a `turn_interrupted` event and lets the client decide.

## Features

- **Tools run anywhere** — a tool is plain HTTP in an environment you choose: a microVM sandbox,
  a browser driving the DOM, your own backend — and one session can span several at once.
- **Built for low overhead** — memory-resident session state, appends behind the turn, ~14 KiB
  per idle session, 25 ms round trips. The numbers above are measured, and CI holds them.
- **Bring your model, spawn your agents** — built-in bindings for the major LLM providers, sealed
  per session, and sessions that create sessions for subagent work.
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
npm install @aexhq/brain @aexhq/brain-pi
```

```ts
import { Brain } from "@aexhq/brain";
import { pi } from "@aexhq/brain-pi";

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
also to everyone filing issues and testing early builds on
[Discord](https://discord.gg/Qk2YnHMHVb).

## License

[MIT](LICENSE).
