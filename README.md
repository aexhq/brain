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
  <a href="https://github.com/aexhq/extensions">Official extensions</a>
</p>

## What is it

**Brain** is a minimal, *blazingly fast* agent runtime. Build your own AI-native apps, with
tools that run anywhere from a client browser to a server sandbox. Run any agentloop, from pi
to codex. Deploy flexibly as a Docker image or an embedded Rust crate. Secure by design, with
Wasm-isolated agentloop and tool execution. Scale easily with minimal memory overhead. Instant
observability with real-time events.

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
ZeroClaw   ██                                    51 ms †
LangGraph  ██████████████████████████████      1049 ms
OpenClaw   ████████████████████████████████████ 1257 ms †
```

**Time to first token**

```text
ZeroClaw   █                                    9.6 ms †
Brain      ██                                    25 ms ★
LangGraph  ████                                48.6 ms
OpenFang   ██████                              75.8 ms
OpenClaw   ██████████████████████████████     874.4 ms †
```

**New session**

```text
Agno       █                                     3 µs °
Brain      ███                                 0.6 ms ★
LangGraph  ████                                0.7 ms
ZeroClaw   ███████                             1.9 ms †
OpenClaw   ████████████                        3.7 ms †
OpenFang   ██████████████████████████████     10.3 ms
```

**Cold start**

```text
Cloudflare  █                                   <5 ms °
ZeroClaw    ██                                  10 ms †
Brain       ███                                 25 ms ★
OpenFang    ██████                             180 ms
LangGraph   ████████████████                   2.5 s
Vertex AI   ███████████████████████            4.7 s °
OpenClaw    ██████████████████████████████     5.98 s †
```

**Memory per idle session**

```text
Agno       █                                  6.5 KiB °
Brain      ██                                  14 KiB ★
OpenFang   █████████                          0.6 MiB
ZeroClaw   █████████████████████               50 MiB †
OpenClaw   ██████████████████████████████     490 MiB †
```

<sub>Medians from the harness in <a href="tools/bench">tools/bench</a> on an AWS
<code>c7g.xlarge</code>. Brain's first-token figure is an upper bound (its whole-turn median);
memory bars are log-scaled. <strong>°</strong> a project's own published figure.
<strong>†</strong> personal/local assistant runtimes. Methodology and the bounds CI enforces:
<a href="BENCHMARKS.md">BENCHMARKS.md</a>.</sub>

## How it works

```text
your app ──── send (HTTP) ────►  [ brain runtime ]  ──── event feed (SSE) ────► your app

┌───────────────────────────────────── inside one turn ────────────────────────────────────┐
│                                                                                          │
│  observation ──► agent loop ──► decision      a secured, isolated Wasm component,        │
│                                               with limited resources; every decision     │
│                                               replays exactly from the log               │
│                                                                                          │
│  decision ──┬──► model call ──► provider      pinned per session; deltas streamed        │
│             │                                 to the event feed as they arrive           │
│             │                                                                            │
│             └──► tool call ──► environment    environments are where tools are           │
│                                               executed, at your choice: browser,         │
│                                               microVM sandbox, lambda, etc.              │
│                                                                                          │
│  every step ──► write-ahead log (WAL)         appended behind the turn; a restart        │
│                                               rebuilds every session from it             │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

Brain owns the session; the agent loop, model, tools, and environment are yours to supply. The
speed comes from a handful of techniques most runtimes don't use:

- **Isolated WebAssembly agent loops** — a loop compiles from any language to a component on
  [Wasmtime](https://wasmtime.dev/): secured, isolated, resource-limited, compiled once and
  activated per decision at native speed. Brain performs every effect on the loop's behalf,
  which makes each decision deterministic and replayable from its position in the log.
- **Write-ahead log (WAL) persistence** — the runtime's only durable state is an append-only
  write-ahead log, written behind the turn, off the hot path. Sessions are memory-resident and
  rebuild from the WAL after a restart; a session interrupted mid-turn comes back with a
  `turn_interrupted` event and lets the client decide.
- **Bounded live streaming** — a model delta reaches subscribers the moment the provider emits
  it. The live feed rides a fixed 1,024-event ring per subscriber; a reader that falls behind
  drops, learns how many records it missed, and re-reads them from the WAL, so a slow consumer
  can never slow a turn.
- **Observability as the data model** — every observation, decision, model intent, token, and
  tool outcome is an event in one feed; watching live and reading history back are the same
  records, so tracing a session is replaying it.

The rest is deliberately boring: one native Rust binary on [Tokio](https://tokio.rs/), serving
the session API over [Axum](https://github.com/tokio-rs/axum) HTTP/SSE, with no external stores.

## Features

- **Tools run anywhere** — a tool is plain HTTP in an environment you choose: a microVM sandbox,
  a browser driving the DOM, your own backend — and one session can span several at once.
- **Built for low overhead** — memory-resident session state, appends behind the turn, ~14 KiB
  per idle session, 25 ms round trips. The numbers above are measured, and CI holds them.
- **Bring your model, spawn your agents** — built-in bindings for 70+ LLM providers via
  [models.dev](https://models.dev), pinned per session, and sessions that create sessions for
  subagent work.
- **Isolated extension execution** — agent loops compile to WebAssembly and run in a
  standalone, resource-limited runtime with no network, filesystem, secrets, or clock; Brain
  performs every effect.
- **Observable end to end** — every observation, decision, model intent, and tool result is an
  event you can stream live or read back later while the turn runs.

## Quick start

The session's tool is a plain function in your own process: the model decides to call it, Brain
dials the environment, and the environment routes the call back to your code.

Run a server (host networking, so Brain can dial the environment on loopback — on Docker
Desktop enable host networking in settings, or run the binary from the
[Quickstart](https://aex.dev/brain/docs/quickstart)):

```sh
docker run --rm --network host   -e BRAIN_LISTEN=127.0.0.1:8080   -e BRAIN_ENVIRONMENT_BASE_URL=http://127.0.0.1:8787   -v brain-data:/var/lib/brain ghcr.io/aexhq/brain:latest
```

```sh
npm install @aexhq/brain @aexhq/agentloop-pi @aexhq/env-app zod
```

Save as `order.mjs` and run with `node order.mjs`:

```js
import { createServer } from "node:http";
import { Brain, appTool, appTools, createEnvironmentHandler } from "@aexhq/brain";
import { app } from "@aexhq/env-app";
import { pi } from "@aexhq/agentloop-pi";
import { z } from "zod";

// The tool: one contract, one plain function, closing over your app's state.
const orders = { "A-1001": { status: "shipped", eta: "Thursday" } };
const lookupOrder = {
  name: "lookup_order",
  description: "Look up an order's status by id.",
  input: z.object({ id: z.string() }),
};

// Host the environment: Brain POSTs operations here, and the tool answers in-process.
const handle = createEnvironmentHandler(app);
const server = createServer(async (request, response) => {
  let body = "";
  for await (const chunk of request) body += chunk;
  response.setHeader("content-type", "application/json");
  response.end(JSON.stringify(await handle(JSON.parse(body))));
});
server.on("upgrade", (request, socket, head) => handle.channel.upgrade(request, socket, head));
server.listen(8787, "127.0.0.1");

appTools
  .connect({ url: "ws://127.0.0.1:8787/environments/env_1/channel", token: "quickstart" })
  .register(lookupOrder, ({ id }) => orders[id] ?? { status: "unknown order" });

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080" });
const session = await brain.sessions.create({
  model: { provider: "openai", name: "gpt-5-mini", apiKey: process.env.OPENAI_API_KEY },
  agentloop: pi(),
  tools: [appTool(lookupOrder).useIn(app({ channelToken: "quickstart" }))],
});

await session.send("Where is order A-1001?");
for await (const event of session.events()) console.log(event.sequence, event.type);

await session.end();
await session.delete();
process.exit(0);
```

The model reads the question, calls `lookup_order`, and your function answers from the `orders`
object beside it — the whole exchange lands in the event feed, tool call and result included.
The same channel works from a browser page or anything behind NAT, and provisioned tools ship
their code into a sandbox environment the same way.

## Roadmap

- [x] The four-part runtime: agent loop, model, tools, environment
- [x] Unified `brain`, `tool`, and `environment` authoring with `brain build`
- [x] Append-only segment log with restart recovery
- [x] Typed content identity
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

## Contact

Support and bug reports: open an [issue](https://github.com/aexhq/brain/issues) or write to
[support@aex.dev](mailto:support@aex.dev). Collaboration and partnerships:
[admin@aex.dev](mailto:admin@aex.dev).
