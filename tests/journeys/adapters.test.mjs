import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";
import { Model, ApiKey, build, fixture, config, message, call, result, history, state, until, root, removeTemp, modelReply } from "./fixture.mjs";
import { responsesReply } from "./model-responses.mjs";
let binaries;
before(async () => { binaries = await build(); }, { timeout: 120000 });
after(async () => { if (binaries) await removeTemp(binaries); });
const budget = { timeout: 120000 };
const selection = (harness, tools = []) => ({ ...config(tools), harness });
const adapters = ["codex", "opencode"];
async function setup(t, harness, options = {}) {
  const f = await fixture(t, binaries, { api: "openai-responses", ...options });
  f.settings.harnesses = { [harness]: { command: [process.execPath, path.join(root, `runners/${harness}/src/main.mjs`)] } };
  return f;
}

for (const harness of adapters) {
  test(`${harness}: a crashed in-flight turn restores passively without replaying abandoned input`, budget, async t => {
    const f = await setup(t, harness, { persistence: {} });
    await f.start(); const s = await f.api.create(selection(harness));
    await f.api.send(s.id, message("retained-before-native-crash")); await state(f.api, s.id); await f.api.flush(s.id);
    f.model.respond = async () => {};
    await f.api.send(s.id, message("abandoned-after-dispatch"));
    await until(() => f.model.requests.length, count => count === 2);
    await f.crash(); await f.start();
    await f.api.get(s.id); await f.api.resume(s.id); await f.api.get(s.id);
    assert.equal(f.model.requests.length, 2, "cold recovery continued abandoned inference");
    const unknown = (await history(f.api, s.id)).filter(event => event.type === "model.request" && event.data.state === "unknown");
    assert.ok(unknown.length > 0);
    const keys = unknown.map(event => `${event.data.request_id}/${event.data.attempt_id ?? ""}`);
    assert.equal(new Set(keys).size, keys.length, "recovery duplicated unknown outcomes");
    await f.api.flush(s.id); await f.crash(); f.settings.data_dir = path.join(f.dir, "crashed-cold-cache"); await f.start();
    await f.api.get(s.id); await f.api.resume(s.id);
    assert.equal(f.model.requests.length, 2, "cold recovery continued abandoned inference");
    f.model.respond = undefined;
    await f.api.send(s.id, message("continue explicitly after the crash")); await state(f.api, s.id);
    const context = JSON.stringify(f.model.requests.at(-1).input);
    assert.ok(context.includes("retained-before-native-crash"));
    assert.ok(!context.includes("abandoned-after-dispatch"), "the abandoned turn was replayed into native context");
  });

  test(`${harness}: a user creates empty and exact catalogs and invokes an external tool`, budget, async t => {
    const f = await setup(t, harness);
    let toolCalls = 0;
    await f.tool("echo", async (req, res) => {
      let body = ""; for await (const chunk of req) body += chunk;
      const request = JSON.parse(body);
      assert.equal(request.tenant_id, "a"); assert.notEqual(request.session_id, request.arguments.session_id);
      toolCalls++; res.setHeader("Content-Type", "application/json"); res.end(JSON.stringify(result("external echo")));
    });
    await f.start();
    const empty = await f.api.create(selection(harness));
    const exact = await f.api.create(selection(harness, ["echo"]));
    assert.equal(f.model.requests.length, 0, "admission started inference");
    await f.api.send(empty.id, message("Say hello")); await state(f.api, empty.id);
    assert.deepEqual((f.model.requests[0].tools ?? []).map(tool => tool.name), []);
    await f.api.send(exact.id, message(call("echo", { session_id: "spoof", value: "hello" }))); await state(f.api, exact.id);
    assert.equal(toolCalls, 1);
    assert.deepEqual(f.model.requests[1].tools.map(tool => tool.name), ["echo"]);
    assert.ok((await history(f.api, exact.id)).some(event => event.type === "tool.result" && !event.data.result.is_error));
    const evidence = (await history(f.api, exact.id)).filter(event => event.type === "model.request");
    assert.ok(evidence.some(event => event.data.state === "completed"));
    await f.api.close(empty.id); // Releases native residency while the peer remains usable.
    await f.api.send(exact.id, message("Continue after peer detachment")); await state(f.api, exact.id);
  });

  test(`${harness}: independent private keys reach concurrent native peers`, budget, async t => {
    const keys = ["adapter-customer-one-private", "adapter-customer-two-private"];
    const f = await setup(t, harness, { modelKeys: keys, persistence: {} });
    f.env.JOURNEY_KEYS = Buffer.alloc(32, 5).toString("base64");
    f.settings.credential_keys = { active: "test", keys: { test: "JOURNEY_KEYS" } };
    await f.start();
    const customers = [{ client: f.api, key: keys[0], label: "customer-one-context" }, { client: f.b, key: keys[1], label: "customer-two-context" }];
    const sessions = await Promise.all(customers.map(({ client, key }) => client.create({ harness, model: Model("openai/gpt-4.1-mini", ApiKey(key)), tools: [] })));
    const pending = [];
    f.model.respond = async (_input, response) => { pending.push(response); if (pending.length === 2) for (const peer of pending) responsesReply(peer, "independent"); };
    await Promise.all(sessions.map((s, i) => customers[i].client.send(s.id, message(customers[i].label))));
    await Promise.all(sessions.map((s, i) => state(customers[i].client, s.id)));
    assert.deepEqual(f.model.authorization.toSorted(), keys.map(key => `Bearer ${key}`).toSorted());
    await Promise.all(sessions.map((s, i) => customers[i].client.flush(s.id)));
    await f.crash(); f.settings.data_dir = path.join(f.dir, "private-cold-cache"); await f.start();
    f.model.respond = undefined;
    for (const [i, s] of sessions.entries()) {
      const client = customers[i].client;
      await client.get(s.id); await client.resume(s.id);
      await client.send(s.id, message("Continue my own context")); await state(client, s.id);
      const input = JSON.stringify(f.model.requests.at(-1).input);
      assert.ok(input.includes(customers[i].label)); assert.ok(!input.includes(customers[1 - i].label));
      assert.equal(f.model.authorization.at(-1), `Bearer ${keys[i]}`);
      const data = JSON.stringify([await client.get(s.id), await history(client, s.id)]);
      for (const key of keys) assert.ok(!data.includes(key) && !f.logs.includes(key));
    }
  });

  test(`${harness}: native checkpoints append and cold restore preserves the next request`, budget, async t => {
    const f = await setup(t, harness, { persistence: {} });
    f.model.respond = async (_input, response) => responsesReply(response, f.model.requests.length === 1 ? "prior-native-context ".repeat(6000) : "native continuation", { input_tokens: 20, output_tokens: 5, total_tokens: 25 });
    await f.start();
    const s = await f.api.create(selection(harness));
    await f.api.send(s.id, message("Remember my prior context")); await state(f.api, s.id);
    const file = path.join(f.settings.data_dir, "sessions", s.id, "native", harness === "codex" ? "rollout.jsonl" : "session.jsonl");
    const first = await readFile(file);
    await f.api.send(s.id, message("Continue before restart")); await state(f.api, s.id);
    const next = await readFile(file);
    assert.deepEqual(next.subarray(0, first.length), first);
    assert.ok(next.length > first.length && next.length - first.length < first.length);
    await f.api.flush(s.id); await f.crash();
    f.settings.data_dir = path.join(f.dir, "native-cold-cache"); await f.start();
    const before = f.model.requests.length;
    await f.api.get(s.id); await f.api.resume(s.id);
    assert.equal(f.model.requests.length, before, "passive recovery resumed native work");
    await f.api.send(s.id, message("Continue after cold recovery")); await state(f.api, s.id);
    assert.ok(JSON.stringify(f.model.requests.at(-1).input).includes("prior-native-context"));
    assert.ok(JSON.stringify(f.model.requests.at(-1).input).includes("native continuation"));
  });

  test(`${harness}: cancelling one active session preserves a concurrent peer`, budget, async t => {
    const f = await setup(t, harness);
    f.model.respond = async (input, response) => { if (!JSON.stringify(input.input).includes("hold this request")) responsesReply(response, "peer completed"); };
    await f.start();
    const a = await f.api.create(selection(harness)), b = await f.api.create(selection(harness));
    await Promise.all([f.api.send(a.id, message("hold this request")), f.api.send(b.id, message("Keep the peer running"))]);
    await state(f.api, b.id); await until(() => f.model.requests.length, n => n === 2);
    await f.api.interrupt(a.id); await state(f.api, a.id, "interrupted");
    await f.api.send(b.id, message("The peer remains available")); await state(f.api, b.id);
    const receipts = (await history(f.api, a.id)).filter(event => event.type === "model.request");
    assert.ok(receipts.some(event => ["cancelled", "unknown"].includes(event.data.state)));
  });

  test(`${harness}: native automatic compaction preserves context through cold recovery`, budget, async t => {
    const f = await setup(t, harness, { persistence: {} });
    f.model.respond = async (input, response) => responsesReply(response, f.model.requests.length === 1 ? "context before compaction ".repeat(5000) : harness === "opencode" && JSON.stringify(input).includes("<template>") ? "## Objective\nnative-compaction-marker\n\n## Requirements\n(none)\n\n## Decisions\n(none)\n\n## Work State\n### Completed\nEstablish context\n### Active\nContinue\n### Blocked\n(none)\n\n## Next Move\n1. Continue\n\n## Relevant Files\n(none)\n\n## Important Context\nnative-compaction-marker" : "native-compaction-marker", { input_tokens: f.model.requests.length === 1 ? 1_100_000 : 20, output_tokens: 5, total_tokens: f.model.requests.length === 1 ? 1_100_005 : 25 }, input.model);
    await f.start();
    const s = await f.api.create(selection(harness));
    await f.api.send(s.id, message("Establish native context")); await state(f.api, s.id);
    await f.api.send(s.id, message("Continue with native compaction")); await state(f.api, s.id);
    assert.ok(f.model.requests.length >= 3, "native compaction did not issue a summary request");
    const native = await readFile(path.join(f.settings.data_dir, "sessions", s.id, "native", harness === "codex" ? "rollout.jsonl" : "session.jsonl"), "utf8");
    assert.ok(native.split("\n").some(line => line && (harness === "codex" ? JSON.parse(line).type === "compacted" : JSON.parse(line).message?.type === "compaction" && JSON.parse(line).message.status === "completed")), "native compaction was not checkpointed");
    await f.api.flush(s.id); await f.crash(); f.settings.data_dir = path.join(f.dir, "compaction-cold-cache"); await f.start();
    const before = f.model.requests.length; await f.api.get(s.id); await f.api.resume(s.id);
    assert.equal(f.model.requests.length, before);
    await f.api.send(s.id, message("Continue after native compaction recovery")); await state(f.api, s.id);
    assert.ok(JSON.stringify(f.model.requests.at(-1).input).includes("native-compaction-marker"));
  });

  test(`${harness}: unbound native calls are unavailable and failures preserve explicit usage gaps`, budget, async t => {
    const f = await setup(t, harness);
    let unbound = false;
    f.model.respond = async (input, response) => {
      if (!unbound) { unbound = true; responsesReply(response, { name: "shell", args: {} }); }
      else responsesReply(response, "unbound tool denied", { input_tokens: 20, output_tokens: 5, total_tokens: 25 });
    };
    await f.start();
    const s = await f.api.create(selection(harness));
    await f.api.send(s.id, message("Reject unbound tools")); await state(f.api, s.id);
    assert.ok(JSON.stringify(f.model.requests.at(-1).input).match(/not found|not enabled|unsupported|not available|unknown tool|no tool named/i));
    assert.equal((await history(f.api, s.id)).filter(event => event.type === "tool.call").length, 0);
    const evidence = (await history(f.api, s.id)).filter(event => event.type === "model.request" && event.data.state === "completed");
    assert.ok(evidence.some(event => harness === "codex" ? event.data.boundary === "native_turn" && event.data.unmeasured.includes("retries") : event.data.boundary === "request" && event.data.attempts === "observed"));
  });

  test(`${harness}: native model retries and request rejection do not invent complete usage`, budget, async t => {
    const f = await setup(t, harness);
    f.model.respond = async (input, response) => {
      if (f.model.requests.length === 1) { response.writeHead(429, { "Content-Type": "application/json", "Retry-After": "0" }); response.end(JSON.stringify({ error: { message: "rate limit" } })); }
      else responsesReply(response, "native retry completed", { input_tokens: 20, output_tokens: 5, total_tokens: 25 }, input.model);
    };
    await f.start(); const s = await f.api.create(selection(harness));
    await f.api.send(s.id, message("Retry inference")); await state(f.api, s.id);
    assert.equal(f.model.requests.length, 2);
    let receipts = (await history(f.api, s.id)).filter(event => event.type === "model.request" && event.data.state !== "started");
    if (harness === "codex") assert.ok(receipts.every(event => event.data.usage.availability !== "reported"));
    else {
      assert.ok(receipts.some(event => event.data.attempt_id && event.data.state === "failed" && event.data.usage.availability === "unknown"));
      assert.ok(receipts.some(event => event.data.attempt_id && event.data.state === "completed" && event.data.usage.input_tokens === 20));
    }
    f.model.respond = async (_input, response) => { response.writeHead(400, { "Content-Type": "application/json" }); response.end(JSON.stringify({ error: { message: "fixture rejected" } })); };
    await f.api.send(s.id, message("Rejected before output")); await state(f.api, s.id, "interrupted");
    receipts = (await history(f.api, s.id)).filter(event => event.type === "model.request" && event.data.state === "failed");
    assert.ok(receipts.length > 0 && receipts.every(event => event.data.usage.availability === "unknown"));
  });
}

test("opencode: raw cache and reasoning evidence stays separate from unknown partial-stream usage", budget, async t => {
  const f = await setup(t, "opencode");
  f.model.respond = async (_input, response) => responsesReply(response, "measured", { input_tokens: 20, output_tokens: 10, total_tokens: 30, input_tokens_details: { cached_tokens: 8 }, output_tokens_details: { reasoning_tokens: 4 } });
  await f.start(); const s = await f.api.create(selection("opencode"));
  await f.api.send(s.id, message("measured request")); await state(f.api, s.id);
  let receipts = (await history(f.api, s.id)).filter(event => event.type === "model.request" && event.data.attempt_id && event.data.state === "completed");
  assert.equal(receipts.length, 1);
  assert.deepEqual(receipts[0].data.usage, { availability: "reported", semantics: "snapshot", input_tokens: 20, output_tokens: 10, cache_read_tokens: 8, reasoning_tokens: 4, input_cache: "included", output_reasoning: "included" });
  let partial = false;
  const observer = await f.observe(s.id);
  t.after(() => observer.stop());
  f.model.respond = async (_input, response) => {
    if (partial) { response.writeHead(400, { "Content-Type": "application/json" }); response.end(JSON.stringify({ error: { message: "fixture stops retries" } })); return; }
    partial = true; let cut = false;
    responsesReply({ writeHead: (...args) => response.writeHead(...args), write(line) { if (!cut) response.write(line); if (line.includes("response.output_text.delta")) { cut = true; setTimeout(() => response.destroy(), 20); } }, end() {} }, "partial native output");
  };
  await f.api.send(s.id, message("disconnect after partial output")); await state(f.api, s.id, "interrupted");
  receipts = (await history(f.api, s.id)).filter(event => event.type === "model.request" && event.data.attempt_id && ["failed", "unknown"].includes(event.data.state));
  assert.ok(receipts.length > 0 && receipts.every(event => event.data.usage.availability === "unknown"));
  await observer.wait(event => event.type === "assistant.delta" && event.data.text.includes("partial native output"));
});
