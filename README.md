<h1 align="center">Brain</h1>

<p align="center"><strong>A tiny, blazing fast, extensible agent kernel.</strong></p>

<p align="center">
  <a href="https://github.com/aexhq/brain/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/aexhq/brain/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://www.npmjs.com/package/@aexhq/brain"><img alt="npm" src="https://img.shields.io/npm/v/%40aexhq%2Fbrain?label=%40aexhq%2Fbrain" /></a>
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue" /></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-1.97%2B-orange" />
  <a href="https://discord.gg/Qk2YnHMHVb"><img alt="Discord" src="https://img.shields.io/badge/discord-join-5865F2" /></a>
</p>

<p align="center">
  <a href="https://aex.dev/brain/docs"><strong>Docs</strong></a> ·
  <a href="https://aex.dev/brain/docs/quickstart">Quickstart</a> ·
  <a href="https://aex.dev/brain/docs/reference/api">API Reference</a> ·
  <a href="https://aex.dev/brain">Website</a> ·
  <a href="https://github.com/aexhq/extensions">Extensions</a> ·
  <a href="https://discord.gg/Qk2YnHMHVb">Discord</a>
</p>

Brain runs agent sessions. It holds the conversation, decides what happens next, calls the model,
hands out tool calls, and appends every step to a log. That is the entire job, in about 7,300 lines
of Rust across six crates.

Four things plug into it, and all four are yours to replace: the **agent loop**, the **model**, the
**tools**, and the **environment** tools run in. Run Brain as a server your app talks to over HTTP,
or embed the `brain` crate in a Rust service you already own.

> [!NOTE]
> **Brain is under early development.** Contracts are replaced in place until the first stable
> release, and there is no upgrade path from earlier builds. APIs, package names, and wire formats
> will change without notice.

## Benchmarks

Same machine, same scripted model behind every runtime — no model latency in any number.
Figures marked ★ are the project's own published numbers.[^bench]

**One turn** (median, lower is better)

```text
Brain             █                                       25 ms
ZeroClaw          ██                                      51 ms
LangGraph Server  █████████████████████████████         1000 ms
OpenClaw          ████████████████████████████████████  1257 ms
```

**Throughput** (complete turns per second, higher is better)

```text
Brain     ██████████████████████████████  227 turns/s
OpenFang  ███                              25 turns/s
```

**New session, ready to take a message** (median, lower is better)

```text
Brain             █████                           0.6 ms
LangGraph Server  ██████                          0.7 ms
ZeroClaw          ███████████████                 1.9 ms
OpenClaw          ██████████████████████████████  3.7 ms
```

**Cold start: launch the process, take a session** (lower is better)

```text
Brain     ████                             25 ms
OpenFang  ██████████████████████████████  180 ms ★
```

- Disk is flat: a 100-turn conversation leaves **0.2 MiB** — the hundredth turn writes the same
  2.3 KiB as the first.
- An idle session costs ~**14 KiB**. The trade: **220 MiB** at rest and a **~20 MB** install,
  mostly the compiled agent loop held warm inside its sandbox.
- A turn slows by ~54 µs per turn of history, because the loop is handed the whole context.
- Framework libraries (LangGraph, CrewAI, AutoGen, Microsoft Agent Framework) and hosted sandboxes
  are measured under different conditions and not ranked against whole servers.
- CI enforces bounds on every push: under 256 MiB resident after 10,000 requests, and a journal
  held to a small constant multiple of its final context. Pre-reset figures are archived in
  [BENCHMARKS.md](BENCHMARKS.md).

[^bench]: AWS `c7g.xlarge`, Linux, one subject at a time, pinned away from the load generator.
    Medians over 600 samples for latency, 100 turns for disk, 16 concurrent sessions for
    throughput, 512 sessions for the memory probe. Each subject is driven through its own HTTP API
    against the same scripted model; ★ figures come from the project's own documentation and are
    not reproduced by us. The harness is in [`tools/bench`](tools/bench).

## How it works

```text
your app <--HTTP/SSE--> [ brain kernel ] --pinned model call--> model API
                           |   |   |
                           |   |   +-- tool call (HTTP) --> environment (sandbox, browser, your backend)
                           |   +-- observation -> decision --> agent loop (Wasm, sealed)
                           +-- append, behind the turn --> segment log
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

## Quick start

Run a server:

```sh
docker run --rm -p 8080:8080 -v brain-data:/var/lib/brain ghcr.io/aexhq/brain:latest
```

Drive a session from TypeScript:

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

Leave out `tools` and the model sees none. Brain listens on loopback and needs no token there; set
`BRAIN_API_TOKEN` to listen anywhere else.

Four runnable scripts — a basic session, event history, the full lifecycle, and the same thing over
raw HTTP with no SDK — are in [`examples/`](examples). Building from source and embedding the crate
in Rust are covered in the [Quickstart](https://aex.dev/brain/docs/quickstart) and the
[embedding guide](https://aex.dev/brain/docs/guides/embed). Setup, the verification commands CI
runs, and how contracts change are in [CONTRIBUTING.md](CONTRIBUTING.md) — issues and pull requests
are welcome.

## Roadmap

- [x] The four-part kernel: agent loop, model, tools, environment
- [x] Unified `brain`, `tool`, and `environment` authoring with `brain build`
- [x] Append-only segment log with restart recovery
- [x] Content identity as a type rather than a digest string
- [x] HTTP/SSE session API and the `@aexhq/brain` SDK
- [x] Remote environment contract with `env-app` and `env-aws-microvm`
- [ ] End-to-end benchmark rebuild and the cross-session isolation test
- [ ] Freeze a v1 API with tagged releases
- [ ] MCP client
- [ ] Subagents
- [ ] File access and workspace sync
- [ ] `web_search` and `web_fetch`
- [ ] crates.io publication
- [ ] Sessions spread across machines sharing environments
- [ ] `checkpoint` and `restore`
- [ ] Custom images with scoped credentials and network metering
- [ ] Hosted Brain at [aex.dev](https://aex.dev/brain)

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
