import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "bun:test";
import { openCodeHarness } from "../../dist/harness-opencode.js";
import { createHarnessContext, HarnessServiceError } from "../../dist/harness.js";

async function fixture(options, test) {
  const directory = await mkdtemp(join(tmpdir(), "brain-opencode-"));
  const harness = openCodeHarness();
  let session = await harness.open({ sessionId: "ses_test", stateDirectory: directory, configuration: {} });
  const controller = new AbortController();
  const requests = [], effects = [], transcripts = [];
  const release = Promise.withResolvers(), reached = Promise.withResolvers();
  let count = 0;
  const barrier = async kind => {
    if (options.block === kind) { reached.resolve(); await release.promise; }
    if (controller.signal.aborted) throw new HarnessServiceError("activation cancelled", "cancelled", true);
    if (options.fail === kind) {
      const error = new HarnessServiceError(`commit failed: ${kind}`, "internal", true);
      controller.abort(error); throw error;
    }
  };
  const kv = new Map();
  const request = async (method, value) => {
    if (method === "kv_read") return { value: kv.get(value) };
    if (method === "kv_put") { kv.set(value.key, value.value); return 1; }
    if (method === "model") {
      await barrier("model-start"); requests.push(value); effects.push("model");
      const first = count++ === 0;
      await barrier("model-end");
      return { message: { role: "assistant", content: first ? [
        { type: "native", format: "openai-responses/v1", data: { type: "reasoning", id: "native-id", encrypted_content: "opaque" } },
        { type: "tool_use", id: "call-1", name: options.unoffered ? "execute" : "brain_echo", input: {} },
      ] : [{ type: "text", text: options.compaction ? "## Objective\n- Complete the request.\n## Work State\n### Completed\n- Called echo.\n## Next Move\n1. Finish." : "Finished." }] }, stop_reason: first ? "tool_use" : "end_turn",
      usage: { input_tokens: options.compaction && count === 2 ? 63900 : 50, output_tokens: 10 } };
    }
    if (method === "dispatch") {
      await barrier("tool-start"); effects.push("tool");
      assert.equal(value[0].name, "echo"); assert.equal(value[0].environment, "host");
      await barrier("tool-end");
      return [{ call_id: "call-1", sequence: 10, finished: true, events: [{ sequence: 11, recorded_at_ms: 0,
        event_type: "tool_result_emitted", data: { result: { call_id: "call-1", output: "ok", is_error: false } } }] }];
    }
    if (method === "set_transcript") transcripts.push(value);
    else if (method !== "emit") throw new Error(`unexpected callback: ${method}`);
    return 12;
  };
  const f = { requests, effects, transcripts, release, reached,
    async cancel() {
      controller.abort(new Error("activation cancelled"));
      const cancelling = session.cancel(); release.resolve(); await cancelling;
    },
    async reopen() { await session.close(); session = await harness.open({ sessionId: "ses_test", stateDirectory: directory, configuration: {} }); },
    run(message = "Call echo and finish. " + "Earlier context. ".repeat(250)) {
      return session.run(createHarnessContext({ session_id: "ses_test", sequence: 3 + count,
        model: { id: "fixture", context_window_tokens: 64000, max_output_tokens: 1024 },
        input: { input: { message }, transcript: [], kv: {}, events: [], system: "Use the provided tools.",
          tools: [{ name: "echo", description: "Echo", input_schema: { type: "object" }, environments: ["host"], environment_refs: [] }],
          configuration: { compaction: { auto: options.compaction ?? false, buffer: 1000, keep: { tokens: 1 } } },
          runtime: { logical_time_ms: 1, deterministic_seed: Array(32).fill(0) } },
      }, controller.signal, request));
    },
  };
  try { await test(f); }
  finally { await session.close(); await rm(directory, { recursive: true, force: true }); }
}

test("OpenCode mediates tools, preserves continuation state, and resumes its native database", async () => fixture({ block: "tool-end" }, async f => {
  const running = f.run(); await f.reached.promise;
  assert.deepEqual(f.effects, ["model", "tool"]);
  f.release.resolve(); await running;
  assert.deepEqual(f.effects, ["model", "tool", "model"]);
  assert.deepEqual(f.requests[0].tools.map(tool => tool.name), ["brain_echo"]);
  assert(f.requests[1].messages.some(message => message.content.some(block => block.type === "native" && block.data.encrypted_content === "opaque")));
  await f.reopen(); await f.run("Continue");
  assert(f.requests.at(-1).messages.some(message => message.content.some(block => block.type === "text" && block.text === "Finished.")));
}), 30000);

test("OpenCode rejects an invented native execute tool before it can run", async () => fixture({ unoffered: true }, async f => {
  await assert.rejects(f.run(), /unoffered tool|failed/);
  assert.deepEqual(f.effects, ["model"]);
}), 30000);

test("OpenCode native compaction calls Brain's model adapter", async () => fixture({ compaction: true }, async f => {
  await f.run();
  await f.run("Continue after the long response");
  assert(f.requests.some(request => JSON.stringify(request.messages).includes("You MUST summarize")), "upstream compaction must call Brain");
}), 30000);

for (const [fail, effects] of [["model-start", []], ["model-end", ["model"]], ["tool-start", ["model"]], ["tool-end", ["model", "tool"]]]) {
  test(`OpenCode stops after a fatal ${fail} callback`, async () => fixture({ fail }, async f => {
    await assert.rejects(f.run(), /commit failed/);
    assert.deepEqual(f.effects, effects);
  }), 30000);
}

test("OpenCode cancels while the mediated model result is pending", async () => fixture({ block: "model-end" }, async f => {
  const running = f.run();
  const rejected = assert.rejects(running, /cancelled|interrupt|aborted/);
  await f.reached.promise; await f.cancel(); await rejected;
  assert.deepEqual(f.effects, ["model"]);
}), 30000);
