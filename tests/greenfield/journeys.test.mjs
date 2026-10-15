import test from "node:test";
import assert from "node:assert/strict";
import { createServer, request as httpRequest } from "node:http";
import { spawn, execFileSync } from "node:child_process";
import { once } from "node:events";
import { randomUUID, randomBytes, createHash, createHmac } from "node:crypto";
import { mkdir, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { SessionClient, SessionError } from "../../packages/brain-sdk/dist/v2.js";
import { SQLiteReceipts } from "../../packages/brain-sdk/dist/receipts.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const binary = process.env.BRAIN_TEST_BINARY ?? path.join(root, ".build", process.platform === "win32" ? "brain.exe" : "brain");
const delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
const fixtureHarness = (name = "fixture", continuation = true) => ({ name, version: "1", command: process.execPath,
  arguments: [path.join(root, "tests", "fixtures", "decision-harness.mjs")], workers: 2, models: ["fixture"], continuation });

async function runPython(t, name, environment) {
  const child = spawn(process.env.BRAIN_TEST_PYTHON ?? "python", [path.join(root, "tests", "fixtures", name)], {
    cwd: root, env: { ...process.env, PYTHONPATH: path.join(root, "packages", "brain-python"), ...environment },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  child.stdout.on("data", chunk => { output += chunk; });
  child.stderr.on("data", chunk => { output += chunk; });
  t.after(async () => {
    if (child.exitCode === null && child.signalCode === null) { const stopped = once(child, "exit"); child.kill(); await stopped; }
  });
  const [code] = await once(child, "exit");
  assert.equal(code, 0, output);
  return output;
}

async function waitFor(get, predicate, description, timeout = 15000) {
  const end = Date.now() + timeout;
  let value;
  while (Date.now() < end) {
    value = await get();
    if (predicate(value)) return value;
    await delay(40);
  }
  assert.fail(`${description}: ${JSON.stringify(value)}`);
}

async function fixture(t, options = {}) {
  const directory = path.join(root, ".test-output", randomUUID());
  await mkdir(directory, { recursive: true });
  let controlURL, schema, d1;
  const postgres = process.env.BRAIN_TEST_POSTGRES_CONTAINER;
  if (process.env.BRAIN_TEST_POSTGRES_URL) {
    const url = new URL(process.env.BRAIN_TEST_POSTGRES_URL);
    assert.equal(url.hostname, "127.0.0.1", "PostgreSQL journeys require a disposable loopback database");
    assert.match(postgres ?? "", /^[a-f0-9]{64}$/u);
    schema = `journey_${randomUUID().replaceAll("-", "")}`;
    execFileSync("docker", ["exec", postgres, "psql", "-U", "brain", "-d", "brain", "-v", "ON_ERROR_STOP=1", "-q", "-c", `CREATE SCHEMA ${schema}`],
      { cwd: root, stdio: "pipe", timeout: 15000 });
    url.searchParams.set("search_path", schema);
    controlURL = url.toString();
  }
  if (process.env.BRAIN_TEST_CONTROL === "d1") {
    assert.equal(controlURL, undefined, "select only one disposable control profile");
    const { startD1 } = await import("../fixtures/d1.mjs");
    d1 = await startD1(directory);
    controlURL = d1.origin;
  }
  if (options.replicas && !controlURL) controlURL = path.join(directory, "shared-control.sqlite");
  const custody = randomBytes(32).toString("hex");
  const state = { modelCalls: 0, applicationCalls: 0, mode: "success", lastMessages: [],
    modelUsage: { prompt_tokens: 7 }, holdModels: false, pendingModels: [] };
  const model = createServer(async (request, response) => {
    let text = "";
    for await (const chunk of request) text += chunk;
    assert.equal(request.method, "POST");
    assert.equal(request.url, "/v1/chat/completions");
    const body = JSON.parse(text);
    state.modelCalls++;
    state.lastMessages = body.messages;
    state.lastModelOptions = body;
    const call = state.modelCalls;
    const complete = () => {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ id: "fixture-response", choices: [{ message: {
        role: "assistant", content: state.modelContentBytes ? "r".repeat(state.modelContentBytes) : `response ${call}`, tool_calls: [],
      } }], usage: state.modelUsage }));
    };
    if (state.holdModels) state.pendingModels.push(complete);
    else complete();
  });
  model.listen(0, "127.0.0.1");
  await once(model, "listening");
  const application = createServer(async (request, response) => {
    let text = "";
    for await (const chunk of request) text += chunk;
    const signature = `v1=${createHmac("sha256", "fixture-signing-key").update(text).digest("hex")}`;
    assert.equal(request.headers["x-brain-signature"], signature);
    const execution = JSON.parse(text);
    assert.equal(request.headers["x-brain-operation"], execution.operation.id);
    state.applicationCalls++;
    if (state.mode === "lost-response") return request.socket.destroy();
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ request_id: execution.operation.id, outcome: "completed", result: { delivered: state.applicationCalls } }));
  });
  application.listen(0, "127.0.0.1");
  await once(application, "listening");
  let processHandle;
  let origin;
  let log = "";
  const processes = [];
  async function launch(name, primary, extra = {}) {
    const env = {
      PATH: process.env.PATH, SystemRoot: process.env.SystemRoot, USERPROFILE: process.env.USERPROFILE, HOME: process.env.HOME,
      BRAIN_DATA: path.join(directory, primary ? "core" : name), BRAIN_NODE_NAME: name,
      BRAIN_LISTEN: "127.0.0.1:0", BRAIN_API_TOKEN: "fixture-operator",
      BRAIN_DEVELOPMENT: "1", BRAIN_L1_BYTES: "1048576", BRAIN_L2_BYTES: "33554432",
      BRAIN_DIRTY_BYTES: "16777216", BRAIN_L2_SHARDS: "2", BRAIN_WORKERS: "8", BRAIN_LEASE_MS: "3000",
      BRAIN_HARNESSES: JSON.stringify(options.harnesses ?? [fixtureHarness()]),
      BRAIN_MODELS: JSON.stringify({ fixture: { kind: "openai", base_url: `http://127.0.0.1:${model.address().port}/v1/`, credential: "fixture" } }),
      BRAIN_SECRET_REFS: JSON.stringify({ local: { fixture: "FIXTURE_MODEL_KEY", signing: "FIXTURE_SIGNING_KEY" } }),
      FIXTURE_MODEL_KEY: "fixture-not-a-provider-key", FIXTURE_SIGNING_KEY: "fixture-signing-key",
      ...(controlURL ? { BRAIN_CONTROL_URL: controlURL } : {}),
      ...(d1 ? { BRAIN_CONTROL: "d1", BRAIN_D1_KEY: d1.key } : {}),
      ...(options.replicas ? { BRAIN_CUSTODY_KEY: custody, BRAIN_ARCHIVE_DIRECTORY: path.join(directory, "shared-archive") } : {}),
      ...options.environment,
      ...extra,
    };
    const child = spawn(binary, [], { cwd: root, env, stdio: ["ignore", "pipe", "pipe"] });
    processes.push(child);
    let output = "";
    const append = chunk => { output += chunk; if (primary) log = output; };
    child.stdout.on("data", append);
    child.stderr.on("data", append);
    await waitFor(() => output, text => {
      assert.equal(child.exitCode, null, `server exited: ${text}`);
      return /listen=127\.0\.0\.1:\d+/u.test(text);
    }, "server readiness");
    const address = `http://${output.match(/listen=(127\.0\.0\.1:\d+)/u)[1]}`;
    assert.equal((await fetch(`${address}/health/ready`)).status, 204);
    if (primary) { processHandle = child; origin = address; }
    return { origin: address };
  }
  async function start() { await launch("primary", true); }
  async function stop() {
    if (processHandle.exitCode !== null || processHandle.signalCode !== null) return;
    const exited = once(processHandle, "exit");
    processHandle.kill();
    await exited;
  }
  t.after(async () => {
    for (const complete of state.pendingModels.splice(0)) complete();
    for (const child of processes) if (child.exitCode === null && child.signalCode === null) { const done = once(child, "exit"); child.kill(); await done; }
    await Promise.all([new Promise(resolve => model.close(resolve)), new Promise(resolve => application.close(resolve))]);
    if (schema) execFileSync("docker", ["exec", postgres, "psql", "-U", "brain", "-d", "brain", "-v", "ON_ERROR_STOP=1", "-q", "-c", `DROP SCHEMA ${schema} CASCADE`],
      { cwd: root, stdio: "pipe", timeout: 15000 });
    await d1?.dispose();
    await rm(directory, { recursive: true, force: true });
  });
  await start();
  async function api(method, url, body, { status = 200, token = "fixture-operator" } = {}) {
    const response = await fetch(origin + url, { method, headers: { Authorization: `Bearer ${token}`,
      ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await response.text();
    assert.equal(response.status, status, `${method} ${url}: ${text}\n${log}`);
    return text ? JSON.parse(text) : undefined;
  }
  function config(overrides = {}) {
    return { model: { provider: "fixture", name: "fixture-model" }, harness: { name: "fixture", version: "1" },
      actions: [], connections: [], events: [], grants: ["session:read", "input:write"],
      limits: { calls: 100, concurrent: 8, input_bytes: 131072, output_bytes: 65536, retained_bytes: 16777216, pending_inputs: 128 },
      ...overrides };
  }
  async function create(overrides = {}, message) {
    return api("POST", "/v2/sessions", { request_id: randomUUID(), config: config(overrides), ...(message === undefined ? {} : { message }) });
  }
  async function settled(id, expected = "completed", count = 1) {
    return waitFor(() => api("GET", `/v2/sessions/${id}/runs`), runs => runs.length === count && runs.every(run => run.status === expected), `runs settle as ${expected}`);
  }
  return { api, create, config, state, settled, start, stop, directory, get origin() { return origin; },
    replica(name = "replica", environment = {}) {
      assert.equal(options.replicas, true, "replicas require shared control, custody and archive admission");
      return launch(name, false, environment);
    },
    releaseModels() { state.holdModels = false; for (const complete of state.pendingModels.splice(0)) complete(); },
    get applicationURL() { return `http://127.0.0.1:${application.address().port}`; } };
}

test("J17/J20/J24: admitted receipts survive dirty pressure and oversized metadata without repeating effects", { timeout: 60000 }, async t => {
  const f = await fixture(t, { environment: {
    BRAIN_L2_SHARDS: "1", BRAIN_L2_BYTES: "16777216", BRAIN_DIRTY_BYTES: "1048576", BRAIN_ARCHIVE_FLUSH_MS: "60000",
  } });
  const view = await f.create({
    connections: [{ name: "worker", kind: "application", provider: "application", ownership: "borrowed" }],
    actions: [{ name: "deliver", description: "Deliver once", connection: "worker", input_schema: { type: "object" },
      max_output_bytes: 8192, timeout_ms: 60000 }],
    events: [{ name: "padding", version: "1", schema: { type: "string" }, disposition: "application", sources: ["operator"] }],
  });
  let session = new SessionClient({ url: f.origin, token: "fixture-operator" }).session(view.id);
  const connection = await session.attach("worker", { requestId: "worker" });
  await session.send("fixture:action:deliver", "original");
  const [operation] = await waitFor(() => session.operations(), values => values.length === 1 && values[0].status === "prepared", "one admitted operation");
  await waitFor(() => session.runs(), values => values[0]?.status === "waiting", "original run waits");
  const execution = await session.claim(operation.id, { request_id: "claim", connection_id: connection.id, generation: connection.generation });
  const body = JSON.stringify(execution);
  const response = await fetch(f.applicationURL, { method: "POST", headers: { "Content-Type": "application/json",
    "X-Brain-Operation": operation.id, "X-Brain-Signature": "v1=" + createHmac("sha256", "fixture-signing-key").update(body).digest("hex") }, body });
  assert.equal(response.status, 200);
  const effect = await response.json();
  assert.equal(f.state.applicationCalls, 1);
  const padding = [];
  for (let index = 0; index < 6; index++) {
    padding.push(await f.api("POST", `/v2/sessions/${view.id}/inputs`, {
      request_id: `padding-${index}`, data: { kind: "event", name: "padding", version: "1", data: "p".repeat(100000) },
    }));
  }
  await waitFor(() => Promise.all(padding.map(command => session.command(command.id))), values =>
    values.filter(value => value.status === "applied").length === 2 && values.some(value => value.status === "accepted"),
  "ordinary writes stop before spending the admitted output reserve");
  const position = (await session.read()).sequence;
  await delay(500);
  assert.equal((await session.read()).sequence, position, "persistent pressure cannot allocate endless unused positions");
  const before = await f.api("GET", "/operator/storage");
  assert(before.reserved_output_bytes > 0);
  assert(before.unarchived_bytes < 1048576);
  const provider = { details: "u".repeat(900000) };
  const receipt = { request_id: "original-result", outcome: "completed",
    result: { ...effect.result, payload: "r".repeat(6000) }, usage: { input_tokens: 9, provider } };
  const completed = await session.receipt(operation.id, receipt, execution.receipt_token);
  assert.equal(completed.status, "completed");
  assert.equal(completed.result_available, true);
  assert.equal(completed.result.payload.length, 6000);
  assert.equal(completed.usage.input_tokens, 9);
  assert.equal(Object.hasOwn(completed.usage, "output_tokens"), false);
  assert.deepEqual(completed.usage.provider, {});
  assert.deepEqual(completed.usage.provider_unavailable, {
    digest: createHash("sha256").update(JSON.stringify(provider)).digest("hex"), bytes: Buffer.byteLength(JSON.stringify(provider)),
  });
  await f.settled(view.id);
  await waitFor(() => session.budget(), value => value.reserved_output_bytes === 0, "written result releases its control reservation");
  const after = await f.api("GET", "/operator/storage");
  assert.equal(after.reserved_output_bytes, 0);
  assert(after.unarchived_bytes < 1048576);
  assert((await Promise.all(padding.map(command => session.command(command.id)))).some(command => command.status === "accepted"),
    "receipt completion was not obtained by dropping accepted ordinary work");
  await runPython(t, "receipt-metadata.py", { BRAIN_URL: f.origin, BRAIN_SESSION: view.id, BRAIN_OPERATION: operation.id });
  await f.stop();
  await f.start();
  session = new SessionClient({ url: f.origin, token: "fixture-operator" }).session(view.id);
  await session.receipt(operation.id, receipt, execution.receipt_token);
  assert.equal((await session.operation(operation.id)).status, "completed");
  assert.equal((await session.runs())[0].status, "completed");
  assert.equal(f.state.applicationCalls, 1);
  assert.equal(f.state.modelCalls, 0);
});

test("J21/J23: planned ownership transfer drains one live model, verifies archive and preserves queued work", { timeout: 60000 }, async t => {
  const f = await fixture(t, { replicas: true });
  f.state.holdModels = true;
  const view = await f.create({}, "original work follows its checkpoint");
  const client = new SessionClient({ url: f.origin, token: "fixture-operator" });
  const session = client.session(view.id);
  await waitFor(() => session.runs(), runs => runs.length === 1 && runs[0].wait?.kind === "operation", "original model checkpoint");
  const [operation] = await session.operations();
  assert.equal(operation.status, "dispatched");
  await session.putDocument("memory", "retained", { request_id: "memory", revision: "0", value: "unchanged across transfer" });
  const before = await session.records();
  const target = await f.replica();
  const targetClient = new SessionClient({ url: target.origin, token: "fixture-operator" });
  const targetNode = (await targetClient.nodes()).find(node => node.id.startsWith("replica-"));
  assert(targetNode);
  const request = { request_id: "planned-move", owner_epoch: (await session.read()).owner_epoch, target_node: targetNode.id };
  const transfer = await session.transferOwnership(request);
  assert.equal(transfer.phase, "draining");
  const queued = await session.send("explicit work accepted while transfer drains", "queued-during-transfer");
  await delay(300);
  assert.equal((await session.command(queued.id)).status, "accepted");
  assert.equal((await session.ownershipTransfer(transfer.id)).phase, "draining");
  assert.equal(f.state.modelCalls, 1);
  f.releaseModels();
  const moved = await waitFor(() => session.ownershipTransfer(transfer.id), value => {
    assert.notEqual(value.phase, "failed", JSON.stringify(value));
    return value.phase === "transferred";
  }, "target verifies the archive before receiving ownership", 25000);
  assert(moved.checkpoint);
  assert.equal((await fetch(f.origin + "/health/ready")).status, 204, "old owner is still live during the CAS handoff");
  await f.stop();
  const resumed = targetClient.session(view.id);
  await waitFor(() => resumed.runs(), runs => runs.length === 2 && runs.every(run => run.status === "completed"), "original and queued work settle on the successor");
  const current = await resumed.read();
  assert.equal(current.id, view.id);
  assert.equal(BigInt(current.owner_epoch), BigInt(request.owner_epoch) + 1n);
  assert.equal((await resumed.operation(operation.id)).status, "completed");
  assert.equal((await resumed.documents("memory"))[0].value, "unchanged across transfer");
  const after = await resumed.records();
  assert.equal(after.gaps.length, 0, "target reads verified bytes without the original node");
  for (const record of before.records) assert.deepEqual(after.records.find(value => value.sequence === record.sequence), record);
  assert.equal((await resumed.transferOwnership(request)).phase, "transferred");
  assert.equal((await resumed.read()).owner_epoch, current.owner_epoch);
  await assert.rejects(() => resumed.cancelTransfer(transfer.id), error => error instanceof SessionError && error.code === "conflict");
  await runPython(t, "transfer-client.py", { BRAIN_URL: target.origin, BRAIN_TOKEN: "fixture-operator", BRAIN_SESSION: view.id,
    BRAIN_TRANSFER: transfer.id });
  assert.equal(f.state.modelCalls, 2, "the second model request belongs only to explicit queued work");
});

test("J23/J24: unsupported harness continuation stays blocked while original late evidence is retained", { timeout: 60000 }, async t => {
  const f = await fixture(t, { harnesses: [fixtureHarness("fixture", false)] });
  const view = await f.create({
    connections: [{ name: "worker", kind: "application", provider: "application", ownership: "borrowed" }],
    actions: [{ name: "deliver", description: "Deliver once", connection: "worker", input_schema: { type: "object" },
      max_output_bytes: 8192, timeout_ms: 5000 }],
  });
  let session = new SessionClient({ url: f.origin, token: "fixture-operator" }).session(view.id);
  const connection = await session.attach("worker", { requestId: "worker" });
  await session.send("fixture:action:deliver", "one-admitted-decision");
  const [operation] = await waitFor(() => session.operations(), values => values.length === 1 && values[0].status === "prepared", "harness prepares its single application effect");
  await waitFor(() => session.runs(), values => values.length === 1 && values[0].status === "waiting", "original checkpoint waits");
  const execution = await session.claim(operation.id, { request_id: "claim", connection_id: connection.id, generation: connection.generation });
  const body = JSON.stringify(execution);
  const response = await fetch(f.applicationURL, { method: "POST", headers: { "Content-Type": "application/json",
    "X-Brain-Operation": execution.operation.id, "X-Brain-Signature": "v1=" + createHmac("sha256", "fixture-signing-key").update(body).digest("hex") }, body });
  const receipt = await response.json();
  assert.equal(f.state.applicationCalls, 1);
  await f.stop();
  await f.start();
  session = new SessionClient({ url: f.origin, token: "fixture-operator" }).session(view.id);
  const [blocked] = await f.settled(view.id, "blocked");
  assert.equal(blocked.error.code, "unsupported");
  assert.equal((await session.operation(operation.id)).status, "unknown", "the original sender never confirmed its result");
  await session.receipt(operation.id, receipt, execution.receipt_token);
  assert.equal((await session.operation(operation.id)).status, "completed");
  assert.equal((await session.runs())[0].status, "blocked");
  const fresh = await f.create({}, "explicit new work can use a new harness process");
  await f.settled(fresh.id);
  await session.control("resume", "cannot-resume-old-process");
  const [stillBlocked] = await f.settled(view.id, "blocked");
  assert.equal(stillBlocked.error.code, "unsupported");
  assert.equal(f.state.applicationCalls, 1);
  assert.equal(f.state.modelCalls, 1, "only explicit fresh work used a model");
});

test("J15: same-session harness handoff is atomic, quiescent and idempotent without replacing history", { timeout: 45000 }, async t => {
  const f = await fixture(t, { harnesses: [fixtureHarness(), fixtureHarness("specialist")] });
  const original = { name: "fixture", version: "1" };
  const specialist = { name: "specialist", version: "1" };
  const view = await f.create({ harness_transitions: [original, specialist] }, "before handoff");
  const session = new SessionClient({ url: f.origin, token: "fixture-operator" }).session(view.id);
  await f.settled(view.id);
  const request = { request_id: "to-specialist", revision: (await session.read()).revision, harness: specialist };
  const moved = await session.handoff(request);
  assert.equal(moved.id, view.id);
  assert.equal(moved.config.harness.name, "specialist");
  assert.equal((await session.handoff(request)).revision, moved.revision);
  await session.send("after handoff");
  await f.settled(view.id, "completed", 2);
  assert(f.state.lastMessages.some(message => message.role === "assistant" && message.content === "response 1"));
  await session.control("pause", "pause-new-work");
  await session.send("pending work must retain its old harness");
  const back = { request_id: "back", revision: (await session.read()).revision, harness: original };
  await assert.rejects(() => session.handoff(back), error => error instanceof SessionError && error.code === "conflict");
  await session.control("cancel", "cancel-pending-before-handoff");
  assert.equal((await session.handoff(back)).config.harness.name, "fixture");
  await session.control("resume", "ready-for-new-work");
  const current = await session.read();
  const replay = await session.handoff(request);
  assert.equal(replay.config.harness.name, "fixture");
  assert.equal(replay.revision, current.revision, "old request cannot undo a newer handoff");
  await waitFor(() => session.records(), page => page.records.filter(record => record.kind === "harness.handed_off").length === 2, "both atomic handoff facts are journalled once");
  assert.equal(f.state.modelCalls, 2);
});

test("J15/J23: external harness fork joins exact partial child results and retains references through restart", { timeout: 60000 }, async t => {
  const f = await fixture(t);
  f.state.holdModels = true;
  f.state.modelUsage = { prompt_tokens: 7, completion_tokens: 3 };
  const view = await f.create({}, "fixture:fork:alpha,beta,gamma");
  let client = new SessionClient({ url: f.origin, token: "fixture-operator" });
  let parent = client.session(view.id);
  await waitFor(() => f.state.modelCalls, count => count === 3, "all three fork children execute simultaneously");
  assert.equal(f.state.pendingModels.length, 3);
  const [run] = await waitFor(() => parent.runs(), values => values.length === 1 && values[0].wait?.kind === "children", "parent waits for its saved fork");
  const pending = await parent.children(run.id);
  assert.equal(pending.ready, false);
  assert.equal(pending.expected, 3);
  assert.deepEqual(pending.children.map(child => child.index), [0, 1, 2]);
  await client.session(pending.children[1].session_id).control("cancel", "cancel-middle-child");
  f.releaseModels();
  const [completed] = await f.settled(view.id);
  assert(completed.completion, "completion is bound to a durable record reference");
  const joined = await parent.children(run.id);
  assert.equal(joined.ready, true);
  assert.deepEqual(joined.children.map(child => child.status), ["completed", "cancelled", "completed"]);
  for (const child of joined.children.filter(child => child.status === "completed")) {
    assert.equal(child.availability, "inline");
    assert.equal(child.result.role, "assistant");
    const record = await client.session(child.session_id).records(String(BigInt(child.completion.sequence) - 1n), 1);
    assert.deepEqual(record.records[0].data.result, child.result, "join consumes the actual original child answer, not its SessionView");
    assert.equal(record.records[0].run_id, child.run_id);
  }
  const parentRecord = await parent.records(String(BigInt(completed.completion.sequence) - 1n), 1);
  assert.deepEqual(parentRecord.records[0].data.result.children, joined.children);
  await client.session(joined.children[0].session_id).send("unrelated later work cannot replace its original answer");
  await waitFor(() => client.session(joined.children[0].session_id).runs(), values => values.some(value => value.status === "blocked"), "local child budget denies additional work");
  assert.deepEqual(await parent.children(run.id), joined);
  await f.stop();
  await f.start();
  client = new SessionClient({ url: f.origin, token: "fixture-operator" });
  parent = client.session(view.id);
  assert.deepEqual(await parent.children(run.id), joined);
  assert.deepEqual((await parent.runs())[0].completion, completed.completion);
  await runPython(t, "children-client.py", { BRAIN_TEST_URL: f.origin, BRAIN_TEST_SESSION: view.id,
    BRAIN_TEST_RUN: run.id, BRAIN_EXPECTED_CHILD_STATUSES: "completed,cancelled,completed", BRAIN_EXPECTED_INLINE: "2" });
  assert.equal(f.state.modelCalls, 3, "restart and inspection never repeat cancelled or completed child effects");
  const single = await f.create({}, "fixture:delegate:delta");
  const [singleRun] = await f.settled(single.id);
  const one = await client.session(single.id).children(singleRun.id);
  assert.equal(one.expected, 1);
  assert.equal(one.children[0].result.content, "response 4");
  assert.equal(f.state.modelCalls, 4, "single delegation uses the same result-bearing protocol without a parent model effect");
});

test("J15/J18: large child answers remain readable by reference without an unbounded join payload", { timeout: 45000 }, async t => {
  const f = await fixture(t);
  f.state.modelContentBytes = 60000;
  f.state.modelUsage = { prompt_tokens: 7, completion_tokens: 16000 };
  const config = f.config({ model: { provider: "fixture", name: "fixture-model", native: { max_completion_tokens: 32768 } } });
  config.limits.output_bytes = 131072;
  const view = await f.create(config, "fixture:fork:large-alpha,large-beta,large-gamma");
  const [run] = await f.settled(view.id);
  const client = new SessionClient({ url: f.origin, token: "fixture-operator" });
  const parent = client.session(view.id);
  const result = await parent.children(run.id);
  assert.equal(result.ready, true);
  assert.deepEqual(result.children.map(child => child.status), ["completed", "completed", "completed"],
    JSON.stringify(result.children.map(({ status, error }) => ({ status, error }))));
  assert.deepEqual(result.children.map(child => child.availability), ["inline", "inline", "referenced"]);
  assert(Buffer.byteLength(JSON.stringify(result)) < 132000);
  const referenced = result.children[2];
  assert.equal(Object.hasOwn(referenced, "result"), false);
  const grant = await parent.grant({ request_id: "parent-reader", subject: "reader", permissions: ["session:read"], expires_ms: Date.now() + 60000 });
  const reader = new SessionClient({ url: f.origin, token: grant.token });
  const record = await reader.session(view.id).childResult(run.id, 2);
  assert.equal(record.sequence, referenced.completion.sequence);
  assert.equal(record.data.result.content.length, 60000);
  await assert.rejects(() => reader.session(referenced.session_id).records(), error => error instanceof SessionError && error.code === "forbidden");
  await assert.rejects(() => reader.session(view.id).childResult("unrelated-run", 0), error => error instanceof SessionError && error.code === "not_found");
  await runPython(t, "children-client.py", { BRAIN_TEST_URL: f.origin, BRAIN_TEST_SESSION: view.id,
    BRAIN_TEST_RUN: run.id, BRAIN_TEST_TOKEN: grant.token, BRAIN_EXPECTED_CHILD_STATUSES: "completed,completed,completed", BRAIN_EXPECTED_INLINE: "2" });
  assert.equal(f.state.modelCalls, 3);
});

test("J15/J17: unowned-parent delegation journals atomically and parallel children cannot amplify shared budgets", { timeout: 60000 }, async t => {
  const f = await fixture(t);
  f.state.holdModels = true;
  f.state.modelUsage = { prompt_tokens: 7, completion_tokens: 3 };
  const config = f.config({ model: { provider: "fixture", name: "fixture-model", native: { max_completion_tokens: 64 } } });
  Object.assign(config.limits, { calls: 3, concurrent: 2, tokens: 100000 });
  const parent = await f.create(config);
  assert.equal(parent.owner_epoch, "0", "parent has no active owner before delegation");
  const client = new SessionClient({ url: f.origin, token: "fixture-operator" });
  const parentSession = client.session(parent.id);
  const childConfig = structuredClone(parent.config);
  Object.assign(childConfig.limits, { calls: 1, concurrent: 1, tokens: 6000 });
  const admit = async request => {
    for (let attempt = 0; attempt < 100; attempt++) {
      try { return await parentSession.delegate(request); }
      catch (error) {
        if (!(error instanceof SessionError) || !["conflict", "overloaded"].includes(error.code)) throw error;
        await delay(20);
      }
    }
    assert.fail("explicit unchanged-ID delegation did not converge after known rollbacks");
  };
  const requests = ["alpha", "beta", "gamma"].map(message => ({ request_id: randomUUID(), message, config: childConfig, context: [] }));
  const [a, b] = await Promise.all(requests.slice(0, 2).map(admit));
  await waitFor(() => f.state.modelCalls, count => count === 2, "two children execute concurrently");
  assert.equal(f.state.pendingModels.length, 2);
  const c = await admit(requests[2]);
  await delay(300);
  assert.equal(f.state.modelCalls, 2, "shared concurrency excludes a third external effect");
  assert.equal((await client.session(c.id).budget()).calls, 0);
  assert.equal((await client.session(c.id).records()).records.filter(record => record.kind === "operation.intent").length, 0,
    "waiting for a budget does not generate repeated dispatch intents");
  for (const change of [
    value => { value.limits.calls = 4; },
    value => { delete value.limits.tokens; },
    value => { value.model.name = "another-model"; },
    value => { value.harness.configuration = { extraAuthority: true }; },
    value => { value.grants.push("arbitrary:authority"); },
    value => { (value.events ??= []).push({ name: "new.event", version: "1", schema: {}, disposition: "wake", sources: [] }); },
  ]) {
    const attempted = structuredClone(childConfig);
    change(attempted);
    await assert.rejects(() => parentSession.delegate({ request_id: randomUUID(), message: "denied", config: attempted, context: [] }),
      error => error instanceof SessionError && error.code === "forbidden");
  }
  f.releaseModels();
  await Promise.all([a, b, c].map(child => f.settled(child.id)));
  assert.equal(f.state.modelCalls, 3);
  const budget = await parentSession.budget();
  assert.equal(budget.calls, 3);
  assert.equal(budget.active_operations, 0);
  assert.equal(budget.reported_tokens, "30");
  assert.equal(budget.reserved_tokens, "0");
  assert.equal(budget.remaining_calls, 0);
  assert.equal(f.state.lastModelOptions.max_completion_tokens, 64, "SDK request applies the admitted model output bound");
  await client.session(a.id).send("cannot obtain another child allowance");
  await waitFor(() => client.session(a.id).runs(), runs => runs.length === 2 && runs.some(run => run.status === "blocked"), "exhausted child is inspectably blocked");
  assert.equal(f.state.modelCalls, 3);
  assert.equal((await admit(requests[0])).id, a.id, "same request reconciles to the original child");
  await waitFor(() => parentSession.records(), page => page.records.filter(record => record.kind === "session.delegated").length === 3,
    "each child has one durable parent linkage");
  await f.stop();
  await f.start();
  const restarted = new SessionClient({ url: f.origin, token: "fixture-operator" }).session(parent.id);
  assert.equal((await restarted.budget()).reported_tokens, "30");
  await delay(300);
  assert.equal(f.state.modelCalls, 3, "restart does not regenerate the delegated decisions");
});

test("J11/J15: parent pause and cancellation fence child dispatch while original late receipts remain evidence", { timeout: 60000 }, async t => {
  const f = await fixture(t);
  const view = await f.create({ connections: [{ name: "worker", kind: "application", provider: "application", ownership: "borrowed" }],
    actions: [{ name: "deliver", description: "Deliver once", connection: "worker", input_schema: { type: "object" },
      max_output_bytes: 8192, timeout_ms: 30000 }] });
  const client = new SessionClient({ url: f.origin, token: "fixture-operator" });
  const parent = client.session(view.id);
  const knownRollback = async action => {
    for (let attempt = 0; attempt < 100; attempt++) {
      try { return await action(); }
      catch (error) {
        if (!(error instanceof SessionError) || !["conflict", "overloaded"].includes(error.code)) throw error;
        await delay(20);
      }
    }
    assert.fail("known-rollback reconciliation did not converge");
  };
  const delegate = async name => {
    const request = { request_id: name, config: view.config, message: name, context: [] };
    return knownRollback(() => parent.delegate(request));
  };
  const a = await delegate("child-alpha");
  const b = await delegate("child-beta");
  await Promise.all([a, b].map(child => f.settled(child.id)));
  const children = [client.session(a.id), client.session(b.id)];
  const connections = [];
  const operations = [];
  for (const child of children) {
    connections.push(await knownRollback(() => child.attach("worker", { requestId: "attach" })));
    await knownRollback(() => child.invoke("deliver", {}, "deliver"));
    operations.push(await waitFor(() => child.operations(), values => values.some(value => value.action === "deliver" && value.status === "prepared"),
      "child application operation is prepared").then(values => values.find(value => value.action === "deliver")));
  }
  const claim = index => children[index].claim(operations[index].id, { request_id: "claim", connection_id: connections[index].id, generation: connections[index].generation });
  await knownRollback(() => parent.control("pause", "pause"));
  await assert.rejects(() => claim(0), error => error instanceof SessionError && error.code === "conflict");
  assert.equal((await children[0].budget()).available_operations, 0);
  assert.equal(f.state.applicationCalls, 0);
  await knownRollback(() => parent.control("resume", "resume"));
  const execution = await knownRollback(() => claim(0));
  const body = JSON.stringify(execution);
  const performed = await fetch(f.applicationURL, { method: "POST", headers: { "Content-Type": "application/json",
    "X-Brain-Operation": execution.operation.id,
    "X-Brain-Signature": "v1=" + createHmac("sha256", "fixture-signing-key").update(body).digest("hex"),
  }, body });
  assert.equal(performed.status, 200);
  const originalReceipt = await performed.json();
  assert.equal(f.state.applicationCalls, 1);
  await knownRollback(() => parent.control("cancel", "cancel-once"));
  await assert.rejects(() => claim(1), error => error instanceof SessionError && error.code === "conflict");
  for (const child of children) await waitFor(() => child.read(), value => value.lifecycle === "closed", "bounded descendant cancellation closes its child");
  await children[0].receipt(execution.operation.id, originalReceipt, execution.receipt_token);
  const completed = await children[0].operation(execution.operation.id);
  assert.equal(completed.status, "completed");
  assert.equal(completed.result.delivered, 1);
  for (const child of children) {
    const runs = await child.runs();
    assert.equal(runs.filter(run => run.status === "cancelled").length, 1);
  }
  await waitFor(() => children[1].operation(operations[1].id), operation => operation.status === "failed", "undispatched child reservation settles without a send");
  assert.equal(f.state.applicationCalls, 1);
  const fresh = await delegate("explicit-new-child");
  await f.settled(fresh.id);
  await knownRollback(() => parent.control("cancel", "cancel-once"));
  await delay(550);
  assert.equal((await client.session(fresh.id).read()).lifecycle, "open");
  assert.equal(f.state.modelCalls, 3);
  assert.equal(f.state.applicationCalls, 1);
});

test("J17: token estimates deny before send and missing usage remains held across restart in both SDKs", { timeout: 45000 }, async t => {
  const f = await fixture(t);
  const config = f.config({ model: { provider: "fixture", name: "fixture-model", native: { max_completion_tokens: 32 } } });
  config.limits.tokens = 1;
  const denied = await f.create(config, "must not be sent");
  await f.settled(denied.id, "blocked");
  assert.equal(f.state.modelCalls, 0);
  const client = new SessionClient({ url: f.origin, token: "fixture-operator" });
  assert.equal((await client.session(denied.id).budget()).calls, 0);
  config.limits.tokens = 8192;
  const admitted = await f.create(config, "one admitted request with a missing output counter");
  await f.settled(admitted.id);
  const session = client.session(admitted.id);
  const operation = (await session.operations())[0];
  assert.equal(operation.usage.input_tokens, 7);
  assert.equal(Object.hasOwn(operation.usage, "output_tokens"), false);
  const before = await session.budget();
  assert.equal(before.reported_tokens, "7");
  assert.equal(before.incomplete_usages, 1);
  assert.equal(before.token_usage_pending, true);
  assert(BigInt(before.reserved_tokens) > 0n);
  assert.equal(before.available_tokens, String(8192n - 7n - BigInt(before.reserved_tokens)));
  await session.send("x".repeat(7000));
  await delay(300);
  assert.equal(f.state.modelCalls, 1, "incomplete usage does not become a fresh allowance");
  assert.equal((await session.operations()).length, 1);
  const intentCount = (await session.records()).records.filter(record => record.kind === "operation.intent").length;
  await delay(250);
  assert.equal((await session.records()).records.filter(record => record.kind === "operation.intent").length, intentCount);
  await f.stop();
  await f.start();
  await runPython(t, "budget-client.py", { BRAIN_TEST_URL: f.origin, BRAIN_TEST_SESSION: admitted.id,
    BRAIN_EXPECTED_HELD: before.reserved_tokens });
  assert.equal(f.state.modelCalls, 1);
  f.state.modelUsage = { prompt_tokens: 7, completion_tokens: 0 };
  const complete = await f.create(config, "explicit zero output usage");
  await f.settled(complete.id);
  const released = await new SessionClient({ url: f.origin, token: "fixture-operator" }).session(complete.id).budget();
  assert.equal(released.reserved_tokens, "0");
  assert.equal(released.reported_tokens, "7");
  assert.equal(released.token_usage_pending, false);
});

test("J03/J16: admission rejects authority replacement, spoofed sources and cross-session access before effects", async t => {
  const f = await fixture(t);
  for (const native of [{ model: "replaced" }, { tools: [] }, { max_retries: 2 }, { extra_headers: {} }]) {
    await f.api("POST", "/v2/sessions", { request_id: randomUUID(), config: f.config({
      model: { provider: "fixture", name: "fixture-model", native },
    }) }, { status: 400 });
  }
  await f.api("POST", "/v2/sessions", { request_id: randomUUID(), config: f.config({
    model: { provider: "fixture", name: "fixture-model", native: { background: true } },
  }) }, { status: 422 });
  const session = await f.create({ events: [{ name: "operator.event", version: "1", schema: { type: "object" },
    disposition: "state", sources: ["operator"] }] });
  const other = await f.create();
  for (const subject of ["operator", "kernel", "creator", "timer", "local", "harness:fixture", "x".repeat(129)]) {
    await f.api("POST", `/v2/sessions/${session.id}/participants`,
      { request_id: randomUUID(), subject, permissions: ["input:write"], expires_ms: Date.now() + 60000 }, { status: 400 });
  }
  const permissions = ["input:write", "session:read", "session:export", "file:read", "participant:grant", "participant:revoke"];
  const grant = await f.api("POST", `/v2/sessions/${session.id}/participants`,
    { request_id: randomUUID(), subject: "browser", permissions, expires_ms: Date.now() + 60000 });
  await f.api("POST", `/v2/sessions/${session.id}/participants`,
    { request_id: randomUUID(), subject: "another-browser", permissions, expires_ms: Date.now() + 60000 }, { token: grant.token, status: 403 });
  await f.api("POST", `/v2/sessions/${session.id}/participants`,
    { request_id: randomUUID(), subject: "browser", permissions: ["*"], expires_ms: Date.now() + 60000 }, { token: grant.token, status: 403 });
  await f.api("POST", `/v2/sessions/${session.id}/inputs`,
    { request_id: randomUUID(), data: { kind: "event", name: "operator.event", version: "1", data: {} } }, { token: grant.token, status: 403 });
  for (const suffix of ["", "/records", "/events", "/runs", "/budget", "/operations", "/interactions", "/connections", "/state", "/memory", "/schedules", "/files", "/export"]) {
    await f.api("GET", `/v2/sessions/${other.id}${suffix}`, undefined, { token: grant.token, status: 403 });
  }
  assert.equal((await fetch(`${f.origin}/v2/sessions/${session.id}`, {
    headers: { Authorization: "Bearer fixture-operator", "X-Brain-Tenant": "another-tenant" },
  })).status, 404);
  await f.api("DELETE", `/v2/sessions/${session.id}/participants/${grant.id}`, undefined, { token: grant.token });
  await f.api("GET", `/v2/sessions/${session.id}`, undefined, { token: grant.token, status: 401 });
  assert.equal(f.state.modelCalls, 0);
  assert.equal(f.state.applicationCalls, 0);
});

test("J08/J13/J17: state events share the exact memory quota and rejected projections do not stall later work", async t => {
  const f = await fixture(t);
  const view = await f.create({ events: [{ name: "state.changed", version: "1", schema: { type: "object" },
    disposition: "state", sources: ["operator"] }] });
  const session = new SessionClient({ url: f.origin, token: "fixture-operator" }).session(view.id);
  for (let index = 0; index < 64; index++) {
    const update = { request_id: randomUUID(), revision: "0", value: "x".repeat(65534) };
    let completed = false;
    for (let attempt = 0; attempt < 64; attempt++) {
      try { await session.putDocument("memory", "quota-" + index, update); completed = true; break; }
      catch (error) {
        if (!(error instanceof SessionError) || !["conflict", "overloaded"].includes(error.code)) throw error;
        await delay(20);
      }
    }
    assert(completed, "explicit unchanged-ID admission must converge after known rollbacks");
  }
  const rejected = await f.api("POST", `/v2/sessions/${view.id}/inputs`,
    { request_id: randomUUID(), data: { kind: "event", name: "state.changed", version: "1", data: {} } });
  const result = await waitFor(() => session.command(rejected.id), value => value.status !== "accepted", "quota-constrained event resolves");
  assert.equal(result.status, "failed");
  assert.equal(result.error.code, "overloaded");
  assert.deepEqual(await session.documents("state"), []);
  await f.api("DELETE", `/v2/sessions/${view.id}/memory/quota-0`);
  const accepted = await f.api("POST", `/v2/sessions/${view.id}/inputs`,
    { request_id: randomUUID(), data: { kind: "event", name: "state.changed", version: "1", data: { changed: true } } });
  await waitFor(() => session.command(accepted.id), value => value.status === "applied", "later state update is not stalled");
  assert.equal((await session.documents("state"))[0].value.changed, true);
  assert.equal(f.state.modelCalls, 0);
});

test("J09/J17/J20: slow uploads and streams have separate bounded admission without blocking health or receipts", { timeout: 30000 }, async t => {
  const f = await fixture(t, { environment: { BRAIN_FILE_TRANSFERS: "1", BRAIN_STREAMS: "1", BRAIN_REQUESTS: "2" } });
  const view = await f.create();
  const slow = httpRequest(`${f.origin}/v2/sessions/${view.id}/files/slow.txt`, { method: "PUT",
    headers: { Authorization: "Bearer fixture-operator", Expect: "100-continue", "Content-Length": 1048576 } });
  slow.on("error", () => {});
  t.after(() => slow.destroy());
  const reading = once(slow, "continue");
  slow.flushHeaders();
  await reading;
  const denied = await fetch(`${f.origin}/v2/sessions/${view.id}/files/other.txt`,
    { method: "PUT", headers: { Authorization: "Bearer fixture-operator" }, body: "bounded" });
  assert.equal(denied.status, 429);
  assert.equal((await denied.json()).code, "overloaded");
  assert.equal((await fetch(`${f.origin}/health/live`)).status, 204);
  assert.equal((await f.api("GET", `/v2/sessions/${view.id}`)).id, view.id);
  const abort = new AbortController();
  t.after(() => abort.abort());
  const stream = await fetch(`${f.origin}/v2/sessions/${view.id}/events`, {
    headers: { Authorization: "Bearer fixture-operator" }, signal: abort.signal,
  });
  assert.equal(stream.status, 200);
  const extra = await fetch(`${f.origin}/v2/sessions/${view.id}/events`, { headers: { Authorization: "Bearer fixture-operator" } });
  assert.equal(extra.status, 429);
  await extra.arrayBuffer();
  assert.equal((await fetch(`${f.origin}/health/ready`)).status, 204);
  await f.api("POST", `/v2/sessions/${view.id}/operations/unknown/receipt`,
    { request_id: randomUUID(), outcome: "completed", result: true }, { status: 401 });
  abort.abort();
  slow.destroy();
  assert.equal(f.state.modelCalls, 0);
});

test("J01/J09/J23: HTTP chatbot retains identity, usage absence and continuation across restart", async t => {
  const f = await fixture(t);
  const session = await f.create({}, "first message");
  await f.settled(session.id);
  const operations = await f.api("GET", `/v2/sessions/${session.id}/operations`);
  assert.equal(f.state.modelCalls, 1);
  assert.equal(operations[0].usage.input_tokens, 7);
  assert.equal(Object.hasOwn(operations[0].usage, "output_tokens"), false);
  const first = await f.api("GET", `/v2/sessions/${session.id}/records?limit=128`);
  assert(first.records.some(record => record.kind === "run.completed"));
  assert.equal(first.gaps.length, 0);
  await f.stop();
  await f.start();
  const retained = await f.api("GET", `/v2/sessions/${session.id}`);
  assert.equal(retained.lifecycle, "open");
  const request = { request_id: randomUUID(), data: { kind: "message", text: "continue" } };
  const accepted = await f.api("POST", `/v2/sessions/${session.id}/inputs`, request);
  const duplicate = await f.api("POST", `/v2/sessions/${session.id}/inputs`, request);
  assert.equal(accepted.id, duplicate.id);
  await f.settled(session.id, "completed", 2);
  assert.equal(f.state.modelCalls, 2);
  assert.equal(f.state.lastMessages.at(-1).content, "continue");
  assert(f.state.lastMessages.some(message => message.role === "assistant" && message.content === "response 1"));
  const page = await f.api("GET", `/v2/sessions/${session.id}/records?after=${first.next}&limit=128`);
  assert(page.records.every(record => BigInt(record.sequence) > BigInt(first.next)));
  assert.equal(new Set([...first.records, ...page.records].map(record => `${record.session_id}:${record.sequence}`)).size, first.records.length + page.records.length);
});

test("J05/J24: lost HTTP receipt never repeats an observable application effect", async t => {
  const f = await fixture(t);
  const session = await f.create({ connections: [{ name: "app", kind: "http", provider: "http", ownership: "borrowed",
    configuration: { url: f.applicationURL }, secrets: { signing: "signing" } }],
    actions: [{ name: "deliver", description: "Deliver once", connection: "app", input_schema: { type: "object" },
      max_output_bytes: 8192, timeout_ms: 10000 }] });
  await f.api("POST", `/v2/sessions/${session.id}/connections`, { request_id: randomUUID(), template: "app" });
  f.state.mode = "lost-response";
  const request = { request_id: randomUUID(), data: { kind: "action", name: "deliver", arguments: {} } };
  const accepted = await f.api("POST", `/v2/sessions/${session.id}/inputs`, request);
  await f.settled(session.id, "blocked");
  assert.equal(f.state.applicationCalls, 1);
  assert.equal((await f.api("GET", `/v2/sessions/${session.id}/operations`))[0].status, "unknown");
  await f.stop();
  await f.start();
  assert.equal((await f.api("POST", `/v2/sessions/${session.id}/inputs`, request)).id, accepted.id);
  await delay(300);
  assert.equal(f.state.applicationCalls, 1);
});

test("J06/J10/J11: claimed application execution, exact approval and late cancellation evidence", async t => {
  const f = await fixture(t);
  const session = await f.create({ connections: [{ name: "browser", kind: "application", provider: "application", ownership: "borrowed" }],
    actions: [{ name: "publish", description: "Publish", connection: "browser", input_schema: { type: "object" },
      max_output_bytes: 8192, timeout_ms: 30000, approval: true }] });
  const connection = await f.api("POST", `/v2/sessions/${session.id}/connections`, { request_id: randomUUID(), template: "browser" });
  const grant = await f.api("POST", `/v2/sessions/${session.id}/participants`, { request_id: randomUUID(), subject: connection.id,
    permissions: ["operation:claim", "session:read"], expires_ms: Date.now() + 60000 });
  await f.api("POST", `/v2/sessions/${session.id}/inputs`, { request_id: randomUUID(), data: { kind: "action", name: "publish", arguments: {} } });
  const operations = await waitFor(() => f.api("GET", `/v2/sessions/${session.id}/operations`), value => value.length === 1, "operation notification");
  const operation = operations[0];
  const claim = { request_id: randomUUID(), connection_id: connection.id, generation: connection.generation };
  await f.api("POST", `/v2/sessions/${session.id}/operations/${operation.id}/claim`, claim, { status: 403, token: grant.token });
  const interactions = await waitFor(() => f.api("GET", `/v2/sessions/${session.id}/interactions`), value => value.length === 1, "durable approval");
  const interaction = interactions[0];
  await f.api("POST", `/v2/sessions/${session.id}/interactions/${interaction.id}`,
    { request_id: randomUUID(), revision: interaction.revision, subject_digest: "wrong", accept: true, answer: true }, { status: 409 });
  await f.api("POST", `/v2/sessions/${session.id}/interactions/${interaction.id}`,
    { request_id: randomUUID(), revision: interaction.revision, subject_digest: interaction.request.subject_digest, accept: true, answer: true });
  const execution = await f.api("POST", `/v2/sessions/${session.id}/operations/${operation.id}/claim`, claim, { token: grant.token });
  await f.api("POST", `/v2/sessions/${session.id}/operations/${operation.id}/claim`, claim, { status: 409, token: grant.token });
  f.state.applicationCalls++;
  await f.api("POST", `/v2/sessions/${session.id}/inputs`, { request_id: randomUUID(), data: { kind: "control", control: "cancel" } });
  const receipt = { request_id: randomUUID(), outcome: "completed", result: { delivered: true } };
  const completed = await f.api("POST", `/v2/sessions/${session.id}/operations/${operation.id}/receipt`, receipt, { token: execution.receipt_token });
  assert.equal(completed.status, "completed");
  await f.api("POST", `/v2/sessions/${session.id}/operations/${operation.id}/receipt`, receipt, { token: execution.receipt_token });
  await f.settled(session.id, "cancelled");
  assert.equal(f.state.applicationCalls, 1);
  await f.api("DELETE", `/v2/sessions/${session.id}/participants/${grant.id}`);
  await f.api("GET", `/v2/sessions/${session.id}/operations`, undefined, { status: 401, token: grant.token });
});

test("J08/J13/J14: typed state events, independent memory and due timers survive a pause", async t => {
  const f = await fixture(t);
  const session = await f.create({ events: [{ name: "document.updated", version: "1", schema: { type: "object", required: ["revision"],
    properties: { revision: { type: "integer" } }, additionalProperties: false }, disposition: "state", sources: ["operator", "timer"] }] });
  await f.api("POST", `/v2/sessions/${session.id}/inputs`, { request_id: randomUUID(), data: { kind: "event", name: "document.updated", version: "1", data: {} } }, { status: 400 });
  await f.api("POST", `/v2/sessions/${session.id}/inputs`, { request_id: randomUUID(), data: { kind: "event", name: "document.updated", version: "1", data: { revision: 1 } } });
  const state = await waitFor(() => f.api("GET", `/v2/sessions/${session.id}/state`), value => value.length === 1, "state-only event");
  assert.equal(state[0].value.revision, 1);
  assert.equal(f.state.modelCalls, 0);
  await f.api("PUT", `/v2/sessions/${session.id}/memory/preference`, { request_id: randomUUID(), revision: "0", value: { language: "en" } });
  await f.api("POST", `/v2/sessions/${session.id}/inputs`, { request_id: randomUUID(), data: { kind: "control", control: "pause" } });
  await f.api("POST", `/v2/sessions/${session.id}/schedules`, { request_id: randomUUID(), due_ms: Date.now() + 100,
    input: { kind: "message", text: "scheduled work" } });
  await delay(800);
  assert.equal(f.state.modelCalls, 0);
  await f.api("POST", `/v2/sessions/${session.id}/inputs`, { request_id: randomUUID(), data: { kind: "control", control: "resume" } });
  await f.settled(session.id);
  assert.equal(f.state.modelCalls, 1);
  assert.equal((await f.api("GET", `/v2/sessions/${session.id}/memory`))[0].value.language, "en");
});

test("J02/J06/J24: TypeScript SSE worker persists its receipt before a lost acknowledgement", async t => {
  const f = await fixture(t);
  const operator = new SessionClient({ url: f.origin, token: "fixture-operator" });
  const session = await operator.create(f.config({ connections: [{ name: "worker", kind: "application", provider: "application", ownership: "borrowed" }],
    actions: [{ name: "deliver", description: "Deliver once", connection: "worker", input_schema: { type: "object" },
      max_output_bytes: 8192, timeout_ms: 30000 }] }));
  const connection = await session.attach("worker");
  const grant = await session.grant({ request_id: randomUUID(), subject: connection.id, permissions: ["operation:claim", "session:read"], expires_ms: Date.now() + 90000 });
  const applicationDirectory = f.directory + "-worker";
  const receipts = new SQLiteReceipts(path.join(applicationDirectory, "receipts.sqlite"));
  t.after(async () => { receipts.close(); await rm(applicationDirectory, { recursive: true, force: true }); });
  const handler = async execution => {
    const body = JSON.stringify(execution);
    const response = await fetch(f.applicationURL, { method: "POST", body, headers: {
      "X-Brain-Operation": execution.operation.id,
      "X-Brain-Signature": `v1=${createHmac("sha256", "fixture-signing-key").update(body).digest("hex")}`,
    } });
    return response.json();
  };
  let loseAcknowledgement = true;
  const worker = new SessionClient({ url: f.origin, token: grant.token, fetch: async (url, options) => {
    const response = await fetch(url, options);
    if (String(url).endsWith("/receipt") && loseAcknowledgement) {
      loseAcknowledgement = false;
      await response.body.cancel();
      throw new TypeError("deliberately lost HTTP acknowledgement");
    }
    return response;
  } }).session(session.id);
  const outcome = worker.work(connection, handler, receipts, { signal: AbortSignal.timeout(15000) }).then(() => null, error => error);
  await session.invoke("deliver", {});
  const error = await outcome;
  assert(error instanceof SessionError && error.uncertain, String(error));
  await f.settled(session.id);
  assert.equal(f.state.applicationCalls, 1);
  const pending = [];
  for await (const item of receipts.pending()) pending.push(item);
  assert.equal(pending.length, 1);
  assert.equal(pending[0].receipt.outcome, "completed");
  await f.stop(); await f.start();
  const recovered = new SessionClient({ url: f.origin, token: grant.token }).session(session.id);
  const abort = new AbortController();
  const recovery = recovered.work(connection, handler, receipts, { after: "0", signal: abort.signal }).catch(error => {
    if (!abort.signal.aborted) throw error;
  });
  await waitFor(async () => {
    let count = 0; for await (const ignored of receipts.pending()) count++;
    return count;
  }, count => count === 0, "application acknowledges the persisted original receipt");
  await delay(200);
  abort.abort(); await recovery;
  assert.equal(f.state.applicationCalls, 1);
});

test("J02: Python SDK streams and continues a real public-service conversation", async t => {
  const f = await fixture(t);
  const child = spawn(process.env.BRAIN_TEST_PYTHON ?? "python", [path.join(root, "tests", "fixtures", "python-client.py")], {
    cwd: root, env: { ...process.env, PYTHONPATH: path.join(root, "packages", "brain-python"), BRAIN_TEST_URL: f.origin,
      BRAIN_TEST_CONFIG: JSON.stringify(f.config()) }, stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  child.stdout.on("data", chunk => { output += chunk; });
  child.stderr.on("data", chunk => { output += chunk; });
  t.after(async () => { if (child.exitCode === null && child.signalCode === null) { const stopped = once(child, "exit"); child.kill(); await stopped; } });
  const [code] = await once(child, "exit");
  assert.equal(code, 0, output);
  assert.match(output, /python-public-journey: passed/u);
  assert.equal(f.state.modelCalls, 2);
});

test("J11/J17: duplicate controls cannot undo later controls and undispatched expiry releases reservations", async t => {
  const f = await fixture(t);
  const session = await f.create({ connections: [{ name: "worker", kind: "application", provider: "application", ownership: "borrowed" }],
    actions: [{ name: "wait", description: "Wait for a worker", connection: "worker", input_schema: { type: "object" },
      max_output_bytes: 8192, timeout_ms: 1000 }] });
  const pause = { request_id: randomUUID(), data: { kind: "control", control: "pause" } };
  await f.api("POST", `/v2/sessions/${session.id}/inputs`, pause);
  assert.equal((await f.api("GET", `/v2/sessions/${session.id}`)).paused, true);
  await f.api("POST", `/v2/sessions/${session.id}/inputs`, { request_id: randomUUID(), data: { kind: "control", control: "resume" } });
  await f.api("POST", `/v2/sessions/${session.id}/inputs`, pause);
  assert.equal((await f.api("GET", `/v2/sessions/${session.id}`)).paused, false);
  await f.api("POST", `/v2/sessions/${session.id}/connections`, { request_id: randomUUID(), template: "worker" });
  await f.api("POST", `/v2/sessions/${session.id}/inputs`, { request_id: randomUUID(), data: { kind: "action", name: "wait", arguments: {} } });
  await f.settled(session.id, "blocked");
  const operations = await f.api("GET", `/v2/sessions/${session.id}/operations`);
  assert.equal(operations[0].status, "failed");
  assert.equal(f.state.applicationCalls, 0);
  assert.equal((await f.api("GET", "/operator/storage")).reserved_output_bytes, 0);
});

test("J02/J10/J18: both SDKs traverse byte-limited collections and every export section", { timeout: 240000 }, async t => {
  const f = await fixture(t);
  const view = await f.create({ connections: [{ name: "browser", kind: "application", provider: "application", ownership: "borrowed" }] });
  const session = new SessionClient({ url: f.origin, token: "fixture-operator" }).session(view.id);
  const knownRollback = async action => {
    for (let attempt = 0; attempt < 64; attempt++) {
      try { return await action(); }
      catch (error) {
        if (!(error instanceof SessionError) || !["conflict", "overloaded"].includes(error.code)) throw error;
        await delay(20);
      }
    }
    assert.fail("explicit admission after known rollbacks did not converge");
  };
  for (let index = 0; index < 64; index++) {
    const request = { request_id: randomUUID(), revision: "0", value: "x".repeat(60000) };
    await knownRollback(() => session.putDocument("memory", "key-" + String(index).padStart(3, "0"), request));
  }
  const first = await session.documents("memory", 128);
  assert(first.length > 0 && first.length < 64, "SQL byte limits, not just count limits, must shorten this page");
  const documents = [];
  for await (const document of session.allDocuments("memory")) documents.push(document.key);
  assert.equal(documents.length, 64);
  assert.equal(new Set(documents).size, 64);
  for (let index = 0; index < 132; index++) {
    const request = { request_id: randomUUID(), kind: "question", run_id: "external", subject: "question-" + index,
      subject_digest: "fixture-subject", prompt: "Choose", schema: { type: "string" }, expires_ms: Date.now() + 600000 };
    await knownRollback(() => session.interact(request));
  }
  const interactions = [];
  for await (const interaction of session.allInteractions()) interactions.push(interaction);
  assert.equal(interactions.length, 132);
  assert.equal(new Set(interactions.map(value => value.id)).size, 132);
  const last = interactions.at(-1);
  const answer = { request_id: randomUUID(), revision: last.revision, subject_digest: last.request.subject_digest, accept: true, answer: "selected" };
  assert.equal((await knownRollback(() => session.answer(last.id, answer))).status, "answered");
  for (let index = 0; index < 3; index++) {
    const attachment = { requestId: randomUUID() };
    await knownRollback(() => session.attach("browser", attachment));
    const schedule = { request_id: randomUUID(), due_ms: Date.now() + 600000, input: { kind: "message", text: "future fixture" } };
    await knownRollback(() => session.schedule(schedule));
    await knownRollback(() => session.putFile("file-" + index, new TextEncoder().encode("fixture file")));
  }
  const closeID = randomUUID();
  const closed = await knownRollback(() => session.control("close", closeID));
  await waitFor(() => session.command(closed.id), value => value.status === "applied", "closed writer reaches its durable input boundary", 60000);
  const counts = { memory: 0, interactions: 0, connections: 0, schedules: 0, files: 0 };
  const sections = new Set();
  let final, pages = 0, continuation;
  for await (const page of session.exportPages()) {
    assert(Buffer.byteLength(JSON.stringify(page)) < 4 * 1024 * 1024);
    sections.add(page.section);
    for (const key of Object.keys(counts)) counts[key] += page[key].length;
    if (!continuation && page.continuation) continuation = page.continuation;
    final = page;
    pages++;
  }
  assert.deepEqual(counts, { memory: 64, interactions: 132, connections: 3, schedules: 3, files: 3 });
  assert.equal(sections.size, 9);
  assert(pages > 9);
  assert.equal(final.complete, true);
  assert.equal(final.continuation, "");
  await assert.rejects(session.export(continuation + "x"), error => error instanceof SessionError && error.code === "invalid");
  assert.match(await runPython(t, "python-pagination.py", { BRAIN_TEST_URL: f.origin, BRAIN_TEST_SESSION: view.id }), /python-pagination: passed/u);
  assert.equal(f.state.modelCalls, 0);
});

test("J02/J24: both SDKs expose generated request ids after real lost mutation acknowledgements", async t => {
  const f = await fixture(t);
  const view = await f.create();
  let lost = false, submissions = 0;
  const client = new SessionClient({ url: f.origin, token: "fixture-operator", fetch: async (input, init) => {
    const response = await fetch(input, init);
    if (new URL(String(input)).pathname.endsWith("/inputs")) {
      submissions++;
      if (!lost) { lost = true; await response.body?.cancel(); throw new TypeError("fixture lost acknowledgement"); }
    }
    return response;
  } });
  const session = client.session(view.id);
  let original;
  await assert.rejects(session.send("typescript uncertain acknowledgement"), error => {
    assert(error instanceof SessionError && error.uncertain);
    assert.match(error.requestId, /^[a-f0-9-]{36}$/u);
    original = error.requestId;
    return true;
  });
  assert.equal(submissions, 1, "SDK automatically repeated the ambiguous mutation");
  await session.send("typescript uncertain acknowledgement", original);
  await f.settled(view.id);
  assert.equal(submissions, 2, "only the caller's explicit reconciliation may resubmit");
  assert.equal(f.state.modelCalls, 1);
  let pythonInputs = 0;
  const bridge = createServer(async (request, response) => {
    let body = "";
    for await (const chunk of request) body += chunk;
    const upstream = await fetch(f.origin + request.url, { method: request.method,
      headers: { Authorization: "Bearer fixture-operator", "Content-Type": "application/json" }, body: body || undefined });
    const text = await upstream.text();
    if (request.url.endsWith("/inputs") && ++pythonInputs === 1) { request.socket.destroy(); return; }
    response.writeHead(upstream.status, { "Content-Type": "application/json" }).end(text);
  });
  bridge.listen(0, "127.0.0.1");
  await once(bridge, "listening");
  t.after(() => new Promise(resolve => bridge.close(resolve)));
  assert.match(await runPython(t, "python-uncertain.py", { BRAIN_TEST_URL: `http://127.0.0.1:${bridge.address().port}`, BRAIN_TEST_SESSION: view.id }),
    /python-uncertain-reconciliation: passed/u);
  await f.settled(view.id, "completed", 2);
  assert.equal(pythonInputs, 2);
  assert.equal(f.state.modelCalls, 2);
});
