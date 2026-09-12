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
try {
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
} finally {
  await brain.close();
}
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
});

const env = brainEnv({ name: "reader", filesystem: { workspace: "read" } });
const placedLoop = loop({ env });
const placedTool = inspect({ env });
```

Resource grants are Environment options. `brainEnv` accepts `filesystem: { workspace?, scratch? }`
with `"read"` or `"write"` access, `network` as HTTP(S) origins, and `secrets` as server-variable
names. Omitted access stays denied; the server's `BRAIN_ENV_*` allow-lists remain the ceiling.
The example requires `workspace` in `BRAIN_ENV_FILESYSTEM_ALLOW`. Use separate bindings if the
loop and Tool should receive different grants.

There is no universal `needs`. Dependencies belong to packages and Environment-owned preparation,
which completes before imports or execution. Unsupported placement and setup failure are explicit
errors, never a reason for Brain to install packages or choose another Environment.

A Tool with `run` is a function this process holds and is placed in `hostEnv`. The SDK registers
this process as a host over SSE, receives commands, validates inputs and outputs, and posts one
terminal outcome. `ctx.emit(kind, data)` appends an extension event to the session's canonical
journal before its promise resolves. Save `await brain.credentials()` and pass it back as
`credentials` to resume the host after a restart.

The host connection belongs to the client and stays open until `await brain.close()`, including
after failed creation or ending the last session. Put creation inside `try` and close in `finally`.
Close is idempotent, aborts client I/O and local handlers, and rejects subsequent operations.
It leaves stored sessions available. Use `session.interrupt()` to stop the current turn,
`session.end()` to finish a session while keeping history, and `session.delete()` to remove an
ended or failed session. See the [lifecycle example](../../examples/session-lifecycle.mjs).
Clients returned by `withToken` have independent lifetimes.

Tool input schemas use Zod's input semantics: defaulted arguments are optional, and handlers
receive parsed defaults and transforms. Ordinary objects strip extra properties; strict objects
reject them. Output schemas continue to describe parsed output.

Host functions may return ordinary successful output or an `Outcome` directly. The top-level
statuses `ok`, `error`, `timeout`, `cancelled` and `unknown` declare outcomes; malformed envelopes
fail as `invalid_output`. Only successful values pass through the output schema. Structured errors
retain code, message, retryable and details. Use an explicit `ok.value` for business data that uses a
reserved status. See [the tested example](../../examples/tool-outcomes.mjs).

Tool deadlines produce `timeout`, explicit cancellation produces `cancelled`, and known failures
produce `error`. `unknown` means an operation may have been dispatched but its result is unavailable.
All non-success outcomes become failed Tool results. Cancellation and timeout do not promise rollback.

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
The caller controls Environment lifetime through setup, detach, and teardown; providers enforce
physical resource ceilings. Setup may defer allocation to the first execute. Placements are fixed
for the life of a session. A canonical Tool can have one implementation in each of several named
Environments. Loops choose a fixed pair or expose authorized choices to the model; Brain validates
the actual pair and canonical input/output independently of model presentation.

Agentloop authors can import generated `ModelRequest`, `ModelResult`, `ToolResult`, `EventPage`, and
`SessionTranscript` types. The JSON schemas ship at `@aexhq/brain/contracts/session.json`; WIT ships
at `@aexhq/brain/contracts/agentloop.wit` and `@aexhq/brain/contracts/tool.wit`. `events(after)` reads
history during an activation, and `emit` appends extension Events. Model, Tool, and Environment
failures are never retried by Brain.

## Upgrading to 0.24

Replace SDK `session.cancel()` with `session.interrupt()` and close each client when finished.
The HTTP cancellation route and stored session format are unchanged. Host streams now remain
open for the client lifetime. `register()` and `credentials()` retain their existing roles;
saved host credentials are separate from provider and API credentials.

## Upgrading from 0.19

Upgrade the server and SDK together and rebuild Agentloop Components against the packaged WIT.
Replace `set_kv` with `kv_put`; bindings expose `kv.read/put/delete`. Remove `needs` from
extension declarations and configure native resource grants explicitly on their Environment.
Do not translate per-Tool grants into a shared union without choosing that authority boundary.

This pre-stable change does not migrate retained 0.19 sessions or old Agentloop binaries.
Keep their matching server/artifacts for recovery; use a separately admitted new session for
the new contract. Deployment must not discard or rewrite existing session data implicitly.
