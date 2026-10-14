import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { createHarnessContext, HarnessServiceError } from "../dist/harness.js";
process.env.PI_OFFLINE = "1";
const { piHarness } = await import("../dist/harness-pi.js");

function deferred() {
  let resolve;
  const promise = new Promise(r => { resolve = r; });
  return { promise, resolve };
}

async function fixture(t, { block, fail, compaction = false } = {}) {
  const directory = await mkdtemp(join(tmpdir(), "brain-pi-"));
  const harness = piHarness();
  let session = await harness.open({ sessionId: "ses_test", stateDirectory: directory, configuration: {} });
  const controller = new AbortController();
  t.after(async () => { await session.close(); await rm(directory, { recursive: true, force: true }); });
  const released = deferred(), reached = deferred();
  const effects = [], requests = [], commits = [], transcripts = [], outputs = [];
  let calls = 0;
  async function barrier(kind) {
    if (kind === block) { reached.resolve(); await released.promise; }
    if (kind === fail) {
      const error = new HarnessServiceError(`commit failed: ${kind}`, "internal", true);
      controller.abort(error);
      throw error;
    }
    commits.push(kind);
  }
  const kv = new Map();
  const request = async (method, value) => {
    if (method === "kv_read") return { value: kv.get(value) };
    if (method === "kv_put") { kv.set(value.key, value.value); return 1; }
    if (method === "model") {
      await barrier("model-start");
      effects.push("model"); requests.push(value);
      const use = calls++ === 0;
      const message = { role: "assistant", content: use
        ? [{ type: "native", format: "openai-responses/v1", data: { type: "reasoning", id: "native-reasoning", encrypted_content: "opaque" } },
          { type: "tool_use", id: "call-1", name: "echo", input: { value: "ok" } }]
        : [{ type: "text", text: "Finished." }] };
      await barrier("model-end");
      return { message, stop_reason: use ? "tool_use" : "end_turn", usage: { input_tokens: compaction && calls === 2 ? 63900 : 50, output_tokens: 10 } };
    }
    if (method === "dispatch") {
      await barrier("tool-start"); effects.push("tool");
      assert.deepEqual(value, [{ call_id: "call-1", name: "echo", environment: "host", input: { value: "ok" } }]);
      await barrier("tool-end");
      return [{ call_id: "call-1", sequence: 10, finished: true, events: [{ sequence: 11, recorded_at_ms: 0, event_type: "tool_result_emitted",
        data: { result: { call_id: "call-1", output: "ok", is_error: false } } }] }];
    }
    if (method === "set_transcript") { transcripts.push(value); return 12; }
    if (method === "emit") { outputs.push(value); return 13; }
    if (method === "events") return { events: [], next_cursor: value };
    throw new Error(`unexpected callback ${method}`);
  };
  function run(message = "Call echo", media = []) {
    return session.run(createHarnessContext({ session_id: "ses_test", sequence: 3 + calls,
      model: { id: "fixture", context_window_tokens: 64000, max_output_tokens: 1024 }, input: {
        input: { message, media }, transcript: [], kv: {}, events: [], system: "Use the provided tools.",
        tools: [{ name: "echo", description: "Echo", input_schema: { type: "object", properties: { value: { type: "string" } }, required: ["value"] }, environments: ["host"], environment_refs: [] }],
        configuration: { compaction: { enabled: compaction, keepRecentTokens: 1, reserveTokens: 1000 } },
        runtime: { logical_time_ms: 1, deterministic_seed: Array(32).fill(0) },
      } }, controller.signal, request));
  }
  return { run, effects, requests, commits, transcripts, outputs, released, reached,
    async reopen() { await session.close(); session = await harness.open({ sessionId: "ses_test", stateDirectory: directory, configuration: {} }); } };
}

test("Pi awaits Brain's model and tool journal barriers and preserves native continuation", async t => {
  const f = await fixture(t, { block: "tool-end" });
  const running = f.run();
  await f.reached.promise;
  assert.deepEqual(f.effects, ["model", "tool"]);
  assert(!f.commits.includes("tool-end"));
  f.released.resolve(); await running;
  assert.deepEqual(f.effects, ["model", "tool", "model"]);
  assert.equal(f.requests[1].messages[1].content[0].data.encrypted_content, "opaque");
  assert.equal(f.requests[1].messages[2].content[0].type, "tool_result");
  assert.equal(f.outputs.at(-1).data.message, "Finished.");
});

test("Pi blocks before dispatch, resumes native state, and carries URL media without fetching", async t => {
  const f = await fixture(t, { block: "model-start" });
  const media = [{ type: "image", url: "https://example.com/image.png" }];
  const running = f.run("Remember this image", media);
  await f.reached.promise;
  assert.deepEqual(f.effects, []);
  f.released.resolve(); await running;
  assert.deepEqual(f.requests[0].messages[0].content.at(-1), media[0]);
  await f.reopen(); await f.run("What did we do?");
  assert(f.requests.at(-1).messages.some(m => m.content.some(c => c.type === "text" && c.text === "Finished.")));
  assert(f.requests.at(-1).messages.some(m => m.content.some(c => c.type === "image" && c.url === media[0].url)));
});

test("Pi's native automatic compaction uses the same Brain model callback", async t => {
  const f = await fixture(t, { compaction: true });
  await f.run("Call echo, then finish. " + "Earlier conversation context. ".repeat(250));
  assert(f.requests.some(request => request.tools.length === 0), "upstream summary request must use Brain");
  assert(f.requests.length >= 3);
  await f.reopen(); await f.run("Continue after compaction");
});

for (const [fail, effects] of [
  ["model-start", []], ["model-end", ["model"]],
  ["tool-start", ["model"]], ["tool-end", ["model", "tool"]],
]) {
  test(`Pi stops after a fatal ${fail} callback`, async t => {
    const f = await fixture(t, { fail });
    await assert.rejects(f.run(), /commit failed/);
    assert.deepEqual(f.effects, effects);
  });
}
