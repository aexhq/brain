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

<sub>Medians measured by the harness in <a href="tools/bench">tools/bench</a> on an AWS
<code>c7g.xlarge</code> with the same instant scripted model behind every subject; the LangGraph
figures measure LangGraph Server, and Brain's first-token figure is an upper bound (its whole-turn
median). Memory bars are log-scaled; Brain's is the marginal cost per additional idle session.
<strong>°</strong> the project's own published figure, not measured by our harness — Cloudflare
Agents' is a V8 isolate spawn on their cloud, Vertex AI Agent Engine's is its documented cold
start, and Agno's are in-process agent instantiation with no server round trip.
<strong>†</strong> personal/local assistant runtimes — a different deployment model than a server
runtime, kept for reference. Runtimes that publish no comparable figures (Letta, Mastra, Golem,
Awaken, Restate, Temporal, AgentScope, VoltAgent, Julep, …) are absent until we measure them
ourselves. Methodology and the bounds CI enforces on every push are in
<a href="BENCHMARKS.md">BENCHMARKS.md</a>.</sub>

## How it works

```text
your app ──── send (HTTP) ────►  [ brain runtime ]  ──── event feed (SSE), token by token ────► your app

┌───────────────────────────────────── inside one turn ────────────────────────────────────┐
│                                                                                          │
│  observation ──► agent loop ──► decision      a Wasm component, sealed: no network, no   │
│                                               filesystem, no secrets, no clock — Brain   │
│                                               performs every effect on its behalf, so    │
│                                               any decision replays exactly               │
│                                                                                          │
│  decision ──┬──► model call ──► provider      pinned per session; deltas stream          │
│             │                                 straight through to the feed               │
│             │                                                                            │
│             └──► tool call ──► environment    plain HTTP: a microVM sandbox, a browser   │
│                                               tab, your own backend — one session can    │
│                                               span several at once                       │
│                                                                                          │
│  every step ──► append-only journal           the write-ahead record, appended behind    │
│                                               the turn; a restart rebuilds every         │
│                                               session from it                            │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

Brain owns the session; the agent loop, model, tools, and environment are yours to supply. The
speed comes from a handful of techniques most runtimes don't use:

- **Sealed WebAssembly agent loops** — a loop compiles from any language to a component on
  [Wasmtime](https://wasmtime.dev/), compiled once and activated per decision at native speed.
  It gets no ambient capabilities — Brain performs every effect for it — which is what makes a
  decision deterministic and replayable from its position in the journal.
- **A write-ahead journal instead of a database** — the only durable state is an append-only
  segment log, written behind the turn, off the hot path. Sessions are memory-resident and
  rebuilt from the journal after a restart; a session interrupted mid-turn comes back with a
  `turn_interrupted` event and lets the client decide.
- **Streaming with a bound, not a buffer** — a model delta reaches subscribers the moment the
  provider emits it; nothing is accumulated per turn. The live feed rides a fixed 1,024-event
  ring per subscriber, so a reader that falls behind drops (and is told how many it missed)
  rather than slowing the turn — the journal is the record it re-reads.
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
  [models.dev](https://models.dev), sealed per session, and sessions that create sessions for
  subagent work.
- **Sealed extension execution** — agent loops compile to WebAssembly and run in a standalone
  runtime with no network, filesystem, secrets, or clock; Brain performs every effect.
- **Observable end to end** — every observation, decision, model intent, and tool result is an
  event you can stream live or read back later, token by token while the turn runs.

## Quick start

Give the agent a voice: its only tool lives in a browser tab, and it answers out loud through
your speakers.

Run a server (host networking, so Brain can dial the environment on loopback — on Docker
Desktop enable host networking in settings, or run the binary from the
[Quickstart](https://aex.dev/brain/docs/quickstart)):

```sh
docker run --rm --network host \
  -e BRAIN_LISTEN=127.0.0.1:8080 \
  -e BRAIN_ENVIRONMENT_BASE_URL=http://127.0.0.1:8787 \
  -v brain-data:/var/lib/brain ghcr.io/aexhq/brain:latest
```

```sh
npm install @aexhq/brain @aexhq/agentloop-pi @aexhq/env-app zod
```

Save as `talk.mjs` and run with `node talk.mjs`:

```js
import { createServer } from "node:http";
import { Brain, appTool, createEnvironmentHandler } from "@aexhq/brain";
import { app } from "@aexhq/env-app";
import { pi } from "@aexhq/agentloop-pi";
import { z } from "zod";

// The page: it holds an outbound WebSocket to the environment and answers
// the `say` tool from inside the tab — with your speakers.
const page = `<!doctype html><title>brain, out loud</title><body>🔊 This tab is a Brain environment.
<script type="module">
import { appTools } from "https://esm.sh/@aexhq/brain";
import { z } from "https://esm.sh/zod@4";
appTools.connect({ url: "ws://127.0.0.1:8787/environments/env_1/channel", token: "quickstart" })
  .register(
    { name: "say", description: "Speak out loud through the user's speakers.", input: z.object({ text: z.string() }) },
    ({ text }) => { speechSynthesis.speak(new SpeechSynthesisUtterance(text)); return "spoken"; },
  );
</script>`;

// Host the environment beside the page: Brain POSTs operations here, the tab holds the channel.
const handle = createEnvironmentHandler(app);
const server = createServer(async (request, response) => {
  if (request.method === "POST") {
    let body = "";
    for await (const chunk of request) body += chunk;
    response.setHeader("content-type", "application/json");
    return response.end(JSON.stringify(await handle(JSON.parse(body))));
  }
  response.setHeader("content-type", "text/html; charset=utf-8");
  response.end(page);
});
server.on("upgrade", (request, socket, head) => handle.channel.upgrade(request, socket, head));
server.listen(8787, "127.0.0.1");

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080" });
const channel = app({ channelToken: "quickstart" });
const session = await brain.sessions.create({
  model: { provider: "openai", name: "gpt-5-mini", apiKey: process.env.OPENAI_API_KEY },
  agentloop: pi(),
  tools: [
    appTool({
      name: "say",
      description: "Speak out loud through the user's speakers.",
      input: z.object({ text: z.string() }),
    }).useIn(channel),
  ],
  system: "You can speak out loud. Answer by saying it.",
});

console.log("Open http://127.0.0.1:8787 in a browser, sound on, then press Enter.");
await new Promise((resolve) => process.stdin.once("data", resolve));

await session.send("Introduce yourself out loud, in one sentence.");
for await (const event of session.events()) console.log(event.sequence, event.type);

await session.end();
await session.delete();
process.exit(0);
```

That's a session whose only tool runs in a browser tab: the Node script hosts the environment and
serves the page, the tab connects out over a WebSocket and registers `say`, Brain routes the
model's tool call down the channel — and the answer comes out of your speakers while the event
feed streams in the terminal.

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

## Contact

Questions, ideas, or something broken? Open an [issue](https://github.com/aexhq/brain/issues) or
write to [admin@aex.dev](mailto:admin@aex.dev).
