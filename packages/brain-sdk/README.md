# `@aexhq/brain`

The typed client and extension composition contract for a Brain server.

```ts
import { Brain, brainEnv, hostEnv, tool } from "@aexhq/brain";
import { pi } from "@aexhq/agentloop-pi";
import { z } from "zod";

const lookup = tool({
  name: "lookup",
  description: "Look up a value.",
  input: z.object({ id: z.string() }),
  run: async ({ id }, ctx) => {
    await ctx.emit("lookup_started", { id });
    return { id, value: "found" };
  },
});

const brain = new Brain({ baseUrl: "http://127.0.0.1:8080" });
const session = await brain.sessions.create({
  model: {
    provider: "vercel-ai-gateway",
    name: "openai/gpt-5-mini",
    apiKey: process.env.VERCEL_AI_GATEWAY_API_KEY!,
  },
  agentloop: pi({ env: brainEnv({ name: "brain" }) }),
  tools: [lookup({ env: hostEnv({ name: "app" }) })],
});

await session.send("Look up item 42.");
for await (const event of session.events()) console.log(event);
```

Every Tool and Agentloop is placed in a named Environment with `{ env, ...options }`. Three kinds
exist: `brainEnv({ name })` is Brain's own, hosted in the server; `hostEnv({ name })` is this
process, registered with Brain as a host; and `environment({ url, credential, configure })` is an
extension reached over HTTP, configured per instance by the application.

`component(urlOrBytes)` wraps an already-built WebAssembly Component. Brain admits those raw bytes;
it does not compile application source. Use it to declare a custom Agentloop or a Tool the brain
env runs:

```ts
import { agentloop, brainEnv, component, tool } from "@aexhq/brain";
import { z } from "zod";

const loop = agentloop({
  implementation: component(new URL("./loop.wasm", import.meta.url)),
});

const inspect = tool({
  name: "inspect",
  description: "Inspect the workspace.",
  input: z.object({ path: z.string() }),
  implementation: component(new URL("./inspect.wasm", import.meta.url)),
  needs: ["file:///workspace"],
});

const env = brainEnv({ name: "brain" });
const placedLoop = loop({ env });
const placedTool = inspect({ env });
```

`needs` is a list of URIs the Environment receives and Brain never reads: `pkg:` for software,
`https:` or `wss:` for a network destination, `file:` for a filesystem location. The brain env
grants each invocation exactly what its needs name, bounded by the server's `BRAIN_ENV_*`
allow-lists; the deployment above must include `workspace` in `BRAIN_ENV_FILESYSTEM_ALLOW`.

A Tool with `run` is a function this process holds and is placed in `hostEnv`. The SDK registers
this process as a host over SSE, receives commands, validates inputs and outputs, and posts one
terminal outcome. `ctx.emit(kind, data)` appends an extension event to the session's canonical
journal before its promise resolves. Save `await brain.credentials()` and pass it back as
`credentials` to resume the host after a restart.

The SDK admits Component bytes by content, preserves explicit placement, and supplies deterministic
idempotency keys for admission. A caller may supply an `idempotencyKey` for other mutating
requests. Repeating session creation with the same key keeps the existing host handlers, including
any active calls and their cancellation signals.

Requests have no implicit client-side deadline because a turn may legitimately outlive a short
HTTP timeout. Set `timeoutMs` on `Brain` when the caller owns a tighter bound; Brain still enforces
its configured model, Tool, and whole-turn limits.

## Preparation and suspended history

Prepare an Agentloop with `await brain.admit(placedLoop)` or
`await brain.admitAgentloop(loopComponent)`, and a Tool with `await brain.admitTool(toolComponent)`.
Use the same Component objects when placing them for creation. Successful admission is cached.
`await session.transcript()` returns canonical messages and their journal sequence without starting
execution; Events and live subscriptions also remain accessible while suspended.

Brain releases session execution at turn end by default. Explicit session `idleTtlMs: 0` retains it.
Environment resource TTL belongs to the Environment. Setup is logical, with allocation deferred to
the first invoke if the Environment prefers. Placements are fixed for the life of a session.

Agentloop authors can import generated `ModelRequest`, `ModelResult`, `ToolResult`, `EventPage`, and
`SessionTranscript` types. The JSON schemas ship at `@aexhq/brain/contracts/session.json`; WIT ships
at `@aexhq/brain/contracts/agentloop.wit` and `@aexhq/brain/contracts/tool.wit`. `events(after)` reads
history during an activation, and `emit` appends extension Events. Model, Tool, and Environment
failures are never retried by Brain.
