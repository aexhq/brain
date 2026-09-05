<pre align="center">
              ______ ______ _______ _______ _______
  ▄████▄     |   __ \   __ \   _   |_     _|    |  |
▄██▄██▄██▄   |   __ <      <       |_|   |_|       |
  ▀▀  ▀▀     |______/___|__|___|___|_______|__|____|
</pre>

<p align="center"><strong>A minimal, distributed and extensible agent runtime</strong></p>

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
  <a href="ROADMAP.md">Roadmap</a> ·
  <a href="README.cn.md">中文</a>
</p>

> [!NOTE]
> **Early preview.** The API and functionality may change without backward compatibility or
> notice until we cut 1.0.0.

## What is it

**Brain** is a standalone, minimal, distributed and extensible agent runtime. Compose an Agentloop,
Models, Tools, and Environments through small public interfaces. One session can invoke Tools in several execution
Environments while keeping its transcript and canonical history locally accessible.

It is for builders who need control over agent execution and want to assemble their own system:
custom assistants, research agents, and future agent platforms. Brain supplies runtime mechanisms;
applications supply product policy, scheduling, tenancy, and infrastructure. Aex is an independent
consumer of these same interfaces.

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


## Architecture

One session can use native Tools, functions in your application, and Tools in several remote
Environments. The Agentloop controls context and decides when to call the model or dispatch Tools;
Brain coordinates execution and records the results.

```mermaid
flowchart TB
  subgraph App["Your application"]
    Client["SDK / HTTP client"]
    Resident["Resident Tools"]
  end

  subgraph Brain["Brain runtime"]
    Server["HTTP / SSE server<br/>Session coordination"]
    Journal[("Local journal<br/>Transcript, slots and Events")]
    subgraph Worker["brainWasm Environment · separate Wasmtime worker"]
      Loop["Agentloop Component"]
      Native["Native Tool Components"]
    end
    Server <-->|"commit / read"| Journal
    Server <-->|"activate / host calls"| Loop
    Server <-->|"invoke / result"| Native
  end

  Client <-->|"HTTP / SSE"| Server
  Server <-->|"host SSE / results"| Resident
  Server <-->|"model calls"| Models["Model providers"]
  Server <-->|"Environment protocol"| EnvA["Environment A<br/>Tools + resources"]
  Server <-->|"Environment protocol"| EnvB["Environment B<br/>Tools + resources"]
```

Brain commits each external-effect intent before dispatch and sends it once. The local journal
retains session state; execution is released after each turn by default. Transcripts and recorded
Events remain readable while execution is suspended. Environment providers own resource allocation,
TTL, and cleanup independently of session execution.

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
  projections on demand after a restart. A disposable checkpoint avoids ordinary full-history replay.
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

## Performance

Brain releases session execution after each turn by default and opens history on demand. Admission
retains compiled, prelinked Components; each invocation gets fresh state. Native Tools have capacity
independent of waiting Agentloops. These properties are covered by regression tests.

The earlier comparison numbers describe an older execution/storage design. See [BENCHMARKS.md](BENCHMARKS.md)
for their provenance and [the benchmark guide](docs/reference/benchmarks.mdx) for current measurements
and limits. New-session latency, whole-process memory, and resume cost must be measured together.

## Contact

For support and bug reports, open an [issue](https://github.com/aexhq/brain/issues) or write
to [support@aex.dev](mailto:support@aex.dev). For collaboration and partnerships, write to
[admin@aex.dev](mailto:admin@aex.dev).
