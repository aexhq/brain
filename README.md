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

**Brain** is a minimal, *blazingly fast*, extensible agent runtime server. You write the
agent loop and the tools, and Brain runs the session. Tools run anywhere, from a browser tab
to a server sandbox. Agent loops run in a Wasm sandbox, so the runtime is secure by design.
Each session uses very little memory, and every step is an event you can watch in real time.

> [!NOTE]
> **Early preview.** The API and functionality may change without backward compatibility or
> notice until we cut 1.0.0.

## Features

- **Tools run anywhere.** A tool is a typed function. It can run in your own process, in a
  microVM sandbox, in a browser page, or on your backend, and one session can use several of
  these at once.
- **Low overhead.** Session state stays in memory and the journal is written after the turn.
  An idle session uses about 14 KiB and a round trip takes 40 ms. CI checks these numbers on
  every build.
- **Bring your model, spawn your agents.** Brain has built-in bindings for 70+ LLM providers
  through [models.dev](https://models.dev). The model is pinned per session, and a session can
  create other sessions for subagent work.
- **Isolated agent loops.** An agent loop compiles to WebAssembly and runs in its own
  sandbox. Brain does the I/O on its behalf, so every decision is deterministic and
  replayable.
- **Observable end to end.** Every observation, decision, model call, and tool result is an
  event. You can stream them live or read them back later.

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

<sub>Each number is the median from the harness in <a href="tools/bench">tools/bench</a>, run on
the same AWS <code>c7g.xlarge</code> for every subject. Bars use a log scale. The chart includes
only agent runtimes that own sessions behind an API. <a href="BENCHMARKS.md">BENCHMARKS.md</a> has
the method and the subject versions.</sub>

## Extensions

Brain owns the session. You write the rest as extensions with the same SDK. Export a
definition, run `npx brain build`, and pass the generated factory to a session. There are
three kinds of extension:

- **Agent loops** decide what happens next. A loop is a set of synchronous hooks compiled to
  a Wasm component, and Brain does the I/O it asks for.
- **Tools** do the work. A tool is a program, a bundled function, a shell script, or an HTTP
  request, that declares which resources it operates on.
- **Environments** run programs. An environment can be a local process, a browser page, or a
  sandbox service, and it declares the resources a program finds there.

### Agent loops

An agent loop registers one handler per observation (`start`, `message`, `model`, `tools`,
`event`, `cancel`). Each handler returns one action (`turn.model`, `turn.tools`, `turn.emit`,
`turn.reply`, `turn.done`, `turn.fail`). Loop state is declared with a schema and can be
replayed from the journal. The loop below runs a full turn. When the model asks for several
tools at once, it hands the whole batch to Brain, which runs them in parallel and returns all
the results in one observation. See the
[guide](https://aex.dev/brain/docs/guides/write-a-loop) for details.

```ts
import { agentloop } from "@aexhq/brain";
import { z } from "zod";

const state = z.object({ messages: z.array(z.unknown()) });
const text = (m) => m.content.filter((b) => b.type === "text").map((b) => b.text).join("");

export const coder = agentloop({}, (author) => {
  const memory = author.state(state, () => ({ messages: [] }));
  const push = (role, content) => memory.messages.push({ role, content });
  // The loop decides what the model is told: the prompt, and which of the session's tools to offer.
  const ask = (turn) => turn.model({ system: "You are a coding agent.", tools: author.tools.map((t) => t.name), messages: memory.messages });

  author.on.message(({ input }, turn) => {
    push("user", [{ type: "text", text: input.message }]);
    return ask(turn);
  });

  author.on.model(({ response }, turn) => {
    push("assistant", response.message.content);
    const calls = response.message.content.filter((b) => b.type === "tool_use").map((b) => ({ callId: b.id, name: b.name, input: b.input }));
    // One batch: Brain runs every call in parallel and reports back once.
    return calls.length ? turn.tools(calls) : turn.reply(text(response.message));
  });

  author.on.tools(({ results }, turn) => {
    push("user", results.map((r) => ({ type: "tool_result", tool_use_id: r.call_id, content: r.output, is_error: r.is_error })));
    return ask(turn);
  });
});
```

### Tools

A tool declares its input and output schemas and the resources it operates on (`fs`,
`process`, `net`, `dom`, `secrets`). Inside, it is plain code on the platform it runs on:
`node:fs`, `child_process`, `fetch`. At run time it receives its binding values, a
`deadline`, a `signal`, and a `progress` callback. If the environment does not declare what
the tool needs, Brain rejects the session at create time. See the
[guide](https://aex.dev/brain/docs/guides/write-a-tool) for the build output.

```ts
import { execFile } from "node:child_process";
import { readFile } from "node:fs/promises";
import { promisify } from "node:util";
import { tool } from "@aexhq/brain";
import { z } from "zod";

export const test = tool(
  {
    description: "Run the test suite and return the report.",
    input: z.object({ filter: z.string().optional() }),
    output: z.object({ passed: z.boolean(), report: z.string() }),
    needs: ["process", "fs"],
  },
  (author) => {
    author.run(async ({ filter }, context) => {
      context.progress({ stage: "running" });
      const passed = await promisify(execFile)("npm", ["test", "--", filter ?? ""], { signal: context.signal }).then(() => true, () => false);
      return { passed, report: await readFile("reports/junit.xml", "utf8") };
    });
  },
);
```

A tool that is one command needs no code at all:

```ts
export const bash = tool.shell({
  description: "Run a shell command in the workspace.",
  input: z.object({ command: z.string() }),
  needs: ["process"],
  script: "$command",
});
```

A tool with an `execute` function runs in your own process instead. There it can use the SDK
directly, for example to create a child session for a subtask and read the child's events
while it runs:

```ts
const delegate = tool({
  name: "delegate",
  description: "Hand a subtask to a fresh agent and return its answer.",
  input: z.object({ task: z.string() }),
  execute: async ({ task }) => {
    const child = await brain.sessions.create({ model, agentloop: coder(), tools: [test({ env: box })] });
    await child.send(task);
    let answer;
    for await (const e of child.events()) if (e.type === "output_emitted" && e.data.type === "assistant_message") { answer = e.data.message; break; }
    await child.end();
    return { answer };
  },
});
```

### Environments

An environment opens an instance, declares the resources a program finds there, and
registers an executor for each program kind it can launch. The methods it returns become
typed calls on the application object, and Brain journals each call. The example below wraps
a sandbox service. Replace the vendor SDK with your own. See the
[guide](https://aex.dev/brain/docs/guides/write-an-environment) for the details.

```ts
import { environment } from "@aexhq/brain";
import { Sandbox } from "@vendor/sandbox";
import { z } from "zod";

export const sandbox = environment(
  {
    options: z.object({ image: z.string() }),
    resources: { fs: { root: "/workspace" }, process: {}, net: { allow: ["registry.npmjs.org"] } },
  },
  (author) => {
    const box = author.open(({ options, id }) => Sandbox.create({ name: id, image: options.image }));
    box.execute.esm({ artifacts: builtTools });
    box.execute.shell(({ instance, signal }, script) => instance.run(script, { cwd: "/workspace", signal }));
    box.close(({ instance }) => instance.kill());
    return { snapshot: box.method((_input, { instance }) => instance.snapshot()) };
  },
);
```

Put all three into one session:

```ts
const box = sandbox({ image: "node:22" });
const session = await brain.sessions.create({ model, agentloop: coder(), tools: [test({ env: box }), bash({ env: box }), delegate] });
```

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
                    | agent loop, a Wasm component  |   decides
                    +---------------+---------------+
                                    | decision
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
- **Bounded live streaming.** A model delta reaches subscribers as soon as the provider emits
  it. Each subscriber has a fixed ring of 1,024 events, and a reader that falls behind resumes
  from the log at the record it last saw. The cost per subscriber stays constant.
- **Events are the data model.** Every observation, decision, model call, token, and tool
  result is an event in one feed. Watching live and reading history use the same records, so
  tracing a session is the same as replaying it.

Brain is one native Rust binary on [Tokio](https://tokio.rs/). It serves the session API over
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
process.exit(0);
```

The model reads the question and calls `lookup_order`. The call arrives as a typed record
on the session's event feed, your function answers it using the `orders` object it closes
over, and the SDK posts the result back. The journal keeps both the call and the result. If
a tool has to run somewhere else, such as a browser page, a sandbox, or another machine, it
declares a hosting environment instead and the session API stays the same. See the
[app tools guide](https://aex.dev/brain/docs/guides/app-tools).

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

## Contact

For support and bug reports, open an [issue](https://github.com/aexhq/brain/issues) or write
to [support@aex.dev](mailto:support@aex.dev). For collaboration and partnerships, write to
[admin@aex.dev](mailto:admin@aex.dev).
