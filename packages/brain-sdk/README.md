# `@aexhq/brain`

The typed client and extension composition contract for a Brain server.

```ts
import { Brain, brainWasm, tool } from "@aexhq/brain";
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

const wasm = brainWasm();
const brain = new Brain({ baseUrl: "http://127.0.0.1:8080" });
const session = await brain.sessions.create({
  model: {
    provider: "vercel-ai-gateway",
    name: "openai/gpt-5-mini",
    apiKey: process.env.VERCEL_AI_GATEWAY_API_KEY!,
  },
  agentloop: pi({ env: wasm }),
  tools: [lookup()],
});

await session.send("Look up item 42.");
for await (const event of session.events()) console.log(event);
```

`component(urlOrBytes)` wraps an already-built WebAssembly Component. Brain admits those raw bytes;
it does not compile application source. Use it to declare a custom Agentloop or placed Tool:

```ts
import { agentloop, component, tool } from "@aexhq/brain";
import { z } from "zod";

const loop = agentloop({
  implementation: component(new URL("./loop.wasm", import.meta.url)),
});

const inspect = tool({
  name: "inspect",
  description: "Inspect the workspace.",
  input: z.object({ path: z.string() }),
  implementation: component(new URL("./inspect.wasm", import.meta.url)),
  needs: ["fs"],
});

const env = brainWasm({ filesystem: { workspace: true } });
const boundLoop = loop({ env });
const boundTool = inspect({ env });
```

The Brain deployment must also include `workspace` in `BRAIN_WASM_FILESYSTEM_ALLOW`.

A Tool with `run` is resident in the application process and is instantiated with `tool({...})()`.
The SDK opens one registered host over SSE, receives commands, validates inputs and outputs, and
posts one terminal outcome. `ctx.emit(kind, data)` appends an extension event to the session's
canonical journal before its promise resolves.

A Tool with `implementation` is placed and requires `{ env, ...options }`. Agentloops are always
placed the same way. The SDK admits Component bytes by content identity, preserves explicit
placement, and supplies deterministic idempotency keys for admission. A caller may supply an
`idempotencyKey` for other mutating requests.

Requests have no implicit client-side deadline because a turn may legitimately outlive a short
HTTP timeout. Set `timeoutMs` on `Brain` when the caller owns a tighter bound; Brain still enforces
its configured model, Tool, and whole-turn limits.

## Preparation and suspended history

Call `await brain.admit(loop)` and `await brain.admit(tool)` before creation to prepare the same
Component objects you will bind. Successful admission is cached. `await session.transcript()`
returns canonical messages and their journal sequence without starting execution; Events and live
subscriptions also remain accessible while suspended.

Brain releases session execution at turn end by default. Explicit session `idleTtlMs: 0` retains it.
Environment resource TTL belongs in the Environment provider's configuration. Setup/attach can be
logical operations with allocation deferred to invocation. Bindings remain fixed throughout MVP.

Agentloop authors can import generated `ModelRequest`, `ModelResult`, `ToolResult`, `EventPage`, and
`SessionTranscript` types. The JSON schemas ship at `@aexhq/brain/contracts/session.json`; WIT ships
at `@aexhq/brain/contracts/agentloop.wit`. `events(after)` reads history during an activation, and
`emit` appends extension Events. Model, Tool, and Environment failures are never retried by Brain.
