import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { Brain, agentloop, brainWasm, component, environment, tool } from "@aexhq/brain";
import { z } from "zod";
import { lazyEnvironment } from "../../examples/lazy-environment.mjs";

const root = await mkdtemp(join(tmpdir(), "brain-runtime-e2e-"));
const listeners = [];
let server;
let clock = 0;
let allocations = 0;
let modelCalls = 0;
let sawExpired = false;
async function listen(handler) {
  const listener = createServer(async (req, res) => {
    try {
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      await handler(req, res, JSON.parse(Buffer.concat(chunks).toString() || "{}"));
    } catch (error) { res.writeHead(500).end(String(error)); }
  });
  listener.listen(0, "127.0.0.1");
  await once(listener, "listening");
  listeners.push(listener);
  return `http://127.0.0.1:${listener.address().port}`;
}
async function stop() {
  if (server && server.exitCode === null) {
    const exited = once(server, "exit");
    server.kill("SIGTERM");
    await exited;
  }
}
async function start(baseUrl) {
  server = spawn(process.env.BRAIN_TEST_SERVER, [], { env: {
    ...process.env, BRAIN_LISTEN: new URL(baseUrl).host, BRAIN_DATA_DIR: join(root, "data"),
    BRAIN_API_TOKEN: "runtime-test", BRAIN_LOOP_WORKER: process.env.BRAIN_TEST_WORKER,
    BRAIN_MODEL_BASE_URL: `${modelUrl}/v1`, BRAIN_ENVIRONMENT_ROUTES_FILE: join(root, "routes.json"),
  }, stdio: ["ignore", "ignore", "inherit"] });
  for (let i = 0; i < 100; i++) {
    if (server.exitCode !== null) throw new Error(`server exited: ${server.exitCode}`);
    try { if ((await fetch(`${baseUrl}/health/ready`)).ok) return; } catch {}
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error("server did not become ready");
}
async function memory() {
  const children = (await readFile(`/proc/${server.pid}/task/${server.pid}/children`, "utf8")).trim().split(/\s+/u).filter(Boolean);
  let privateKiB = 0;
  for (const pid of [server.pid, ...children]) {
    const rollup = await readFile(`/proc/${pid}/smaps_rollup`, "utf8");
    for (const match of rollup.matchAll(/^Private_(?:Clean|Dirty):\s+(\d+)/gmu)) privateKiB += Number(match[1]);
  }
  return privateKiB;
}
let modelUrl;
try {
  const routes = {};
  for (const driver of ["first", "second"]) {
    const provider = lazyEnvironment({ now: () => clock, allocate: async () => { allocations++; return new Map(); } });
    routes[driver] = { api_key: driver, endpoint: await listen(async (req, res, command) => {
      assert.equal(req.headers.authorization, `Bearer ${driver}`);
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(await provider.handle(command)));
    }) };
  }
  await writeFile(join(root, "routes.json"), JSON.stringify(routes));
  modelUrl = await listen(async (_req, res, request) => {
    modelCalls++;
    const last = request.messages.at(-1);
    const done = last.role === "tool";
    if (done && request.messages.slice(-2).some((m) => JSON.stringify(m).includes("expired"))) sawExpired = true;
    const delta = done ? { content: "done" } : { tool_calls: ["first", "second"].map((name, index) => ({
      index, id: `${name}-${modelCalls}`, type: "function", function: { name, arguments: JSON.stringify({ value: "hello" }) },
    })) };
    res.writeHead(200, { "content-type": "text/event-stream" }).end(`data: ${JSON.stringify({ choices: [{ index: 0, delta, finish_reason: done ? "stop" : "tool_calls" }] })}\n\ndata: [DONE]\n\n`);
  });
  const reservation = createServer();
  reservation.listen(0, "127.0.0.1"); await once(reservation, "listening");
  const baseUrl = `http://127.0.0.1:${reservation.address().port}`;
  await new Promise((resolve) => reservation.close(resolve));
  await start(baseUrl);
  const brain = new Brain({ baseUrl, token: "runtime-test" });
  const artifact = component(pathToFileURL(process.env.BRAIN_TEST_REFERENCE_AGENTLOOP));
  await brain.admitAgentloop(artifact);
  const loop = agentloop({ implementation: artifact });
  const tools = ["first", "second"].map((driver) => tool({ name: driver, input: z.object({ value: z.string() }),
    implementation: { type: "reference_echo" }, description: "Echo input" })({ env: environment({ driver })() }));
  const options = { model: { provider: "vercel-ai-gateway", name: "test/scripted", apiKey: "test" }, agentloop: loop({ env: brainWasm() }), tools };
  const session = await brain.sessions.create(options);
  assert.equal(allocations, 0, "logical setup/attach must not allocate");
  await session.send("use both environments");
  assert.equal(allocations, 2);
  assert.equal((await session.transcript()).messages.length, 4);
  clock = 60_000;
  await session.send("use both again");
  assert.equal(allocations, 2, "expiration must not trigger allocation or retry");
  assert.ok(sawExpired, "environment failure must reach the model");
  const calls = modelCalls;
  const before = await session.transcript();
  await stop();
  await start(baseUrl);
  assert.deepEqual(await session.transcript(), before);
  assert.equal(modelCalls, calls, "restart and history reads must not activate a loop");
  const baseline = await memory();
  const histories = [];
  for (let i = 0; i < 32; i++) {
    const retained = await brain.sessions.create(options);
    for (let turn = 0; turn < 8; turn++) await retained.send(`turn ${turn}: ${"context ".repeat(128)}`);
    histories.push(retained);
  }
  for (const retained of histories) assert.equal((await retained.transcript()).messages.length, 32);
  const after = await memory();
  assert.ok(after - baseline < 64 * 1024, `retained histories grew private memory by ${after - baseline} KiB`);
  console.log(JSON.stringify({ sessions: histories.length, turnsPerSession: 8, serverAndWorkerPrivateKiB: { baseline, after } }));
} finally {
  await stop();
  await Promise.all(listeners.map((listener) => new Promise((resolve) => listener.close(resolve))));
  await rm(root, { recursive: true, force: true });
}
