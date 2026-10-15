import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { randomUUID, randomBytes } from "node:crypto";
import { mkdir, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const baseImage = "node@sha256:862263c612aa437e3037674b85419622a9d93bff80aa1eee5398dfe686375532";
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));

function docker(args, env = {}) {
  return execFileSync("docker", args, { cwd: root, encoding: "utf8", timeout: 30000,
    env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "pipe"] }).trim();
}
function logs(id) {
  const result = spawnSync("docker", ["logs", id], { cwd: root, encoding: "utf8", timeout: 15000 });
  return result.stdout + result.stderr;
}
async function until(get, accept, label, timeout = 45000) {
  let value;
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    value = await get();
    if (accept(value)) return value;
    await delay(80);
  }
  assert.fail(`${label}: ${JSON.stringify(value)}`);
}

async function clusterFixture(t) {
  const postgres = process.env.BRAIN_TEST_POSTGRES_CONTAINER;
  assert.match(postgres ?? "", /^[a-f0-9]{64}$/u, "run through postgres-emulator.ps1 -Cluster");
  const configured = new URL(process.env.BRAIN_TEST_POSTGRES_URL);
  assert.equal(configured.hostname, "127.0.0.1");
  const suffix = randomUUID().replaceAll("-", "");
  const schema = `cluster_${suffix}`;
  const directory = path.join(root, ".test-output", schema);
  const archive = path.join(directory, "archive");
  await mkdir(archive, { recursive: true });
  const containers = new Set(), paused = new Set();
  const network = docker(["network", "create", "--label", "copilot.session=211666ca-c6e9-4741-aef7-9b195ab235c0", `brain-cluster-${suffix}`]);
  let connected = false;
  const psql = command => docker(["exec", postgres, "psql", "-U", "brain", "-d", "brain", "-v", "ON_ERROR_STOP=1", "-q", "-c", command]);
  async function remove(id) {
    if (paused.has(id)) { docker(["unpause", id]); paused.delete(id); }
    docker(["container", "rm", "--force", id]);
    containers.delete(id);
  }
  t.after(async () => {
    for (const id of [...containers]) await remove(id);
    if (connected) docker(["network", "disconnect", network, postgres]);
    docker(["network", "rm", network]);
    psql(`DROP SCHEMA IF EXISTS ${schema} CASCADE`);
    await rm(directory, { recursive: true, force: true });
  });
  psql(`CREATE SCHEMA ${schema}`);
  docker(["network", "connect", "--alias", "database", network, postgres]);
  connected = true;
  const sourceMount = `type=bind,source=${root},target=/work,readonly`;
  const fixture = docker(["run", "--detach", "--network", network, "--network-alias", "fixture",
    "--cpus", "0.5", "--memory", "128m", "--pids-limit", "64", "--read-only", "--cap-drop=ALL",
    "--publish", "127.0.0.1::9000", "--mount", sourceMount, baseImage, "node", "/work/tests/fixtures/cluster-model.mjs"]);
  containers.add(fixture);
  const fixtureURL = "http://" + docker(["port", fixture, "9000/tcp"]);
  await until(async () => fetch(fixtureURL + "/health/live").then(r => r.status).catch(() => 0), value => value === 204, "model fixture readiness");
  const key = randomBytes(32).toString("hex"), token = randomBytes(32).toString("hex");
  const controlURL = new URL(configured);
  controlURL.hostname = "database";
  controlURL.port = "5432";
  controlURL.searchParams.set("search_path", schema);
  const common = {
    BRAIN_API_TOKEN: token, BRAIN_CUSTODY_KEY: key, BRAIN_DEVELOPMENT: "1", BRAIN_DATA: "/data",
    BRAIN_CONTROL_URL: controlURL.toString(), BRAIN_ARCHIVE_DIRECTORY: "/archive", BRAIN_LISTEN: "0.0.0.0:8080",
    BRAIN_HARNESSES: JSON.stringify([{ name: "fixture", version: "1", command: "node",
      arguments: ["/work/tests/fixtures/decision-harness.mjs"], workers: 1, models: ["fixture"], continuation: true }]),
    BRAIN_MODELS: JSON.stringify({ fixture: { kind: "openai", base_url: "http://fixture:9000/v1/", credential: "fixture" } }),
    BRAIN_SECRET_REFS: JSON.stringify({ local: { fixture: "FIXTURE_MODEL_KEY" } }), FIXTURE_MODEL_KEY: "fixture-no-live-key",
    BRAIN_WORKERS: "4", BRAIN_INFLIGHT: "4", BRAIN_LEASE_MS: "6000", BRAIN_L1_BYTES: "0",
    BRAIN_L2_BYTES: "33554432", BRAIN_DIRTY_BYTES: "16777216", BRAIN_L2_SHARDS: "2",
  };
  async function start(name, { volume = name, interval = "60000", custody = key, expectedFailure = false, harness = "fixture", continuation = true } = {}) {
    const data = path.join(directory, volume);
    await mkdir(data, { recursive: true });
    const env = { ...common, BRAIN_NODE_NAME: name, BRAIN_INTERNAL_URL: `http://${name}:8080`,
      BRAIN_ARCHIVE_FLUSH_MS: interval, BRAIN_CUSTODY_KEY: custody,
      BRAIN_HARNESSES: JSON.stringify([{ name: harness, version: "1", command: "node",
        arguments: ["/work/tests/fixtures/decision-harness.mjs"], workers: 1, models: ["fixture"], continuation }]) };
    const id = docker(["run", "--detach", "--network", network, "--network-alias", name,
      "--cpus", "1", "--memory", "512m", "--pids-limit", "128", "--read-only", "--cap-drop=ALL",
      "--security-opt=no-new-privileges", "--publish", "127.0.0.1::8080", "--mount", sourceMount,
      "--mount", `type=bind,source=${data},target=/data`, "--mount", `type=bind,source=${archive},target=/archive`,
      "--workdir", "/work", ...Object.keys(env).flatMap(key => ["--env", key]), baseImage, "/work/.build/brain-linux"], env);
    containers.add(id);
    if (expectedFailure) {
      await until(() => docker(["inspect", id, "--format", "{{.State.Status}}"]), state => state === "exited", "mismatched custody refuses startup");
      assert.match(logs(id), /original retained cluster custody key/u);
      return { id };
    }
    const origin = "http://" + docker(["port", id, "8080/tcp"]);
    await until(async () => {
      const running = docker(["inspect", id, "--format", "{{.State.Running}}"]);
      assert.equal(running, "true", logs(id));
      return fetch(origin + "/health/ready").then(r => r.status).catch(() => 0);
    }, status => status === 204, `${name} readiness`);
    return { id, origin };
  }
  async function api(node, method, resource, body, expected = 200, authority = token) {
    const response = await fetch(node.origin + resource, { method, headers: { Authorization: `Bearer ${authority}`,
      ...(body === undefined ? {} : { "Content-Type": "application/json" }) }, body: body === undefined ? undefined : JSON.stringify(body) });
    const text = await response.text();
    assert.equal(response.status, expected, `${method} ${resource}: ${text}`);
    return text ? JSON.parse(text) : undefined;
  }
  const config = overrides => ({
    model: { provider: "fixture", name: "fixture" }, harness: { name: "fixture", version: "1" },
    connections: [], actions: [], events: [{ name: "observation", version: "1", schema: { type: "object" }, disposition: "state", sources: ["operator"] }],
    grants: ["session:read", "input:write"],
    limits: { calls: 100, concurrent: 4, input_bytes: 131072, output_bytes: 65536, retained_bytes: 16777216, pending_inputs: 128 }, ...overrides,
  });
  return { start, api, config, fixtureURL, paused, remove, token };
}

test("J19/J21/J22/J23/J24: real Linux owners, cross-ingress claims, stale processes and returning lost-volume evidence", { timeout: 180000 }, async t => {
  const { start, api, config, fixtureURL, paused, remove, token } = await clusterFixture(t);
  const a = await start("node-a");
  const session = await api(a, "POST", "/v2/sessions", { request_id: randomUUID(), config: config({
    connections: [{ name: "application", kind: "application", provider: "application", ownership: "borrowed" }],
    actions: [{ name: "deliver", description: "Deliver once", connection: "application", input_schema: { type: "object" },
      max_output_bytes: 8192, timeout_ms: 60000 }],
  }) });
  await api(a, "POST", `/v2/sessions/${session.id}/inputs`, { request_id: randomUUID(), data: {
    kind: "event", name: "observation", version: "1", data: { seed: true },
  } });
  await until(() => api(a, "GET", `/v2/sessions/${session.id}`), value => value.owner_epoch === "1", "first live owner");
  const b = await start("node-b");
  const connection = await api(b, "POST", `/v2/sessions/${session.id}/connections`, { request_id: randomUUID(), template: "application" });
  await api(b, "POST", `/v2/sessions/${session.id}/inputs`, { request_id: randomUUID(), data: { kind: "action", name: "deliver", arguments: {} } });
  const [operation] = await until(() => api(b, "GET", `/v2/sessions/${session.id}/operations`),
    values => values.length === 1 && values[0].status === "prepared", "prepared application operation through another ingress");
  const execution = await api(b, "POST", `/v2/sessions/${session.id}/operations/${operation.id}/claim`,
    { request_id: randomUUID(), connection_id: connection.id, generation: connection.generation });
  const effect = await fetch(fixtureURL + "/effect", { method: "POST", body: "{}" }).then(r => r.json());
  assert.equal(effect.delivered, 1);
  const before = await until(async () => {
    const page = await api(b, "GET", `/v2/sessions/${session.id}/records?limit=128`);
    assert(page.gaps.every(gap => gap.kind === "reserved"),
      `live allocated positions may still be committing; written records must be routable: ${JSON.stringify(page.gaps)}`);
    return page;
  }, page => page.gaps.length === 0, "all current transcript writes become durably routable");
  assert.equal(before.gaps.length, 0, "authenticated node routing must read still-unarchived L2 records");
  assert(before.records.some(record => record.kind === "operation.intent"));
  assert((await api(a, "GET", "/operator/storage")).reserved_output_bytes > 0, "dispatched output has a real local reserve");
  assert.equal((await fetch(b.origin + `/internal/v2/records/${session.id}/1?digest=invalid`, { headers: { Authorization: `Bearer ${token}` } })).status, 401);

  docker(["pause", a.id]); paused.add(a.id);
  await delay(6500);
  await api(b, "POST", `/v2/sessions/${session.id}/inputs`, { request_id: randomUUID(), data: {
    kind: "event", name: "observation", version: "1", data: { owner_loss: true },
  } });
  const moved = await until(() => api(b, "GET", `/v2/sessions/${session.id}`), value => BigInt(value.owner_epoch) > 1n, "fenced successor ownership");
  assert.equal(moved.id, session.id);
  await api(b, "POST", `/v2/sessions/${session.id}/operations/${operation.id}/claim`,
    { request_id: randomUUID(), connection_id: connection.id, generation: connection.generation }, 409);
  await api(b, "POST", `/v2/sessions/${session.id}/operations/${operation.id}/receipt`,
    { request_id: randomUUID(), outcome: "completed", result: effect }, 200, execution.receipt_token);
  await until(() => api(b, "GET", `/v2/sessions/${session.id}/runs`),
    runs => runs.length === 1 && runs[0].status === "completed", "successor consumes the original application receipt");
  docker(["unpause", a.id]); paused.delete(a.id);
  await until(async () => fetch(a.origin + "/health/ready").then(r => r.status), status => status === 503, "old process observes expired incarnation");
  const returned = await until(() => api(b, "GET", `/v2/sessions/${session.id}/records?limit=128`),
    page => page.gaps.length === 0, "old-node bytes return only as verified history");
  const identities = new Set(returned.records.map(record => `${record.session_id}:${record.sequence}`));
  for (const record of before.records) assert(identities.has(`${record.session_id}:${record.sequence}`));
  await remove(a.id);
  assert.equal((await api(b, "GET", `/v2/sessions/${session.id}/records?limit=128`)).gaps.length, 0,
    "successor persists verified historical bytes, rather than relying on a routed memory cache");
  assert.equal((await fetch(fixtureURL + "/state").then(r => r.json())).effects, 1);

  await fetch(fixtureURL + "/hold", { method: "POST", body: "{}" });
  const modelSession = await api(b, "POST", "/v2/sessions", { request_id: randomUUID(), config: config({}), message: "one model request" });
  await until(() => fetch(fixtureURL + "/state").then(r => r.json()), value => value.models === 1, "one externally observed model request");
  const [modelOperation] = await until(() => api(b, "GET", `/v2/sessions/${modelSession.id}/operations`),
    values => values.length === 1 && values[0].status === "dispatched", "persisted model dispatch");
  const c = await start("node-c", { interval: "1000" });
  docker(["pause", b.id]); paused.add(b.id);
  await until(() => api(c, "GET", `/v2/sessions/${modelSession.id}/operations/${modelOperation.id}`),
    value => value.status === "unknown", "lost model owner becomes uncertain without resending");
  await until(() => api(c, "GET", `/v2/sessions/${modelSession.id}/runs`),
    runs => runs.length === 1 && runs[0].status === "blocked", "unsupported uncertain model continuation is explicit");
  await remove(b.id);
  const lost = await api(c, "GET", `/v2/sessions/${modelSession.id}/records?limit=128`);
  assert(lost.gaps.some(gap => gap.kind === "unavailable"), "lost written positions cannot be relabeled unused");
  assert.equal((await fetch(fixtureURL + "/state").then(r => r.json())).models, 1);
  await fetch(fixtureURL + "/release", { method: "POST", body: "{}" });
  const restored = await start("node-b", { volume: "node-b", interval: "1000" });
  await until(() => api(c, "GET", `/v2/sessions/${modelSession.id}/records?limit=128`),
    page => page.gaps.length === 0, "returning intact volume restores historical bytes across incarnation change");
  assert.equal((await api(c, "GET", `/v2/sessions/${modelSession.id}/operations/${modelOperation.id}`)).status, "unknown");
  assert.equal((await fetch(fixtureURL + "/state").then(r => r.json())).models, 1, "returning old volume is not fresh model execution permission");
  assert.equal((await fetch(restored.origin + "/health/ready")).status, 204);
  const formerOwner = await start("node-a", { volume: "node-a", interval: "1000" });
  await until(() => api(formerOwner, "GET", "/operator/storage"), report => report.reserved_output_bytes === 0,
    "a restarted former owner reconciles only the already settled and journal-bound result reserve");
  assert((await api(restored, "GET", "/operator/storage")).reserved_output_bytes > 0,
    "unknown model work keeps its output reserve despite old-volume recovery");

  const localSession = await api(restored, "POST", "/v2/sessions", { request_id: randomUUID(), config: config({
    connections: [{ name: "files", kind: "storage", provider: "local_storage", ownership: "managed", capabilities: ["read", "write"] }],
    actions: [{ name: "files", description: "Use the retained namespace", connection: "files", input_schema: { type: "object" },
      max_output_bytes: 8192, timeout_ms: 60000 }],
  }) });
  await api(c, "POST", `/v2/sessions/${localSession.id}/connections`, { request_id: randomUUID(), template: "files" });
  await api(c, "POST", `/v2/sessions/${localSession.id}/inputs`, { request_id: randomUUID(), data: {
    kind: "action", name: "files", arguments: { op: "write", path: "retained.txt", text: "original retained namespace" },
  } });
  await until(() => api(c, "GET", `/v2/sessions/${localSession.id}/runs`),
    runs => runs.length === 1 && runs[0].status === "completed", "local storage is placed on its admitted retained volume");
  await remove(restored.id);
  const replacement = await start("node-b", { volume: "node-b-empty", interval: "1000" });
  const waiting = await api(c, "POST", `/v2/sessions/${localSession.id}/inputs`, { request_id: randomUUID(), data: {
    kind: "action", name: "files", arguments: { op: "read", path: "retained.txt" },
  } });
  await delay(6500);
  assert.equal((await api(c, "GET", `/v2/sessions/${localSession.id}/commands/${waiting.id}`)).status, "accepted",
    "a reused node name and empty volume are not authority to replace a retained storage namespace");
  assert.equal((await api(c, "GET", `/v2/sessions/${localSession.id}/operations`)).length, 1);
  await remove(replacement.id);
  await start("node-b", { volume: "node-b", interval: "1000" });
  const storageOperations = await until(() => api(c, "GET", `/v2/sessions/${localSession.id}/operations`),
    operations => operations.length === 2 && operations.every(operation => operation.status === "completed"),
    "returning original storage satisfies admitted resource affinity");
  const read = storageOperations.find(operation => operation.result?.base64);
  assert.equal(Buffer.from(read.result.base64, "base64").toString("utf8"), "original retained namespace");
  assert.equal((await fetch(fixtureURL + "/state").then(r => r.json())).models, 1);
  await start("wrong-key", { custody: randomBytes(32).toString("hex"), expectedFailure: true });
});

test("J21/J23: real Linux planned handoff verifies archives and fences expired and process-bound targets", { timeout: 180000 }, async t => {
  const { start, api, config, fixtureURL, paused, remove } = await clusterFixture(t);
  const source = await start("transfer-source");
  await fetch(fixtureURL + "/hold", { method: "POST", body: "{}" });
  const session = await api(source, "POST", "/v2/sessions", {
    request_id: "planned-session", config: config({}), message: "original model work",
  });
  const resource = `/v2/sessions/${session.id}`;
  await until(() => fetch(fixtureURL + "/state").then(r => r.json()), value => value.models === 1, "one source model request");
  const target = await start("transfer-target");
  const identify = async name => {
    const node = (await api(source, "GET", "/operator/nodes")).find(value =>
      value.endpoint === `http://${name}:8080` && !value.draining && value.lease_expires_ms > Date.now());
    assert(node);
    return node.id;
  };
  const oldTarget = await identify("transfer-target");
  const firstRequest = { request_id: "interrupted-transfer", owner_epoch: (await api(source, "GET", resource)).owner_epoch, target_node: oldTarget };
  docker(["pause", target.id]); paused.add(target.id);
  const first = await api(source, "POST", `/operator/sessions/${session.id}/transfers`, firstRequest);
  await fetch(fixtureURL + "/release", { method: "POST", body: "{}" });
  await until(() => api(source, "GET", `/operator/sessions/${session.id}/transfers/${first.id}`),
    value => value.phase === "verifying", "source records and freezes its checkpoint while target is paused");
  const failed = await until(() => api(source, "GET", `/operator/sessions/${session.id}/transfers/${first.id}`),
    value => value.phase === "failed", "expired target cannot inherit a partial verification proof");
  assert.equal(failed.error.code, "unavailable");
  assert.equal((await api(source, "GET", resource)).owner_epoch, firstRequest.owner_epoch);
  await until(() => api(source, "GET", resource + "/runs"), runs => runs.length === 1 && runs[0].status === "completed", "source continues its original result after failed handoff");
  assert.equal((await fetch(fixtureURL + "/state").then(r => r.json())).models, 1);
  await remove(target.id);
  const successor = await start("transfer-target");
  const newTarget = await identify("transfer-target");
  assert.notEqual(newTarget, oldTarget, "a replacement process has a different incarnation");
  assert.equal((await api(source, "POST", `/operator/sessions/${session.id}/transfers`, firstRequest)).phase, "failed",
    "a reused node name does not restart an old transfer");
  await fetch(fixtureURL + "/hold", { method: "POST", body: "{}" });
  await api(source, "POST", resource + "/inputs", { request_id: "second-explicit-input", data: { kind: "message", text: "explicit second work" } });
  await until(() => fetch(fixtureURL + "/state").then(r => r.json()), value => value.models === 2, "second explicit model request");
  const request = { request_id: "verified-transfer", owner_epoch: firstRequest.owner_epoch, target_node: newTarget };
  const transfer = await api(source, "POST", `/operator/sessions/${session.id}/transfers`, request);
  assert.equal(transfer.phase, "draining");
  await fetch(fixtureURL + "/release", { method: "POST", body: "{}" });
  const moved = await until(() => api(successor, "GET", `/operator/sessions/${session.id}/transfers/${transfer.id}`), value => {
    assert.notEqual(value.phase, "failed", JSON.stringify(value));
    return value.phase === "transferred";
  }, "target verifies original archive bytes before the live-owner CAS");
  assert(moved.checkpoint);
  assert.equal(moved.verified_runs, 1);
  assert.equal((await fetch(source.origin + "/health/ready")).status, 204, "handoff did not rely on source lease loss");
  await until(() => api(successor, "GET", resource + "/runs"),
    runs => runs.length === 2 && runs.every(run => run.status === "completed"), "new owner consumes both original model results without resending");
  const before = await api(source, "GET", resource + "/records?limit=128");
  await remove(source.id);
  const after = await api(successor, "GET", resource + "/records?limit=128");
  assert.equal(after.gaps.length, 0);
  for (const record of before.records) assert.deepEqual(after.records.find(value => value.sequence === record.sequence), record);
  assert.equal(BigInt((await api(successor, "GET", resource)).owner_epoch), BigInt(request.owner_epoch) + 1n);
  assert.equal((await fetch(fixtureURL + "/state").then(r => r.json())).models, 2);
  const grant = await api(successor, "POST", resource + "/participants", {
    request_id: "browser", subject: "browser", permissions: ["session:read"], expires_ms: Date.now() + 60000,
  });
  await api(successor, "POST", `/operator/sessions/${session.id}/transfers`, { ...request, request_id: "denied" }, 403, grant.token);
  const opaque = await start("opaque-source", { harness: "opaque", continuation: false });
  const opaqueSession = await api(opaque, "POST", "/v2/sessions", { request_id: "opaque", config: config({
    harness: { name: "opaque", version: "1" },
    connections: [{ name: "application", kind: "application", provider: "application", ownership: "borrowed" }],
    actions: [{ name: "deliver", description: "Deliver once", connection: "application", input_schema: { type: "object" },
      max_output_bytes: 8192, timeout_ms: 60000 }],
  }) });
  const opaqueResource = `/v2/sessions/${opaqueSession.id}`;
  await api(opaque, "POST", opaqueResource + "/connections", { request_id: "worker", template: "application" });
  await api(opaque, "POST", opaqueResource + "/inputs", { request_id: "opaque-work", data: { kind: "message", text: "fixture:action:deliver" } });
  await until(() => api(opaque, "GET", opaqueResource + "/operations"), values => values.length === 1 && values[0].status === "prepared", "opaque harness retains its original live process");
  const compatible = await start("opaque-target", { harness: "opaque" });
  const targetInfo = (await api(compatible, "GET", "/operator/nodes")).find(value => value.endpoint === "http://opaque-target:8080");
  const retained = await api(opaque, "GET", opaqueResource);
  const denied = await api(compatible, "POST", `/operator/sessions/${opaqueSession.id}/transfers`, {
    request_id: "no-opaque-resume", owner_epoch: retained.owner_epoch, target_node: targetInfo.id,
  }, 422);
  assert.equal(denied.code, "unsupported");
  assert.equal((await api(opaque, "GET", opaqueResource)).owner_epoch, retained.owner_epoch);
  assert.deepEqual(await fetch(fixtureURL + "/state").then(r => r.json()), { models: 2, effects: 0 });
});
