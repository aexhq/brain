import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { access, appendFile, readFile, readdir, rename, unlink, writeFile } from "node:fs/promises";
import path from "node:path";
import { responsesReply } from "./model-responses.mjs";
import { Pi, Brain, Model, ApiKey, Tool, ManagedTool, build, call, config, delay, fixture, history, jsonBody, message,
  modelReply, rejects, removeTemp, result, root, state, tokens, until,
} from "./fixture.mjs";

let binaries;
before(async () => { binaries = await build(); }, { timeout: 120000 });
after(async () => { if (binaries) await removeTemp(binaries); });
const budget = { timeout: 90000 };
const completed = (input, event) => event.type === "session.status" && event.input_id === input.id && event.data.status === "idle";
const retained = events => events.filter(event => event.cursor);
const ids = events => events.map(event => event.id);
const toolResults = events => events.filter(event => event.type === "tool.result");

test("installed SDK deletion removes local and archived state and prevents cold resurrection", budget, async t => {
  const f = await fixture(t, binaries, { persistence: {} });
  await f.start();
  const session = await f.api.create(config());
  await f.api.send(session.id, message("Retain private history before deletion"));
  await state(f.api, session.id);
  const checkpoint = await f.api.flush(session.id);
  const foreign = new Brain(f.url, tokens.b);
  await rejects(foreign.delete(session.id), 404, "not_found");
  await f.api.delete(session.id);
  await f.api.delete(session.id);
  await rejects(f.api.get(session.id), 404, "not_found");
  assert.ok(!(await f.api.list()).sessions.some(value => value.id === session.id));
  await assert.rejects(access(path.join(f.settings.data_dir, "sessions", session.id)));
  await f.crash(); f.settings.data_dir = path.join(f.dir, "deleted-cold-cache"); await f.start();
  await rejects(f.api.restore(session.id, checkpoint.checkpoint), 404, "not_found");
  const python = await f.python(path.join(root, "tests/journeys/python_delete.py"));
  assert.equal(python.deleted, true);
});

test("Pi receipts preserve reported cache/reasoning subsets on both OpenAI transports", budget, async t => {
  for (const api of ["openai-completions", "openai-responses"]) await t.test(api, async t => {
    const f = await fixture(t, binaries, { api });
    await f.start();
    const s = await f.api.create(config());
    f.model.respond = async (_input, response) => {
      const starts = (await history(f.api, s.id)).filter(event => event.type === "model.request" && event.data.state === "started");
      assert.ok(starts.some(event => event.data.attempt_id), "provider dispatched before physical start was retained");
      response.setHeader("x-request-id", "request-evidence");
      if (api === "openai-responses") responsesReply(response, "counted", { input_tokens: 20, output_tokens: 10, total_tokens: 30, input_tokens_details: { cached_tokens: 8 }, output_tokens_details: { reasoning_tokens: 4 } });
      else modelReply(response, "counted", { prompt_tokens: 20, completion_tokens: 10, total_tokens: 30, prompt_tokens_details: { cached_tokens: 8 }, completion_tokens_details: { reasoning_tokens: 4 } });
    };
    await f.api.send(s.id, message("Account for this generation")); await state(f.api, s.id);
    const receipts = (await history(f.api, s.id)).filter(event => event.type === "model.request" && event.data.state === "completed");
    assert.equal(receipts.length, 2);
    const request = receipts.find(event => event.data.attempt_id).data;
    assert.equal(request.provider_request_id, "request-evidence");
    assert.deepEqual(request.usage, { availability: "reported", semantics: "snapshot", input_tokens: 12, output_tokens: 10, cache_read_tokens: 8, reasoning_tokens: 4, input_cache: "excluded", output_reasoning: "included" });
    assert.equal(request.usage.input_tokens + request.usage.cache_read_tokens + request.usage.output_tokens, 30);
  });
});

test("Pi records retry attempts and leaves absent provider usage unknown", budget, async t => {
  const f = await fixture(t, binaries);
  await f.start();
  const s = await f.api.create(config());
  f.model.respond = async (_input, response) => {
    if (f.model.requests.length === 1) { response.writeHead(429, { "Content-Type": "application/json", "Retry-After": "0" }); response.end(JSON.stringify({ error: { message: "rate limit" } })); }
    else modelReply(response, "retried");
  };
  await f.api.send(s.id, message("Retry the model, never a tool action")); await state(f.api, s.id);
  assert.equal(f.model.requests.length, 2);
  const receipts = (await history(f.api, s.id)).filter(event => event.type === "model.request" && event.data.state !== "started");
  const physical = receipts.filter(event => event.data.attempt_id);
  assert.equal(physical.length, 2);
  assert.equal(physical[0].data.state, "failed"); assert.equal(physical[1].data.state, "completed");
  assert.ok(receipts.every(event => event.data.usage.availability === "unknown" && event.data.usage.input_tokens === undefined));
});

test("Pi evidence settles rejection, partial streams, cancellation and abrupt host loss", budget, async t => {
  for (const outcome of ["rejected", "partial", "cancelled", "crashed"]) await t.test(outcome, async t => {
    const f = await fixture(t, binaries);
    f.model.respond = async (_input, response) => {
      if (outcome === "rejected") { response.writeHead(400, { "Content-Type": "application/json" }); response.end(JSON.stringify({ error: { message: "fixture rejected request" } })); }
      else if (outcome === "partial") {
        response.writeHead(200, { "Content-Type": "text/event-stream" });
        response.write(`data: ${JSON.stringify({ id: "partial", choices: [{ index: 0, delta: { role: "assistant", content: "partial output" }, finish_reason: null }] })}\n\n`);
        await delay(30); response.destroy();
      }
    };
    await f.start();
    const s = await f.api.create(config());
    await f.api.send(s.id, message(`Exercise ${outcome}`));
    await until(() => f.model.requests.length);
    if (outcome === "cancelled") { await f.api.interrupt(s.id); await state(f.api, s.id, "interrupted"); }
    else if (outcome === "crashed") { await f.crash(); await f.start(); await f.api.get(s.id); }
    else await state(f.api, s.id, "interrupted");
    const evidence = (await history(f.api, s.id)).filter(event => event.type === "model.request");
    const latest = new Map(evidence.map(event => [`${event.data.request_id}/${event.data.attempt_id ?? ""}`, event.data]));
    assert.ok(latest.size >= 2);
    assert.ok([...latest.values()].every(value => value.state !== "started"), "an observed dispatch remained unsettled");
    assert.ok([...latest.values()].every(value => value.usage.availability === "unknown" && value.usage.input_tokens === undefined));
    if (outcome === "crashed") {
      assert.equal(evidence.filter(event => event.data.state === "unknown").length, 2);
      const before = evidence.length;
      await f.api.get(s.id);
      assert.equal((await history(f.api, s.id)).filter(event => event.type === "model.request").length, before);
      assert.equal(f.model.requests.length, 1);
    }
  });
});

test("Pi automatic compaction uses the private key and survives passive cold recovery", budget, async t => {
  const key = "private-compaction-provider-key";
  const f = await fixture(t, binaries, { persistence: {}, modelKeys: [key] });
  f.env.JOURNEY_ENCRYPTION = Buffer.alloc(32, 9).toString("base64");
  f.settings.credential_keys = { active: "test", keys: { test: "JOURNEY_ENCRYPTION" } };
  await f.start();
  const s = await f.api.create({ harness: Pi(), model: Model("openai/gpt-4.1-mini", ApiKey(key)), tools: [] });
  f.model.respond = async (_input, response) => {
    const n = f.model.requests.length;
    modelReply(response, n === 1 ? "retained prior context ".repeat(6000) : "compacted-context-marker", { prompt_tokens: n === 2 ? 1_100_000 : 20, completion_tokens: 5, total_tokens: n === 2 ? 1_100_005 : 25 });
  };
  await f.api.send(s.id, message("Build prior context")); await state(f.api, s.id);
  await f.api.send(s.id, message("Compact the native context")); await state(f.api, s.id);
  const evidence = (await history(f.api, s.id)).filter(event => event.type === "model.request");
  assert.ok(evidence.some(event => event.data.purpose === "compaction" && event.data.state === "completed"), "native compaction was unobserved");
  assert.ok(f.model.authorization.every(value => value === `Bearer ${key}`));
  await f.api.flush(s.id); await f.crash(); f.settings.data_dir = path.join(f.dir, "compacted-cold-cache"); await f.start();
  const before = f.model.requests.length;
  await f.api.get(s.id); await f.api.resume(s.id);
  assert.equal(f.model.requests.length, before);
  await f.api.send(s.id, message("Continue after compaction")); await state(f.api, s.id);
  assert.ok(JSON.stringify(f.model.requests.at(-1).messages).includes("compacted-context-marker"));
  assert.equal((await history(f.api, s.id)).filter(event => event.type === "model.request" && event.data.state === "unknown").length, 0);
});

test("request evidence replaces repeated snapshots and reconciles a crashed dispatch once", budget, async t => {
  const f = await fixture(t, binaries, { persistence: {} });
  f.settings.harnesses.evidence = { command: [process.execPath, path.join(root, "tests/journeys/evidence-runner.mjs")] };
  await f.start();
  const s = await f.api.create({ ...config(), harness: "evidence" });
  f.model.respond = async (input, response) => {
    const evidence = (await history(f.api, s.id)).filter(event => event.type === "model.request");
    assert.ok(evidence.some(event => event.data.request_id === input.messages[0].content && event.data.state === "started"), "inference preceded persisted start");
    response.setHeader("x-request-id", "provider-request");
    modelReply(response, "observed");
  };
  await f.api.send(s.id, message("all paths"));
  await state(f.api, s.id);
  const snapshots = (await history(f.api, s.id)).filter(event => event.type === "model.request");
  assert.equal(snapshots.length, 15, "identical native snapshots duplicated history");
  assert.deepEqual([...new Set(snapshots.map(event => event.data.purpose))].toSorted(), ["compaction", "other", "primary"]);
  const latest = new Map(snapshots.map(event => [`${event.data.request_id}/${event.data.attempt_id ?? ""}`, event.data]));
  assert.equal(latest.size, 6);
  assert.equal([...latest.values()].filter(value => value.usage.availability === "reported").length, 3);
  for (const value of latest.values()) {
    assert.equal(value.state, "completed"); assert.equal(value.harness, "evidence");
    assert.equal(value.model, "openai/gpt-4.1-mini");
    if (value.attempt_id) { assert.equal(value.usage.output_tokens, 4); assert.equal(value.usage.reasoning_tokens, 1); assert.equal(value.usage.output_reasoning, "included"); }
  }
  await f.api.send(s.id, message("unknown usage"));
  await state(f.api, s.id);
  const unknowns = (await history(f.api, s.id)).filter(event => event.type === "model.request" && event.data.state === "completed" && event.data.usage.availability === "unknown");
  assert.ok(unknowns.length > 0);
  assert.ok(unknowns.every(event => event.data.usage.input_tokens === undefined));
  f.model.respond = async () => {}; // Kill after a real dispatch, with no observed outcome.
  const before = f.model.requests.length;
  await f.api.send(s.id, message("pending dispatch"));
  await until(() => f.model.requests.length, count => count === before + 1);
  await f.api.flush(s.id);
  await f.crash();
  f.settings.data_dir = path.join(f.dir, "evidence-cold-cache");
  await f.start();
  await f.api.get(s.id);
  const recovered = await history(f.api, s.id);
  const unsettled = recovered.filter(event => event.type === "model.request" && event.data.state === "unknown");
  assert.equal(unsettled.length, 2);
  assert.ok(unsettled.every(event => event.data.usage.availability === "unknown"));
  await f.api.get(s.id);
  assert.equal((await history(f.api, s.id)).length, recovered.length, "re-reading recovery duplicated request records");
  assert.equal(f.model.requests.length, before + 1, "recovery redispatched inference");
});

test("a harness that omits terminal model evidence fails the affected turn", budget, async t => {
  const f = await fixture(t, binaries);
  f.settings.harnesses.evidence = { command: [process.execPath, path.join(root, "tests/journeys/evidence-runner.mjs")] };
  await f.start();
  const s = await f.api.create({ ...config(), harness: "evidence" });
  await f.api.send(s.id, message("omit outcome"));
  await state(f.api, s.id, "failed");
  const events = await history(f.api, s.id);
  assert.equal(events.filter(event => event.type === "model.request" && event.data.state === "unknown").length, 2);
  const peer = await f.api.create(config());
  await f.api.send(peer.id, message("Healthy peer"));
  await state(f.api, peer.id);
});

test("common host pool defaults and aggregate limits apply across installed factories", budget, async t => {
  for (const limited of [false, true]) await t.test(limited ? "configured ceiling" : "no aggregate restriction", async t => {
    const f = await fixture(t, binaries);
    f.settings.harnesses.peer = { ...f.settings.harnesses.pi };
    if (limited) f.settings.host_pool = { hosts: 1 };
    await f.start();
    const first = await f.api.create(config());
    await f.api.send(first.id, message("First factory"));
    await state(f.api, first.id);
    const second = await f.api.create({ ...config(), harness: "peer" });
    await f.api.send(second.id, message("Second factory"));
    await state(f.api, second.id, limited ? "failed" : "idle");
    assert.equal(f.model.requests.length, limited ? 1 : 2);
    if (limited) {
      assert.ok((await history(f.api, second.id)).some(event => event.type === "session.error"));
      await f.api.close(first.id);
      await f.api.resume(second.id);
      await f.api.send(second.id, message("Capacity released"));
      await state(f.api, second.id);
      assert.equal(f.model.requests.length, 2);
    }
  });
});

test("private model keys reach Pi independently and survive restart and cold restore", budget, async t => {
  const keys = ["customer-one-private-key", "customer-two-private-key"];
  const f = await fixture(t, binaries, { persistence: {}, modelKeys: keys });
  f.env.JOURNEY_ENCRYPTION = Buffer.alloc(32, 7).toString("base64");
  f.settings.credential_keys = { active: "test", keys: { test: "JOURNEY_ENCRYPTION" } };
  await f.start();
  const sessions = await Promise.all(keys.map(key => f.api.create({ harness: Pi(), model: Model("openai/gpt-4.1-mini", ApiKey(key)), tools: [] })));
  const peers = [];
  f.model.respond = async (_input, response) => {
    peers.push(response);
    if (peers.length === 2) {
      f.model.respond = undefined;
      for (const peer of peers) modelReply(peer, "independent customer keys");
    }
  };
  const run = async () => {
    for (const s of sessions) await f.api.send(s.id, message("Use the selected model."));
    await Promise.all(sessions.map(s => state(f.api, s.id)));
    assert.deepEqual(f.model.authorization.slice(-2).toSorted(), keys.map(key => `Bearer ${key}`).toSorted());
  };
  await run();
  await Promise.all(sessions.map(s => f.api.flush(s.id)));
  await f.crash();
  await f.start();
  await run();
  await f.crash();
  f.settings.data_dir = path.join(f.dir, "cold-cache");
  await f.start();
  const before = f.model.requests.length;
  for (const s of sessions) { await f.api.get(s.id); await f.api.resume(s.id); }
  assert.equal(f.model.requests.length, before, "recovery called the model");
  await run();
  for (const s of sessions) {
    const publicData = JSON.stringify([await f.api.get(s.id), await history(f.api, s.id)]);
    for (const key of keys) { assert.ok(!publicData.includes(key)); assert.ok(!f.logs.includes(key)); }
  }
  await f.crash();
  const inspect = async directory => {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) await inspect(file);
      else {
        const data = await readFile(file);
        for (const key of keys) assert.ok(!data.includes(Buffer.from(key)), `plaintext provider key in ${file}`);
      }
    }
  };
  await inspect(f.dir);
});

test("authoring constructors compose a managed service and a connected client function", budget, async t => {
  const f = await fixture(t, binaries);
  await f.tool("lookup", async (_req, res) => { res.setHeader("Content-Type", "application/json"); res.end(JSON.stringify(result("managed"))); });
  await f.start();
  let calls = 0;
  const s = await f.api.create({ harness: Pi(), model: Model("openai/gpt-4.1-mini"), tools: [
    ManagedTool("lookup"), Tool({ name: "local", description: "Read client data", input_schema: { type: "object" }, run() { calls++; return { value: "local" }; } }),
  ] });
  assert.deepEqual(s.descriptors.map(tool => tool.name), ["lookup", "runtime_local"]);
  await f.api.send(s.id, message(call("runtime_local")));
  await state(f.api, s.id);
  assert.equal(calls, 1);
  await f.api.send(s.id, message(call("lookup")));
  await state(f.api, s.id);
  assert.equal(toolResults(await history(f.api, s.id)).length, 2);
  await f.api.close(s.id);
  await f.api.dispose();
});

test("installed Python SDK owns its runtime function connection", budget, async t => {
  const f = await fixture(t, binaries);
  await f.start();
  const result = await f.python(path.join(root, "tests/journeys/python_runtime.py"));
  assert.equal(result.calls, 1);
  assert.equal(f.model.requests.length, 2);
});

test("a developer validates host configuration before creating local state or starting inference", budget, async t => {
  const f = await fixture(t, binaries, { persistence: {} });
  const valid = await f.check();
  assert.equal(valid.status, 0, valid.stderr);
  assert.match(valid.stdout, /configuration valid/);
  await assert.rejects(access(f.settings.data_dir), { code: "ENOENT" });
  await assert.rejects(access(f.settings.persistence.directory), { code: "ENOENT" });
  const invalid = [
    { ...f.settings, unknown_option: true },
    { ...f.settings, limits: { ...f.settings.limits, active: 0 } },
    { ...f.settings, tokens: [{ token_env: "JOURNEY_MISSING_TOKEN", tenant: "a" }] },
    { ...f.settings, harnesses: { pi: { command: [] } } },
    { ...f.settings, persistence: { provider: "not-installed" } },
    { ...f.settings, tools: [{ name: "bad", description: "invalid tool", input_schema: { $ref: "https://example.invalid/schema" }, url: "http://127.0.0.1:1", tenants: ["a"] }] },
  ];
  for (const value of invalid) {
    const checked = await f.check(value);
    assert.equal(checked.status, 1, checked.stdout);
    assert.match(checked.stderr, /brain stopped/);
  }
  assert.equal(f.model.requests.length, 0);
});

test("developer creates, streams, continues, paginates, deduplicates, and closes a conversation", budget, async t => {
  const f = await fixture(t, binaries);
  await f.start();
  assert.equal((await fetch(f.url + "/health/live")).status, 200);
  const s = await f.api.create(config());
  assert.equal(s.status, "idle");
  assert.deepEqual(s.config, config().config);
  assert.equal(s.checkpoint.files, undefined, "native artifacts leaked into public metadata");
  assert.deepEqual(await history(f.api, s.id), []);
  assert.equal(f.model.requests.length, 0, "creating/reading a session invoked inference");
  const stream = await f.observe(s.id);
  const input = message("Hello, 世界 🌍", "first");
  const accepted = await f.api.send(s.id, input);
  assert.equal(accepted.type, "user.message");
  assert.equal((await f.api.send(s.id, input)).id, accepted.id);
  await rejects(f.api.send(s.id, message("different", "first")), 409, "conflict");
  await stream.wait(event => completed(accepted, event));
  const events = await history(f.api, s.id, 1);
  assert.deepEqual(ids(retained(stream.events)), ids(events), "live stream and paginated history diverged");
  assert.equal(new Set(ids(events)).size, events.length);
  assert.equal(events.filter(event => event.type === "user.message").length, 1);
  const answer = events.find(event => event.type === "assistant.message");
  assert.equal(answer.input_id, accepted.id);
  assert.equal(answer.data.text, "answer: Hello, 世界 🌍");
  assert.ok(stream.events.some(event => event.type === "assistant.delta" && !event.cursor));
  assert.ok(events.every(event => !["assistant.delta", "tool.progress"].includes(event.type)));
  assert.equal(f.model.requests.length, 1);
  assert.ok(!f.model.requests[0].tools?.length, "unselected native tools were exposed");
  await stream.stop();

  const reconnect = await f.observe(s.id, { after: accepted.cursor });
  await reconnect.wait(event => event.id === events.at(-1).id);
  const next = await f.api.send(s.id, message("Continue this conversation", "second"));
  await reconnect.wait(event => completed(next, event));
  const all = await history(f.api, s.id);
  assert.deepEqual(ids(retained(reconnect.events)), ids(all.slice(1)), "replay/live boundary lost or duplicated retained events");
  assert.deepEqual(f.model.requests[1].messages.slice(0, f.model.requests[0].messages.length), f.model.requests[0].messages);
  assert.ok(JSON.stringify(f.model.requests[1].messages).includes(answer.data.text));
  assert.equal((await f.api.list()).sessions[0].id, s.id);
  await reconnect.stop();
  await f.api.close(s.id);
  await state(f.api, s.id, "closed");
  await rejects(f.api.send(s.id, message("too late")), 409, "conflict");
  await rejects(f.api.resume(s.id), 409, "conflict");
  assert.ok((await history(f.api, s.id)).some(event => event.id === answer.id), "closing erased history");
  assert.equal(f.model.requests.length, 2);
});

test("tenant, reader, participant, and delegated tool credentials enforce their scopes", budget, async t => {
  const f = await fixture(t, binaries, { persistence: {} });
  let sibling, foreign, delegated;
  const calls = [];
  await f.tool("inspect", async (req, res) => {
    assert.equal(req.headers.authorization, `Bearer ${tokens.tool}`);
    const body = await jsonBody(req);
    calls.push(body);
    assert.equal(body.tenant_id, "a");
    assert.notEqual(body.session_id, body.arguments.session_id, "model arguments overrode authenticated scope");
    assert.equal(body.brain_url, f.url);
    delegated = new Brain(body.brain_url, body.inspection_token);
    assert.equal((await delegated.get(body.session_id)).id, body.session_id);
    assert.deepEqual((await delegated.list()).sessions.map(s => s.id), [body.session_id]);
    assert.ok((await history(delegated, body.session_id)).some(event => event.type === "tool.call"));
    for (const id of [sibling.id, foreign.id]) {
      await rejects(delegated.get(id), 404, "not_found");
      await rejects(delegated.history(id), 404, "not_found");
    }
    await rejects(delegated.send(body.session_id, message("forged input")), 403, "forbidden");
    res.setHeader("Content-Type", "application/json"); res.end(JSON.stringify(result("own session inspected")));
  }, { inspect_session: true });
  await f.start();
  await rejects(new Brain(f.url, "wrong").list(), 401, "forbidden");
  const s = await f.api.create(config(["inspect"]));
  sibling = await f.api.create(config());
  foreign = await f.b.create(config());
  assert.deepEqual((await f.b.list()).sessions.map(s => s.id), [foreign.id]);
  assert.deepEqual((await f.b.catalog()).tools, []);
  assert.deepEqual((await f.api.catalog()).tools.map(tool => tool.name), ["inspect"]);
  await rejects(f.b.create(config(["inspect"])), 403, "forbidden");
  for (const action of [
    () => f.b.get(s.id), () => f.b.history(s.id), () => f.b.send(s.id, message("cross tenant")),
    () => f.b.interrupt(s.id), () => f.b.resume(s.id), () => f.b.close(s.id),
    () => f.b.flush(s.id), () => f.b.sync(s.id), () => f.b.restore(s.id, s.checkpoint.id),
    () => f.b.observe(s.id).next(),
  ]) await rejects(action(), 404, "not_found");
  assert.equal((await f.reader.get(s.id)).id, s.id);
  for (const action of [
    () => f.reader.create(config()), () => f.reader.send(s.id, message("read only")),
    () => f.reader.interrupt(s.id), () => f.reader.resume(s.id), () => f.reader.close(s.id),
    () => f.reader.flush(s.id), () => f.reader.restore(s.id, s.checkpoint.id),
    () => f.user.create(config()), () => f.user.close(s.id), () => f.user.resume(s.id),
  ]) await rejects(action(), 403, "forbidden");
  const sent = await f.user.send(s.id, message(call("inspect", { session_id: sibling.id })));
  await state(f.api, s.id);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].session_id, s.id);
  assert.deepEqual(f.model.requests[0].tools.map(tool => tool.function.name), ["inspect"]);
  await rejects(delegated.get(s.id), 401, "forbidden");
  const events = await history(f.api, s.id);
  assert.ok(events.some(event => event.input_id === sent.id && event.type === "tool.result"));
  assert.ok(!JSON.stringify(events).includes(calls[0].inspection_token), "delegated secret leaked into transcript");
  assert.equal(f.model.requests.length, 2);
});

test("external echo, self-inspection, and operator approvals complete through real Pi", budget, async t => {
  const f = await fixture(t, binaries);
  const service = await f.examples();
  await f.start();
  assert.equal((await service.operator("/approvals", undefined, tokens.tool)).status, 401);
  const s = await f.api.create(config(["echo", "inspect_session", "ask_approval"]));
  for (const [name, args, text] of [
    ["echo", { text: "remote echo 世界" }, "remote echo 世界"],
    ["inspect_session", {}, "user.message"],
  ]) {
    const input = await f.api.send(s.id, message(call(name, args)));
    await state(f.api, s.id);
    const events = await history(f.api, s.id);
    const output = toolResults(events).find(event => event.input_id === input.id);
    assert.equal(output.data.result.is_error, undefined);
    assert.ok(output.data.result.content[0].text.includes(text));
    assert.ok(f.model.requests.at(-1).messages.some(m => m.role === "tool" && m.content.includes(text)));
  }
  const stream = await f.observe(s.id, { after: (await history(f.api, s.id)).at(-1).cursor });
  for (const approved of [true, false]) {
    const input = await f.api.send(s.id, message(call("ask_approval", { question: "Proceed with the example?" })));
    const pending = await until(service.pending, values => values.length === 1, "operator approval");
    assert.equal(pending[0].session_id, s.id);
    await stream.wait(event => event.type === "tool.progress" && event.input_id === input.id);
    const count = f.model.requests.length;
    await delay(150);
    assert.equal(f.model.requests.length, count, "waiting for approval polled the model");
    assert.equal((await f.api.get(s.id)).status, "running");
    assert.equal((await fetch(f.url + "/health/ready")).status, 200);
    // One waiting session leaves a free execution slot usable by another user.
    const other = await f.api.create(config());
    await f.api.send(other.id, message("independent conversation"));
    await state(f.api, other.id);
    const decision = await service.operator(`/approvals/${pending[0].call_id}`, { approved });
    assert.equal(decision.status, 204);
    assert.equal((await service.operator(`/approvals/${pending[0].call_id}`, { approved })).status, 404, "answer was accepted twice");
    await stream.wait(event => completed(input, event));
    const events = await history(f.api, s.id);
    const output = toolResults(events).find(event => event.input_id === input.id);
    assert.equal(Boolean(output.data.result.is_error), !approved);
    assert.equal(Boolean(output.data.result.unknown_outcome), false);
    assert.equal(output.data.call_id, pending[0].call_id);
    assert.ok(!events.some(event => event.type === "session.error"), "approval denial became a fatal Brain error");
    assert.ok(f.model.requests.at(-1).messages.some(m => m.role === "tool" && m.content.includes(approved ? "answered yes" : "answered no")));
    assert.deepEqual(await service.pending(), []);
  }
});

test("an observer can disconnect during inference, reconnect, and recover every retained event", budget, async t => {
  const f = await fixture(t, binaries, { limits: { subscribers: 1 } });
  const response = Promise.withResolvers();
  f.model.respond = async (_input, res) => {
    res.writeHead(200, { "Content-Type": "text/event-stream" });
    res.write('data: {"id":"stream","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","content":"partial 🌍"},"finish_reason":null}]}\n\n');
    response.resolve(res);
  };
  await f.start();
  const s = await f.api.create(config());
  const live = await f.observe(s.id);
  await rejects(f.api.observe(s.id).next(), 429, "overloaded");
  const sent = await f.api.send(s.id, message("stream this"));
  await live.wait(event => event.type === "assistant.delta");
  const cursor = retained(live.events).at(-1).cursor;
  await live.stop();
  assert.equal((await f.api.get(s.id)).status, "running");
  const res = await response.promise;
  res.end('data: {"id":"stream","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}\n\ndata: [DONE]\n\n');
  await state(f.api, s.id);
  // Exercise the browser-compatible Last-Event-ID header as well as SDK cursors.
  const controller = new AbortController();
  const replayClient = new Brain(f.url, tokens.a, (target, init) => {
    target.search = "";
    return fetch(target, { ...init, headers: { ...init.headers, "Last-Event-ID": cursor } });
  });
  const replay = [];
  try {
    for await (const event of replayClient.observe(s.id, { signal: controller.signal })) {
      replay.push(event);
      if (completed(sent, event)) break;
    }
  } finally { controller.abort(); }
  assert.deepEqual(ids(replay), ids((await f.api.history(s.id, cursor)).events));
  assert.ok(replay.some(event => event.type === "assistant.message" && event.data.text === "partial 🌍"));
  assert.equal(f.model.requests.length, 1, "observer reconnection caused inference");
  await rejects(f.api.history(s.id, "lost-tail-cursor"), 409, "cursor_unavailable");
  await rejects(f.api.observe(s.id, { after: "lost-tail-cursor" }).next(), 409, "cursor_unavailable");
});

test("full queues reject before acceptance; interrupt and close release waiting tools and execution slots", budget, async t => {
  const f = await fixture(t, binaries, { limits: { active: 1, queue: 1 } });
  const service = await f.examples();
  await f.start();
  const s = await f.api.create(config(["ask_approval"]));
  const waiting = message(call("ask_approval", { question: "Wait for cancellation" }), "waiting");
  const active = await f.api.send(s.id, waiting);
  const pending = (await until(service.pending, list => list.length === 1))[0];
  const queued = await f.api.send(s.id, message("discard queued input", "queued"));
  const rejected = message("retry only after there is capacity", "retryable");
  const raw = await fetch(`${f.url}/sessions/${s.id}/events`, {
    method: "POST", headers: { Authorization: `Bearer ${tokens.a}` }, body: JSON.stringify(rejected),
  });
  assert.equal(raw.status, 429); assert.equal(raw.headers.get("Retry-After"), "1");
  assert.equal((await raw.json()).error.code, "overloaded");
  assert.equal((await f.api.send(s.id, waiting)).id, active.id, "a full queue rejected a previously accepted key");
  await rejects(f.api.send(s.id, { ...message("steering"), delivery: "steer" }), 422, "unsupported");
  await rejects(f.api.resume(s.id), 409, "conflict");
  const other = await f.api.create(config());
  await f.api.send(other.id, message("another session waiting for the active slot"));
  assert.equal(f.model.requests.length, 1, "active limit allowed overlapping inference");
  await f.api.interrupt(s.id);
  await state(f.api, s.id, "interrupted");
  await state(f.api, other.id);
  await until(service.pending, list => list.length === 0, "cancelled approval cleanup");
  assert.equal((await service.operator(`/approvals/${pending.call_id}`, { approved: true })).status, 404);
  const events = await history(f.api, s.id);
  assert.ok(events.some(event => event.type === "session.error" && event.input_id === queued.id));
  assert.ok(!events.some(event => event.type === "user.message" && event.data.key === "retryable"));
  assert.ok(!JSON.stringify(f.model.requests).includes("discard queued input"), "cancelled queued input reached the model");
  await rejects(f.api.send(s.id, rejected), 409, "conflict");
  const count = f.model.requests.length;
  await f.api.resume(s.id);
  assert.equal(f.model.requests.length, count, "resume restarted inference");
  await f.api.send(s.id, rejected);
  await state(f.api, s.id);
  assert.equal((await history(f.api, s.id)).filter(event => event.data.key === "retryable").length, 1);
  assert.equal(f.model.requests.length, count + 1);
  assert.ok(!JSON.stringify(f.model.requests.at(-1)).includes("Wait for cancellation"), "aborted native tail was reused");

  const closeInput = await f.api.send(s.id, message(call("ask_approval", { question: "Close while waiting" })));
  await until(service.pending, list => list.length === 1);
  await f.api.close(s.id);
  await state(f.api, s.id, "closed");
  await until(service.pending, list => list.length === 0);
  await rejects(f.api.resume(s.id), 409, "conflict");
  await rejects(f.api.send(s.id, message("cannot reopen")), 409, "conflict");
  assert.ok((await history(f.api, s.id)).some(event => event.type === "session.error" && event.input_id === closeInput.id));
});

test("tool denial, timeout, disconnect, malformed output, redirects, and size limits remain model-visible", budget, async t => {
  const f = await fixture(t, binaries);
  const calls = [];
  let redirected = 0;
  await f.tool("unreliable", async (req, res) => {
    if (req.url === "/redirected") { redirected++; res.end(); return; }
    const body = await jsonBody(req);
    calls.push(body);
    assert.equal(body.inspection_token, undefined);
    assert.equal(body.brain_url, undefined);
    res.setHeader("Content-Type", "application/json");
    switch (body.arguments.mode) {
      case "denied": res.end(JSON.stringify({ ...result("permission denied by tool"), is_error: true })); break;
      case "http": res.writeHead(503); res.end("service unavailable"); break;
      case "malformed": res.end('{"content":'); break;
      case "oversize": res.end(JSON.stringify(result("x".repeat(2048)))); break;
      case "disconnect": res.destroy(); break;
      case "timeout": res.flushHeaders(); break; // Cancelled by the configured tool deadline.
      case "redirect": res.writeHead(307, { Location: "/redirected" }); res.end(); break;
      case "forged": res.end(JSON.stringify({ ...result("fake control"), status: "closed" })); break;
      case "incomplete_sse": res.writeHead(200, { "Content-Type": "text/event-stream" }); res.end('event: progress\ndata: {"waiting":true}\n\n'); break;
      default: assert.fail("unexpected test tool mode");
    }
  }, { timeout_seconds: 1, max_output_bytes: 1024 });
  await f.start();
  const s = await f.api.create(config(["unreliable"]));
  for (const mode of ["denied", "http", "malformed", "oversize", "disconnect", "timeout", "redirect", "forged", "incomplete_sse"]) {
    await t.test(mode, async () => {
      const count = calls.length;
      const inference = f.model.requests.length;
      const sent = await f.api.send(s.id, message(call("unreliable", { mode })));
      await state(f.api, s.id);
      const events = (await history(f.api, s.id)).filter(event => event.input_id === sent.id);
      const invoke = events.find(event => event.type === "tool.call");
      const outputs = toolResults(events);
      assert.equal(outputs.length, 1);
      const output = outputs[0].data;
      assert.equal(output.call_id, invoke.data.id);
      assert.equal(output.result.is_error, true);
      assert.equal(Boolean(output.result.unknown_outcome), mode !== "denied");
      assert.equal(calls.length, count + 1, "Brain retried a tool invocation");
      assert.equal(calls.at(-1).call_id, invoke.data.id);
      assert.equal(f.model.requests.length, inference + 2);
      assert.ok(f.model.requests.at(-1).messages.some(m => m.role === "tool" && m.content.includes(output.result.content[0].text)));
      assert.ok(events.some(event => event.type === "assistant.message" && event.data.text === "tool handled"));
      assert.ok(!events.some(event => event.type === "session.error"), "tool failure halted the session");
    });
  }
  assert.equal(redirected, 0, "tool credentials followed an HTTP redirect");
});

test("process death preserves local history and deduplication without replaying abandoned tool or queued work", budget, async t => {
  const f = await fixture(t, binaries);
  const calls = [];
  await f.tool("wait", async (req, res) => { calls.push(await jsonBody(req)); res.writeHead(200, { "Content-Type": "text/event-stream" }); res.flushHeaders(); });
  await f.start();
  const s = await f.api.create(config(["wait"]));
  const first = message("remember the completed turn", "saved");
  const saved = await f.api.send(s.id, first);
  const checkpoint = (await state(f.api, s.id)).checkpoint;
  const completedHistory = await history(f.api, s.id);
  const abandoned = message(call("wait", { text: "abandoned tool invocation" }), "abandoned");
  const active = await f.api.send(s.id, abandoned);
  await until(() => calls.length, count => count === 1);
  const queued = await f.api.send(s.id, message("unexecuted queue tail", "queued"));
  const beforeCrash = await history(f.api, s.id);
  await f.crash();
  const file = path.join(f.settings.data_dir, "sessions", s.id, "events.jsonl");
  const bytes = await readFile(file);
  await appendFile(file, '{"id":"torn tail');
  await f.start();
  assert.equal((await f.api.get(s.id)).status, "interrupted");
  assert.deepEqual(ids(await history(f.api, s.id)), ids(beforeCrash), "valid history was lost during torn-tail repair");
  assert.deepEqual(await readFile(file), bytes, "torn tail was not trimmed to the valid prefix");
  assert.equal(f.model.requests.length, 2); assert.equal(calls.length, 1);
  assert.equal((await f.api.send(s.id, first)).id, saved.id, "idempotency did not survive restart");
  assert.equal((await f.api.send(s.id, abandoned)).id, active.id);
  await rejects(f.api.send(s.id, message("requires explicit resume")), 409, "conflict");
  await f.api.resume(s.id);
  assert.equal(f.model.requests.length, 2); assert.equal(calls.length, 1);
  const resumed = await f.api.send(s.id, message("continue after process death"));
  await state(f.api, s.id);
  assert.equal(f.model.requests.length, 3); assert.equal(calls.length, 1, "uncertain tool side effect was replayed");
  const next = f.model.requests.at(-1);
  assert.deepEqual(next.messages.slice(0, f.model.requests[0].messages.length), f.model.requests[0].messages);
  assert.ok(JSON.stringify(next).includes("answer: remember the completed turn"));
  assert.ok(!JSON.stringify(next).includes("abandoned tool invocation"));
  assert.ok(!JSON.stringify(next).includes("unexecuted queue tail"));
  const events = await history(f.api, s.id);
  assert.deepEqual(ids(events.slice(0, completedHistory.length)), ids(completedHistory));
  assert.ok(events.some(event => event.id === queued.id), "abandoned accepted input disappeared from observed history");
  assert.ok(events.some(event => completed(resumed, event)));
  assert.notEqual((await f.api.get(s.id)).checkpoint.id, checkpoint.id);
});

test("a fresh cache restores uploaded history and native context; explicit rollback rejects lost-tail cursors", budget, async t => {
  const f = await fixture(t, binaries, { persistence: {} });
  const calls = [];
  await f.tool("external", async (req, res) => {
    const body = await jsonBody(req); calls.push(body);
    if (body.arguments.wait) { res.writeHead(200, { "Content-Type": "text/event-stream" }); res.flushHeaders(); return; }
    res.setHeader("Content-Type", "application/json"); res.end(JSON.stringify(result("remote side effect completed")));
  });
  await f.start();
  const s = await f.api.create(config(["external"]));
  await f.api.send(s.id, message(call("external", { wait: false })));
  await state(f.api, s.id);
  const savedHistory = await history(f.api, s.id);
  const saved = await f.api.flush(s.id);
  assert.ok(saved.checkpoint); assert.ok(saved.history_bytes >= saved.resume_offset);
  assert.equal(saved.native_checkpoint, (await f.api.get(s.id)).checkpoint.id);
  await f.api.send(s.id, message(call("external", { wait: true, text: "uncompleted native work" })));
  await until(() => calls.length, count => count === 2);
  const queued = await f.api.send(s.id, message("uploaded but never executed"));
  const ahead = await f.api.flush(s.id);
  const aheadHistory = await history(f.api, s.id);
  assert.ok(ahead.history_bytes > saved.history_bytes);
  assert.equal(ahead.resume_offset, saved.resume_offset);
  assert.equal(ahead.native_checkpoint, saved.native_checkpoint);
  await rejects(f.api.restore(s.id, saved.checkpoint), 409, "conflict");
  await f.crash();
  f.settings.data_dir = path.join(f.dir, "empty-cache");
  await f.start();
  await rejects(f.b.get(s.id), 404, "not_found");
  await rejects(f.b.restore(s.id, ahead.checkpoint), 404, "not_found");
  const restored = await f.api.get(s.id); // Local miss falls back to the latest external manifest.
  assert.equal(restored.status, "interrupted");
  assert.deepEqual(restored.config, s.config);
  assert.deepEqual(ids(await history(f.api, s.id)), ids(aheadHistory));
  assert.equal((await f.api.sync(s.id)).native_checkpoint, saved.native_checkpoint);
  assert.equal(f.model.requests.length, 3); assert.equal(calls.length, 2);
  await f.api.resume(s.id);
  await f.api.send(s.id, message("continue from the saved boundary"));
  await state(f.api, s.id);
  const next = f.model.requests.at(-1);
  assert.deepEqual(next.messages.slice(0, f.model.requests[1].messages.length), f.model.requests[1].messages);
  assert.ok(JSON.stringify(next).includes("remote side effect completed"));
  assert.ok(!JSON.stringify(next).includes("uncompleted native work"));
  assert.ok(!JSON.stringify(next).includes("uploaded but never executed"));
  assert.equal(calls.length, 2);
  const stream = await f.observe(s.id);
  await rejects(f.api.restore(s.id, saved.checkpoint), 409, "conflict");
  await stream.stop();
  const rollback = await f.api.restore(s.id, saved.checkpoint);
  assert.equal(rollback.status, "interrupted");
  assert.deepEqual(ids(await history(f.api, s.id)), ids(savedHistory));
  await rejects(f.api.history(s.id, queued.cursor), 409, "cursor_unavailable");
  await rejects(f.api.observe(s.id, { after: queued.cursor }).next(), 409, "cursor_unavailable");
  await f.api.resume(s.id);
  assert.equal(f.model.requests.length, 4); assert.equal(calls.length, 2);
});

test("an archive outage fills the bounded sync queue, stops admission, and recovers without losing accepted input", budget, async t => {
  const f = await fixture(t, binaries, { persistence: { interval_seconds: 1, queue: 1 } });
  await f.start();
  const s = await f.api.create(config());
  await until(() => f.api.sync(s.id), status => !status.pending, "initial synchronization");
  // Replace only the fixture's external archive directory with an ordinary
  // file, making every subsequent storage write fail on Windows and Linux.
  const archive = f.settings.persistence.directory;
  assert.equal(path.dirname(archive), f.dir);
  await rename(archive, archive + "-offline");
  await writeFile(archive, "storage unavailable");
  const input = message("keep this accepted message locally", "accepted-before-outage");
  const accepted = await f.api.send(s.id, input);
  await state(f.api, s.id);
  await until(() => f.api.sync(s.id), status => status.pending && status.error, "failed synchronization");
  assert.equal((await fetch(f.url + "/health/live")).status, 200);
  assert.equal((await fetch(f.url + "/health/ready")).status, 503);
  const retry = message("admit after storage recovery", "retry-after-outage");
  await rejects(f.api.send(s.id, retry), 429, "overloaded");
  await rejects(f.api.create(config()), 429, "overloaded");
  assert.equal((await f.api.send(s.id, input)).id, accepted.id);
  assert.ok((await history(f.api, s.id)).some(event => completed(accepted, event)));
  await rejects(f.api.flush(s.id), 500, "brain_error");
  await unlink(archive);
  await rename(archive + "-offline", archive);
  await f.api.flush(s.id);
  await until(() => f.api.sync(s.id), status => !status.pending && !status.error, "synchronization recovery");
  assert.equal((await fetch(f.url + "/health/ready")).status, 200);
  await f.api.send(s.id, retry);
  await state(f.api, s.id);
  const events = await history(f.api, s.id);
  assert.deepEqual(events.filter(event => event.type === "user.message").map(event => event.data.key), [input.key, retry.key]);
  assert.equal(f.model.requests.length, 2);
});

test("provider failures interrupt; broken harness protocols fail; unrelated sessions keep working", budget, async t => {
  const f = await fixture(t, binaries);
  f.settings.harnesses.broken = { command: [process.execPath, "-e", 'process.stdin.once("data", () => process.stdout.write("invalid protocol\\n"));'] };
  await f.start();
  const broken = await f.api.create({ ...config(), harness: "broken" });
  const input = await f.api.send(broken.id, message("break the protocol"));
  await state(f.api, broken.id, "failed");
  const failure = (await history(f.api, broken.id)).find(event => event.type === "session.error");
  assert.equal(failure.input_id, input.id);
  assert.equal(failure.data.code, "brain_error");
  assert.equal(f.model.requests.length, 0);
  await rejects(f.api.send(broken.id, message("no automatic recovery")), 409, "conflict");

  const s = await f.api.create(config());
  await f.api.send(s.id, message("completed before provider failure"));
  await state(f.api, s.id);
  f.model.respond = async (_input, res) => {
    res.writeHead(401, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ error: { message: "recording provider rejected this request", type: "invalid_api_key" } }));
  };
  const denied = await f.api.send(s.id, message("failed provider request"));
  await state(f.api, s.id, "interrupted");
  const events = await history(f.api, s.id);
  assert.ok(events.some(event => event.type === "session.error" && event.input_id === denied.id && event.data.code === "provider_error"));
  assert.equal(toolResults(events).length, 0, "control failure was disguised as a tool result");
  assert.equal(f.model.requests.length, 2, "non-retryable provider failure was replayed");
  await rejects(f.api.send(s.id, message("resume required")), 409, "conflict");
  f.model.respond = undefined;
  await f.api.resume(s.id);
  assert.equal(f.model.requests.length, 2);
  await f.api.send(s.id, message("recover from provider failure"));
  await state(f.api, s.id);
  assert.ok(!JSON.stringify(f.model.requests.at(-1)).includes("failed provider request"));
  assert.ok(JSON.stringify(f.model.requests.at(-1)).includes("completed before provider failure"));
  assert.ok(f.logs.includes(broken.id));
  assert.ok(f.logs.includes("session execution stopped"));
  for (const secret of [tokens.a, tokens.tool, "local-recording-provider-key"]) assert.ok(!f.logs.includes(secret), "server logs exposed credentials");
});

test("corrupt local history fails explicitly and an operator can recover a verified external checkpoint", budget, async t => {
  const f = await fixture(t, binaries, { persistence: {} });
  await f.start();
  const s = await f.api.create(config());
  await f.api.send(s.id, message("history that must not silently disappear"));
  await state(f.api, s.id);
  const events = await history(f.api, s.id);
  const checkpoint = await f.api.flush(s.id);
  await f.crash();
  const file = path.join(f.settings.data_dir, "sessions", s.id, "events.jsonl");
  const corrupt = await readFile(file);
  corrupt[0] = "!".charCodeAt(0); // Complete damaged record followed by valid records.
  await writeFile(file, corrupt);
  await f.start();
  await rejects(f.api.get(s.id), 500, "brain_error");
  await rejects(f.api.history(s.id), 500, "brain_error");
  await rejects(f.api.send(s.id, message("must not append to corruption")), 500, "brain_error");
  assert.deepEqual(await readFile(file), corrupt, "interior corruption was silently trimmed or replaced");
  assert.equal(f.model.requests.length, 1);
  assert.equal((await f.api.restore(s.id, checkpoint.checkpoint)).status, "interrupted");
  assert.deepEqual(ids(await history(f.api, s.id)), ids(events));
  await f.api.resume(s.id);
  assert.equal(f.model.requests.length, 1);
  await f.api.send(s.id, message("continue after explicit recovery"));
  await state(f.api, s.id);
  assert.ok(JSON.stringify(f.model.requests.at(-1)).includes("answer: history that must not silently disappear"));
});

test("invalid or unsupported requests are rejected without creating sessions or consuming input keys", budget, async t => {
  const f = await fixture(t, binaries);
  await f.start();
  for (const [input, status, code] of [
    [{ ...config(), extra: true }, 400, "invalid_request"],
    [{ ...config(), harness: "not-installed" }, 422, "unsupported"],
    [{ ...config(), config: { ...config().config, model: "missing/model" } }, 400, "invalid_request"],
    [{ ...config(), config: { ...config().config, reasoning_effort: "invalid" } }, 400, "invalid_request"],
    [config(["not-installed"]), 403, "forbidden"],
  ]) await rejects(f.api.create(input), status, code);
  assert.deepEqual((await f.api.list()).sessions, []);
  const s = await f.api.create(config());
  for (const input of [
    message(" ", "valid-later"), { ...message("bad delivery", "valid-later"), delivery: "unknown" },
    { ...message("extra field", "valid-later"), unexpected: true },
    { type: "user.interrupt", text: "not allowed" }, message("x".repeat(4097)),
  ]) await rejects(f.api.send(s.id, input), 400, "invalid_request");
  await rejects(f.api.send(s.id, message("x".repeat(32768))), 413, "invalid_request");
  await rejects(f.api.history(s.id, "", 0), 400, "invalid_request");
  await rejects(f.api.list("", 1001), 400, "invalid_request");
  await rejects(f.api.flush(s.id), 422, "unsupported");
  assert.deepEqual(await history(f.api, s.id), []);
  assert.equal(f.model.requests.length, 0);
  await f.api.send(s.id, message("valid retry", "valid-later"));
  await state(f.api, s.id);
  assert.equal(f.model.requests.length, 1);
  const second = await f.api.create(config());
  const expected = [s.id, second.id].sort();
  const page = await f.api.list("", 1);
  assert.deepEqual(page.sessions.map(s => s.id), expected.slice(0, 1));
  assert.deepEqual((await f.api.list(expected[0], 1)).sessions.map(s => s.id), expected.slice(1));
  assert.deepEqual((await f.api.list(expected[1], 1)).sessions, []);
});

test("an observer exceeding its buffer is disconnected while canonical history and execution survive", budget, async t => {
  const f = await fixture(t, binaries, { limits: { subscriber_bytes: 4096 } });
  const text = "large streamed answer ".repeat(512);
  f.model.respond = async (_input, res) => modelReply(res, text);
  await f.start();
  const s = await f.api.create(config());
  const stream = await f.observe(s.id);
  let disconnected = false;
  stream.done.then(() => { disconnected = true; });
  const sent = await f.api.send(s.id, message("a large answer"));
  await state(f.api, s.id);
  await until(() => disconnected, Boolean, "bounded observer disconnect");
  await stream.stop();
  const events = await history(f.api, s.id);
  assert.ok(events.some(event => event.type === "assistant.message" && event.data.text === text));
  assert.ok(events.some(event => completed(sent, event)));
  assert.equal(f.model.requests.length, 1);
  // A replacement subscriber can replay the retained response from disk.
  const replay = await f.observe(s.id, { after: sent.cursor });
  await replay.wait(event => completed(sent, event));
});

test("Python and TypeScript clients manage the same live conversation and saved checkpoints", budget, async t => {
  const f = await fixture(t, binaries, { persistence: {} });
  const service = await f.examples();
  await f.start();
  const s = await f.api.create(config(["echo", "ask_approval"]));
  await f.api.send(s.id, message("TypeScript started this conversation"));
  await state(f.api, s.id);
  // Python creates another session, streams both, cancels a pending approval,
  // restores a checkpoint, and closes its sessions using only the public SDK.
  const result = await f.python(path.join(root, "tests/journeys/python_journey.py"), { JOURNEY_SESSION: s.id });
  assert.equal((await f.api.get(s.id)).status, "closed");
  assert.equal((await f.api.get(result.created)).status, "closed");
  assert.ok((await history(f.api, s.id)).some(event => event.type === "assistant.message" && event.data.text === "tool handled"));
  await until(service.pending, list => list.length === 0);
  assert.equal(f.model.requests.length, 6);
});

test("concurrent tenant sessions respect the active limit and keep context isolated across runner eviction", budget, async t => {
  const f = await fixture(t, binaries, { limits: { active: 2, loaded: 4 } });
  const calls = [], waiting = [];
  await f.tool("echo", async (req, res) => {
    const body = await jsonBody(req); calls.push(body);
    res.setHeader("Content-Type", "application/json"); res.end(JSON.stringify(result(body.arguments.text)));
  }, { tenants: ["a", "b"] });
  f.model.respond = async (input, res) => { waiting.push({ input, res }); };
  await f.start();
  const sessions = [];
  for (let i = 0; i < 4; i++) {
    const api = i % 2 ? f.b : f.api;
    const tenant = i % 2 ? "b" : "a";
    const marker = `private-conversation-${i}`;
    const input = config(["echo"]); input.config.instructions = marker;
    sessions.push({ api, tenant, marker, ...(await api.create(input)) });
  }
  const accepted = await Promise.all(sessions.map(s => s.api.send(s.id, message(call("echo", { text: s.marker }), "same-key-across-sessions"))));
  assert.equal(new Set(ids(accepted)).size, sessions.length);
  await until(() => waiting.length, count => count === 2, "two active model requests");
  await delay(100);
  assert.equal(f.model.requests.length, 2, "active execution cap was exceeded");
  assert.equal((await fetch(f.url + "/health/ready")).status, 200);
  f.model.respond = undefined;
  for (const { input, res } of waiting) modelReply(res, { name: "echo", args: { text: input.messages[0].content } });
  await Promise.all(sessions.map(s => state(s.api, s.id)));
  assert.equal(calls.length, 4);
  for (const invocation of calls) {
    const s = sessions.find(s => s.id === invocation.session_id);
    assert.ok(s); assert.equal(invocation.tenant_id, s.tenant); assert.equal(invocation.arguments.text, s.marker);
  }
  await Promise.all(sessions.map(s => s.api.send(s.id, message(`continue ${s.marker}`, "next"))));
  await Promise.all(sessions.map(s => state(s.api, s.id)));
  assert.equal(f.model.requests.length, 12);
  assert.equal(calls.length, 4, "evicting a runner replayed its previous tool call");
  for (const request of f.model.requests) {
    const marker = request.messages[0].content;
    assert.ok(sessions.some(s => s.marker === marker));
    for (const other of sessions.filter(s => s.marker !== marker)) assert.ok(!JSON.stringify(request).includes(other.marker), "context crossed a session boundary");
  }
  for (const [api, tenant] of [[f.api, "a"], [f.b, "b"]]) {
    assert.deepEqual((await api.list()).sessions.map(s => s.id).sort(), sessions.filter(s => s.tenant === tenant).map(s => s.id).sort());
  }
});

test("operators receive correlated traces and metrics; exporter failures do not stop conversations", budget, async t => {
  const f = await fixture(t, binaries);
  const exports = [];
  let unavailable = false;
  const collector = await f.listen(async (req, res) => {
    const chunks = []; for await (const chunk of req) chunks.push(chunk);
    const status = unavailable ? 503 : 200;
    exports.push({ path: req.url, body: Buffer.concat(chunks), status });
    res.writeHead(status, { "Content-Type": "application/x-protobuf" }); res.end();
  });
  f.env.OTEL_EXPORTER_OTLP_ENDPOINT = collector.url;
  const traceID = "0123456789abcdef0123456789abcdef";
  const scopes = [];
  await f.tool("traced", async (req, res) => {
    scopes.push(req.headers.traceparent);
    await jsonBody(req);
    res.setHeader("Content-Type", "application/json"); res.end(JSON.stringify(result("traced tool result")));
  });
  await f.start();
  const api = new Brain(f.url, tokens.a, (target, init) => fetch(target, { ...init, headers: { ...init.headers, traceparent: `00-${traceID}-0123456789abcdef-01` } }));
  const s = await api.create(config(["traced"]));
  await api.send(s.id, message(call("traced")));
  await state(api, s.id);
  assert.match(scopes[0], new RegExp(`^00-${traceID}-[a-f0-9]{16}-01$`));
  await until(() => exports, values => values.some(e => e.path === "/v1/traces") && values.some(e => e.path === "/v1/metrics"), "OTLP traces and metrics", 30000);
  const spans = Buffer.concat(exports.filter(e => e.path === "/v1/traces").map(e => e.body));
  assert.ok(spans.includes(Buffer.from(traceID, "hex")));
  assert.ok(spans.includes("session.execute")); assert.ok(spans.includes("tool.call"));
  const metrics = Buffer.concat(exports.filter(e => e.path === "/v1/metrics").map(e => e.body));
  for (const name of ["brain.http.requests", "brain.tool.calls", "brain.transcript.bytes", "brain.executions.active"]) assert.ok(metrics.includes(name), `missing exported metric ${name}`);
  unavailable = true;
  await api.send(s.id, message(call("traced")));
  await state(api, s.id);
  await until(() => exports, values => values.some(e => e.status === 503), "exporter outage");
  await api.send(s.id, message("continue while telemetry is unavailable"));
  await state(api, s.id);
  assert.equal(f.model.requests.length, 5);
  assert.equal(scopes.length, 2);
  assert.ok(!(await history(api, s.id)).some(event => event.type === "session.error"));
});

for (const transport of ["sse", "rpc"]) test(`runtime SDK callbacks over ${transport}: scope, progress, restart, rebind, and no observer execution`, budget, async t => {
  const f = await fixture(t, binaries, { persistence: {} });
  await f.start();
  let calls = 0;
  const tools = [{ name: "lookup", description: "Look up a client value", input_schema: { type: "object" }, output_schema: { type: "object", required: ["count"] },
    async run(args, context) { calls++; assert.equal(context.scope.tenant_id, "a"); assert.notEqual(context.scope.session_id, args.session_id); await context.progress({ ready: true }); return { count: calls }; },
  }];
  const s = await f.api.create({ ...config(), runtime_tools: tools, runtime_transport: transport });
  assert.deepEqual(s.descriptors.map(t => t.name), ["runtime_lookup"]);
  assert.ok(!JSON.stringify(s).includes("async run"));
  const stream = await f.observe(s.id);
  const input = await f.api.send(s.id, message(call("runtime_lookup", { session_id: "spoofed" })));
  await stream.wait(e => completed(input, e));
  assert.equal(calls, 1);
  assert.ok(stream.events.some(e => e.type === "tool.progress"));
  assert.deepEqual((await history(f.api, s.id)).find(e => e.type === "tool.result").data.result.structured_content, { count: 1 });
  await stream.stop();
  await history(f.api, s.id);
  assert.equal(calls, 1, "observer replay executed a callback");
  await f.api.dispose();
  await f.api.send(s.id, message(call("runtime_lookup")));
  await state(f.api, s.id);
  const unavailable = (await history(f.api, s.id)).filter(e => e.type === "tool.result").at(-1).data.result;
  assert.equal(unavailable.is_error, true); assert.ok(!unavailable.unknown_outcome);
  await f.api.flush(s.id);
  await f.crash(); await f.start();
  await f.api.rebind(s.id, tools, transport);
  await f.api.send(s.id, message(call("runtime_lookup")));
  await state(f.api, s.id);
  assert.equal(calls, 2);
  await f.api.close(s.id);
});
