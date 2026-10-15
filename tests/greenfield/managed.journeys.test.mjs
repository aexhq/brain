import test from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { spawn, execFileSync } from "node:child_process";
import { once } from "node:events";
import { randomUUID, randomBytes } from "node:crypto";
import { mkdir, readFile, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const extension = process.platform === "win32" ? ".exe" : "";
const delay = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
const nodeImage = "node@sha256:862263c612aa437e3037674b85419622a9d93bff80aa1eee5398dfe686375532";
const pythonImage = "python@sha256:78387bc3881b8273120a12ebe6c1ab22b018ccc2c9adf565ae1ac9b536e184ea";

async function waitFor(get, accept, description, timeout = 120000) {
  let value;
  const until = Date.now() + timeout;
  while (Date.now() < until) {
    value = await get();
    if (accept(value)) return value;
    await delay(50);
  }
  assert.fail(`${description}: ${JSON.stringify(value)}`);
}

async function fixture(t) {
  const directory = path.join(root, ".test-output", `managed-${randomUUID()}`);
  await mkdir(directory, { recursive: true });
  const processes = [], resources = new Set(), images = new Set();
  const key = randomBytes(32).toString("hex");
  const token = randomBytes(32).toString("hex");
  let brainOrigin = "", workerOrigin = "", allowCallbacks = true, loseResponse = false;
  let workerProcess, callbackReceipts = 0, sentInvocations = 0;
  const baseEnv = Object.fromEntries(["PATH", "SystemRoot", "USERPROFILE", "HOME", "DOCKER_HOST", "DOCKER_CONTEXT", "DOCKER_CONFIG"]
    .filter(name => process.env[name]).map(name => [name, process.env[name]]));
  async function proxy(target, callback) {
    const server = createServer(async (request, response) => {
      try {
        let body = "";
        for await (const chunk of request) {
          body += chunk;
          assert(Buffer.byteLength(body) <= 1 << 20);
        }
        if (callback && !allowCallbacks) { response.writeHead(503).end(); return; }
        if (!callback) {
          const invocation = JSON.parse(body);
          resources.add(invocation.execution.operation.id);
          sentInvocations++;
        }
        const upstream = await fetch(target() + request.url, { method: request.method, body,
          headers: { "Content-Type": "application/json", ...(request.headers.authorization ? { Authorization: request.headers.authorization } : {}),
            ...(request.headers["x-brain-signature"] ? { "X-Brain-Signature": request.headers["x-brain-signature"],
              "X-Brain-Operation": request.headers["x-brain-operation"] } : {}) }, redirect: "manual" });
        const result = await upstream.text();
        if (callback && upstream.ok) callbackReceipts++;
        if (!callback && loseResponse) { request.socket.destroy(); return; }
        response.writeHead(upstream.status, { "Content-Type": "application/json" }).end(result);
      } catch {
        if (!response.headersSent) response.writeHead(502);
        response.end();
      }
    });
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    return { server, origin: `http://127.0.0.1:${server.address().port}` };
  }
  const callbacks = await proxy(() => brainOrigin, true);
  const invocations = await proxy(() => workerOrigin, false);
  let modelCalls = 0;
  const model = createServer((_request, response) => {
    modelCalls++;
    response.writeHead(500).end("managed-only journey must not call a model");
  });
  model.listen(0, "127.0.0.1");
  await once(model, "listening");
  async function start(binary, environment, marker) {
    const child = spawn(path.join(root, ".build", binary + extension), [], {
      cwd: root, env: { ...baseEnv, ...environment }, stdio: ["ignore", "pipe", "pipe"],
    });
    processes.push(child);
    let log = "";
    child.stdout.on("data", chunk => { log += chunk; });
    child.stderr.on("data", chunk => { log += chunk; });
    await waitFor(() => ({ log, exit: child.exitCode }), value => {
      assert.equal(value.exit, null, `${binary} exited: ${value.log}`);
      return value.log.includes(marker);
    }, `${binary} readiness`, 45000);
    const origin = `http://${log.match(/listen=(127\.0\.0\.1:\d+)/u)[1]}`;
    assert.equal((await fetch(origin + "/health/live")).status, 204);
    return { child, origin };
  }
  async function stop(child) {
    if (child.exitCode !== null || child.signalCode !== null) return;
    const done = once(child, "exit");
    child.kill();
    await done;
  }
  async function startWorker() {
    const running = await start("brain-worker", {
      BRAIN_WORKER_DATA: path.join(directory, "worker"), BRAIN_WORKER_KEY: key,
      BRAIN_WORKER_LISTEN: "127.0.0.1:0", BRAIN_WORKER_TRUSTED_DEVELOPMENT: "1",
      BRAIN_WORKER_CALLBACK_ORIGINS: callbacks.origin, BRAIN_WORKER_CONCURRENCY: "1",
      BRAIN_WORKER_NODE_IMAGE: nodeImage, BRAIN_WORKER_PYTHON_IMAGE: pythonImage,
    }, "worker_ready");
    workerProcess = running.child;
    workerOrigin = running.origin;
  }
  t.after(async () => {
    for (const child of processes) await stop(child);
    for (const { server } of [callbacks, invocations]) await new Promise(resolve => server.close(resolve));
    await new Promise(resolve => model.close(resolve));
    for (const id of resources) {
      const created = execFileSync("docker", ["container", "ls", "--all", "--quiet", "--filter", `label=dev.aex.brain.operation=${id}`],
        { cwd: root, encoding: "utf8", timeout: 15000 }).trim().split(/\s+/u).filter(Boolean);
      for (const container of created) execFileSync("docker", ["container", "rm", "--force", container], { cwd: root, stdio: "pipe", timeout: 15000 });
      const committed = execFileSync("docker", ["image", "ls", "--quiet", "--no-trunc", "--filter", `label=dev.aex.brain.operation=${id}`],
        { cwd: root, encoding: "utf8", timeout: 15000 }).trim().split(/\s+/u).filter(Boolean);
      for (const image of committed) images.add(image);
    }
    for (const image of images) execFileSync("docker", ["image", "rm", image], { cwd: root, stdio: "pipe", timeout: 30000 });
    await rm(directory, { recursive: true, force: true });
    assert.equal(modelCalls, 0, "managed package execution made an unexpected model request");
  });
  await startWorker();
  const core = await start("brain", {
    BRAIN_DATA: path.join(directory, "core"), BRAIN_LISTEN: "127.0.0.1:0", BRAIN_API_TOKEN: token, BRAIN_DEVELOPMENT: "1",
    BRAIN_PUBLIC_URL: callbacks.origin, BRAIN_COMPUTE_WORKERS: JSON.stringify({ local_compute: { url: invocations.origin + "/invoke", signing: "worker" } }),
    BRAIN_HARNESSES: JSON.stringify([{ name: "fixture", version: "1", command: process.execPath,
      arguments: [path.join(root, "tests", "fixtures", "decision-harness.mjs")], workers: 1, models: ["fixture"], continuation: true }]),
    BRAIN_MODELS: JSON.stringify({ fixture: { kind: "openai", credential: "fixture", base_url: `http://127.0.0.1:${model.address().port}/v1/` } }),
    BRAIN_SECRET_REFS: JSON.stringify({ local: { fixture: "FIXTURE_MODEL_KEY", worker: "FIXTURE_WORKER_KEY", package: "FIXTURE_PACKAGE_KEY" } }),
    FIXTURE_MODEL_KEY: "fixture-not-a-provider-key", FIXTURE_WORKER_KEY: key, FIXTURE_PACKAGE_KEY: "journey-scoped-secret",
    BRAIN_WORKERS: "4", BRAIN_INFLIGHT: "4", BRAIN_L1_BYTES: "1048576", BRAIN_L2_BYTES: "33554432",
    BRAIN_DIRTY_BYTES: "16777216", BRAIN_L2_SHARDS: "2",
  }, "ready listen=");
  brainOrigin = core.origin;
  async function api(method, resource, body, expected = 200) {
    const response = await fetch(brainOrigin + resource, { method, headers: { Authorization: `Bearer ${token}`,
      ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await response.text();
    assert.equal(response.status, expected, `${method} ${resource}: ${text}`);
    return text ? JSON.parse(text) : undefined;
  }
  async function session(actions, capabilities) {
    return api("POST", "/v2/sessions", { request_id: randomUUID(), config: {
      model: { provider: "fixture", name: "fixture" }, harness: { name: "fixture", version: "1" }, actions, events: [],
      connections: [{ name: "worker", kind: "local", provider: "local_compute", ownership: "managed", capabilities,
        secrets: { PACKAGE_SECRET: "package" } }], grants: ["session:read", "input:write"],
      limits: { calls: 100, concurrent: 4, input_bytes: 131072, output_bytes: 65536, retained_bytes: 16777216, pending_inputs: 128 },
    } });
  }
  async function attach(id) {
    return api("POST", `/v2/sessions/${id}/connections`, { request_id: randomUUID(), template: "worker" });
  }
  const builder = await session([{ name: "build_package", description: "Build immutable managed package", connection: "worker",
    input_schema: { type: "object", required: ["package_id"], properties: { package_id: { type: "string" } } },
    max_output_bytes: 8192, timeout_ms: 300000 }], ["build"]);
  await attach(builder.id);
  async function build(language) {
    const fixture = path.join(root, "tests", "fixtures", `managed-${language}`);
    const entrypoint = language === "node" ? "action.ts" : "action.py";
    const files = { [entrypoint]: await readFile(path.join(fixture, entrypoint), "utf8") };
    if (language === "node") files["package.json"] = await readFile(path.join(fixture, "package.json"), "utf8");
    const action = { name: "inspect", description: "Inspect dependency, limits and secret scope", connection: "worker",
      input_schema: { type: "object", required: ["value"], properties: { value: { type: "string" } } },
      max_output_bytes: 8192, timeout_ms: 15000 };
    const manifest = { name: `journey-${language}`, version: "1.0.0", language, entrypoint, files,
      lockfile: await readFile(path.join(fixture, language === "node" ? "package-lock.json" : "requirements.txt"), "utf8"),
      actions: [action], secrets: ["PACKAGE_SECRET"], network: [], memory_bytes: 134217728, cpu_millis: 1000, wall_ms: 15000 };
    const admitted = await api("POST", "/v2/packages", { request_id: randomUUID(), session_id: builder.id, builder: "worker", manifest });
    const pkg = await waitFor(() => api("GET", `/v2/packages/${admitted.id}`), value => {
      assert(!["failed", "unknown"].includes(value.status), `managed ${language} build: ${value.status}: ${value.error}`);
      return value.status === "ready";
    }, `immutable ${language} package build`);
    images.add(pkg.image);
    assert.match(pkg.image, /^sha256:[a-f0-9]{64}$/u);
    const runtime = await session([{ ...action, package: pkg.id }], ["invoke"]);
    await attach(runtime.id);
    return { runtime, pkg };
  }
  return { api, build, resources, images, get workerOrigin() { return workerOrigin; },
    get sentInvocations() { return sentInvocations; }, get callbackReceipts() { return callbackReceipts; },
    callbacks(value) { allowCallbacks = value; }, loseResponse(value) { loseResponse = value; },
    async restartWorker() { await stop(workerProcess); await startWorker(); } };
}

test("J04/J17: TypeScript and Python managed packages execute real locked dependencies outside the API process", { timeout: 240000 }, async t => {
  const f = await fixture(t);
  assert.equal((await fetch(f.workerOrigin + "/invoke", { method: "POST", body: "{}" })).status, 401);
  const catalog = await f.api("GET", "/v2/catalog");
  assert(catalog.some(entry => entry.name === "local_compute" && entry.limitations.some(value => value.includes("hostile-code"))));
  for (const language of ["node", "python"]) {
    const { runtime } = await f.build(language);
    await f.api("POST", `/v2/sessions/${runtime.id}/inputs`, { request_id: randomUUID(),
      data: { kind: "action", name: "inspect", arguments: { value: "12" } } });
    const [operation] = await waitFor(() => f.api("GET", `/v2/sessions/${runtime.id}/operations`), values => {
      if (values[0]) assert(!["failed", "unknown"].includes(values[0].status), JSON.stringify(values[0]));
      return values.length === 1 && values[0].status === "completed";
    }, `${language} managed invocation`);
    assert.deepEqual(operation.result, { dependency: true, secret_injected: true, internal_key_absent: true,
      read_only: true, uid: 65532, operation_id: operation.id });
    assert.equal(operation.error, undefined);
    await waitFor(() => f.api("GET", `/v2/sessions/${runtime.id}/runs`),
      runs => runs.length === 1 && runs[0].status === "completed", "managed run completes");
  }
  assert.equal(f.sentInvocations, 4);
});

test("J04/J24: retained managed receipts recover after lost transport without rerunning supplied code", { timeout: 180000 }, async t => {
  const f = await fixture(t);
  const { runtime } = await f.build("node");
  f.callbacks(false);
  f.loseResponse(true);
  await f.api("POST", `/v2/sessions/${runtime.id}/inputs`, { request_id: randomUUID(),
    data: { kind: "action", name: "inspect", arguments: { value: "12" } } });
  const [uncertain] = await waitFor(() => f.api("GET", `/v2/sessions/${runtime.id}/operations`),
    values => values.length === 1 && values[0].status === "unknown", "lost worker response is uncertain");
  const before = f.sentInvocations;
  await f.restartWorker();
  f.callbacks(true);
  const recovered = await waitFor(() => f.api("GET", `/v2/sessions/${runtime.id}/operations/${uncertain.id}`),
    value => value.status === "completed", "worker reports its retained receipt");
  assert.equal(recovered.result.operation_id, uncertain.id);
  assert.equal(f.sentInvocations, before);
  assert(f.callbackReceipts > 0);
  await f.api("POST", `/v2/sessions/${runtime.id}/inputs`, { request_id: randomUUID(), data: { kind: "control", control: "resume" } });
  await waitFor(() => f.api("GET", `/v2/sessions/${runtime.id}/runs`),
    runs => runs.length === 1 && runs[0].status === "completed", "explicit continuation consumes the retained outcome");
  assert.equal(f.sentInvocations, before);
});
