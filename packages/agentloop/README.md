# @aexhq/agentloop

Author custom agentloops for Brain sessions. An agentloop is capability-pure policy code
driving a session's turns; the Brain kernel executes and journals every effect through
`contracts/agentloop/v1`, and your loop runs isolated in the managed loop host.

```js
import { defineAgentloop } from "@aexhq/agentloop";

export const { activate } = defineAgentloop({
  async onMessage(ctx, message) {
    const round = await ctx.model.stream({
      system: "answer briefly",
      messages: [{ role: "user", content: message.content }],
    });
    const calls = round.content.filter((block) => block.type === "tool_call");
    if (calls.length > 0) {
      await ctx.tools.dispatch(calls);
    }
    await ctx.kv.set({ last_turn_at: Date.now() });
    // Returning is finishing; throw to fail the turn.
  },
});
```

The ctx surface is deliberately small and journal-only durable:

- `ctx.model.stream(request)` — one round against the session's **sealed** provider and model.
  Presentation (system text, which sealed tools you show, sampling) is yours; authority is not.
- `ctx.tools.dispatch(calls)` — execute calls against the sealed tool grant.
- `ctx.journal.append(entries)` / `ctx.journal.read(query)` — durable loop entries: `custom`
  (opaque), `event` (surfaces to the application as `loop.event`), and `mark` (your hydration
  floor). `ctx.kv.get/set` — a small durable key/value map.
- `ctx.turn.finish(result?)` / `ctx.turn.fail(error)` — the turn's terminal.
- `onSessionStart(start)` — a fresh instance's hydration: durable kv, the latest mark, and the
  entry tail after it. Loop memory is a cache; this is how it rebuilds.

Build the uploadable bundle with the node-only builder:

```js
import { buildLoopBundle } from "@aexhq/agentloop/build";

const bundle = await buildLoopBundle({ entry: "./my-loop.mjs" });
// bundle.sha256 + the toolchain you upload under = the sealed identity of your loop.
```

The builder produces a deterministic ESM source bundle (the sealed identity is its SHA-256
plus the server toolchain that componentizes it) and refuses `\p{…}` Unicode property
escapes, which the pinned guest engine rejects at parse time.
