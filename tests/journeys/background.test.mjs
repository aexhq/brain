import assert from "node:assert/strict";
import test from "node:test";
import { agentloop, brainEnv, hostEnv, tool } from "@aexhq/brain";
import { z } from "zod";
import { fixture, collect } from "./support.mjs";

const returns = new Map();
const grants = new Map();
async function call(grant, method, input) {
  const response = await fetch(grant.url, { method: "POST", headers: {
    authorization: `Bearer ${grant.token}`, "content-type": "application/json",
  }, body: JSON.stringify({ method, input }) });
  assert.equal(response.status, 200, await response.clone().text());
  return response.json();
}

const f = fixture({ providers: {
  observer: async ({ operation: op }) => {
    let receipt = { type: "accepted" };
    if (op.request.type === "execute") {
      const { input, callback } = op.request;
      if (input.input) {
        const environment = input.tools[0].environments[0];
        returns.set(op.session_id, await call(callback, "dispatch", [
          { call_id: "background", environment, name: "work", input: { background: true } },
        ]));
        await call(callback, "set_transcript", [{ role: "user", content: [{ type: "text", text: "loop-selected" }] }]);
      }
      let after = input.kv["brain.last_activation"] ?? 0;
      for (;;) {
        const page = await call(callback, "events", after);
        if (!page.events.length) break;
        for (const event of page.events) if (event.event_type === "tool_call_ended") {
          await call(callback, "kv_put", { key: "observed_finish", value: event.sequence });
        }
        after = page.next_cursor;
      }
      await call(callback, "acknowledge", after);
      receipt = { type: "result", output: {} };
    }
    return { contract: "environment/v1", sequence: op.sequence, receipt };
  },
  remote: async ({ operation: op }) => {
    if (op.request.type === "execute") grants.set(op.session_id, op.request.callback);
    return { contract: "environment/v1", sequence: op.sequence,
      receipt: op.request.type === "execute" ? { type: "returned" } : { type: "accepted" } };
  },
} });

for (const placement of ["host", "http", "native"]) {
  test(`${placement} Tool return preserves later emissions and finish while the loop owns its transcript`, { timeout: 30_000 }, async t => {
    let context;
    const work = tool({ name: "work", description: "Return before completing", input: z.object({ background: z.boolean() }),
      ...(placement === "host" ? { run: (_, call) => { context = call; } }
        : { implementation: placement === "native" ? f.toolComponent : { type: "background" } }),
    });
    const env = placement === "host" ? hostEnv({ name: "app" })
      : placement === "native" ? brainEnv({ name: "brain" }) : f.provider("remote");
    const session = await f.create(t, {
      agentloop: agentloop({ implementation: { type: "observer" } })({ env: f.provider("observer") }),
      tools: [work({ env })],
    });
    await session.send("start");
    const [returned] = returns.get(session.id);
    assert.equal(returned.finished, false);
    assert.equal(returned.events.filter(event => event.event_type === "tool_result_emitted").length, 0);
    if (placement === "host") {
      await context.emitResult({ value: "later" });
      await context.finish();
      await assert.rejects(context.emitResult("too late"), /finished/u);
    } else if (placement === "http") {
      const grant = grants.get(session.id);
      await call(grant, "result", { status: "ok", value: "later" });
      await call(grant, "finish", null);
    }
    let observed;
    for await (const event of session.stream(0, AbortSignal.timeout(10_000))) {
      if (event.type === "kv_set" && event.data.key === "observed_finish") { observed = event.data.value; break; }
    }
    const events = await collect(session.events());
    const result = events.find(event => event.type === "tool_result_emitted");
    assert.ok(result.sequence < observed);
    assert.equal(events.filter(event => event.type === "tool_call_ended").length, 1);
    if (placement !== "native") assert.ok(events.some(event => event.type === "turn_started" && event.data.trigger === "events"));
    assert.deepEqual((await session.transcript()).messages,
      [{ role: "user", content: [{ type: "text", text: "loop-selected" }] }]);
    assert.equal(f.modelRequests.length, 0);
  });
}
