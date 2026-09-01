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

**Brain** is a minimal, *blazingly fast*, extensible agent runtime server. Deploy and build
your own AI-native apps, write customized agentloop, tools that run in any environment from
client browser to server sandbox. Secure by design, with Wasm-isolated agentloop and
execution. Scale easily with minimal memory overhead. Instant observability with real-time
events.

## Benchmarks

No model latency in any number. ★ marks Brain in each chart.

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

<sub>Medians from the harness in <a href="tools/bench">tools/bench</a>, every subject measured on
the same AWS <code>c7g.xlarge</code>; bars are log-scaled. Brain's first-token figure is now a real
measurement: the scripted provider delays its first token deliberately and the probe
subtracts the delay, so what remains is the runtime's own overhead. Cold start — launch on a
fresh data directory to the first turn served — measures 0.66 s for Brain (n=8); competitor
rows land with the next measuring session. The charts compare agent runtimes —
servers that own sessions and run an agent loop behind an API; agent libraries and generic
durable-execution engines are measured by the harness but not charted, because a turn without
persistence or an agent surface is a different product. Subject versions, methodology, and
the bounds CI enforces:
<a href="BENCHMARKS.md">BENCHMARKS.md</a>.</sub>

## How it works

One turn, end to end — every effect preceded by its journaled intent, the loop sandboxed in
its own process, output streaming while the turn runs, and a client-hosted tool answered by
your own code off the event feed:

```mermaid
sequenceDiagram
    autonumber
    participant App as your app
    participant Brain as brain server
    participant Journal as journal
    participant Loop as agent loop (Wasm worker)
    participant Model as model provider

    App->>Brain: POST /messages
    Brain->>Journal: turn_started
    Brain->>Loop: activate(user_message)
    Loop-->>Brain: decision: model
    Brain->>Journal: model_intent
    Brain->>Model: completion (streaming)
    Brain--)App: assistant deltas on the SSE feed, mid-turn
    Model-->>Brain: completion
    Brain->>Journal: model_result
    Brain->>Loop: activate(model_completed)
    Loop-->>Brain: decision: tools
    Brain->>Journal: tool_intent — the turn parks
    Brain--)App: the call surfaces on the event feed
    App->>App: your execute() runs beside your state
    App->>Brain: POST /tool-results/{operation_id}
    Brain->>Journal: tool_result
    Brain->>Loop: activate(tools_completed)
    Loop-->>Brain: decision: finish
    Brain->>Journal: turn_finished + context, once per turn
    Brain-->>App: updated session
```

And the part most runtimes cannot draw — what happens when the process dies:

```mermaid
sequenceDiagram
    participant App as your app
    participant Brain as brain server (new process)
    participant Journal as journal

    Note over Brain: previous process died mid-turn
    Brain->>Journal: read every session's row at boot
    Brain->>Journal: turn_interrupted, for anything left running
    App->>Brain: GET /events?after=n
    Journal-->>App: exactly the records missed
    App->>Brain: POST /messages
    Brain-->>App: the same conversation continues
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

The session's tool is a plain function in your own process: declare it once, hand it to
the session, and the SDK answers the model's calls off the session's own event feed. No
server in your app, no ports, no channel.

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
process.exit(0);
```

The model reads the question and calls `lookup_order`: the call arrives as a typed
record on the session's event feed, your function answers it beside the `orders` object
it closes over, and the SDK posts the result back — durable in the journal, tool call
and result included. A tool that must live somewhere else — a browser page, a sandbox,
another machine — declares a hosting environment instead, and the session API does not
change: see the [app tools guide](https://aex.dev/brain/docs/guides/app-tools).

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
