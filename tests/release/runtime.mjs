import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { Brain, agentloop, brainEnv, component, environment, tool } from "@aexhq/brain";
import { z } from "zod";
import { lazyEnvironment } from "../../examples/lazy-environment.mjs";

const root = await mkdtemp(join(tmpdir(), "brain-runtime-e2e-"));
const listeners = [];
let server;
let allocations = 0;
let modelCalls = 0;
let sawUnavailable = false;
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
    BRAIN_API_TOKEN: "runtime-test", BRAIN_ENV_WORKER: process.env.BRAIN_TEST_WORKER,
    BRAIN_MODEL_BASE_URL: `${modelUrl}/v1`,
  }, stdio: ["ignore", "inherit", "inherit"] });
  for (let i = 0; i < 1200; i++) {
    if (server.exitCode !== null) throw new Error(`server exited: ${server.exitCode}`);
    try { if ((await fetch(`${baseUrl}/health/ready`)).ok) return; } catch {}
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error("server did not become ready");
}
let modelUrl;
let runtimeBaseUrl;
try {
  const providers = {};
  const control = {};
  for (const name of ["first", "second"]) {
    const provider = lazyEnvironment({ allocate: async () => { allocations++; return new Map(); } });
    control[name] = provider;
    providers[name] = await listen(async (req, res, command) => {
      assert.equal(req.headers.authorization, `Bearer ${name}`);
      if (command.operation.request.type === "setup" && modelCalls === 0) {
        const listed = await fetch(`${runtimeBaseUrl}/v1/sessions`, { headers: { authorization: "Bearer runtime-test" } });
        assert.equal(listed.status, 200);
        const row = (await listed.json()).sessions.find((row) => row.session_id === command.operation.session_id);
        assert.equal(row.status, "creating", "listing during setup must share the live creation store");
      }
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(await provider.handle(command)));
    });
  }
  modelUrl = await listen(async (_req, res, request) => {
    modelCalls++;
    const done = request.input.at(-1).type === "function_call_output";
    if (done && request.input.slice(-2).some(m => JSON.stringify(m).includes("unavailable"))) sawUnavailable = true;
    const output = done ? [] : ["first", "second"].map(name => ({ type: "function_call", call_id: `${name}-${modelCalls}`, name, arguments: JSON.stringify({ value: "hello" }) }));
    const frames = done ? [
      { type: "response.output_text.delta", output_index: 0, delta: "done" },
      { type: "response.output_item.done", output_index: 0, item: { type: "message", content: [{ type: "output_text", text: "done" }] } },
    ] : output.map((item, output_index) => ({ type: "response.output_item.done", output_index, item }));
    frames.push({ type: "response.completed", response: { output } });
    res.writeHead(200, { "content-type": "text/event-stream" }).end(frames.map(frame => `data: ${JSON.stringify(frame)}\n\n`).join(""));
  });
  const reservation = createServer();
  reservation.listen(0, "127.0.0.1"); await once(reservation, "listening");
  const baseUrl = `http://127.0.0.1:${reservation.address().port}`;
  await new Promise((resolve) => reservation.close(resolve));
  runtimeBaseUrl = baseUrl;
  await start(baseUrl);
  const brain = new Brain({ baseUrl, token: "runtime-test" });
  const artifact = component(pathToFileURL(process.env.BRAIN_TEST_REFERENCE_AGENTLOOP));
  await brain.admitAgentloop(artifact);
  const loop = agentloop({ implementation: artifact });
  const provider = environment({ options: z.object({ url: z.url(), token: z.string() }), url: ({ url }) => url, credential: ({ token }) => token, configure: () => ({}) });
  const tools = ["first", "second"].map((name) => tool({ name, input: z.object({ value: z.string() }),
    implementation: { type: "reference_echo" }, description: "Echo input" })({ env: provider({ name, url: providers[name], token: name }) }));
  const options = { model: { provider: "vercel-ai-gateway", name: "test/scripted", apiKey: "test" }, agentloop: loop({ env: brainEnv({ name: "brain" }) }), tools };
  const session = await brain.sessions.create(options);
  assert.equal(allocations, 0, "logical setup must not allocate");
  await session.send("use both environments");
  assert.equal(allocations, 2);
  assert.equal((await session.transcript()).messages.length, 4);
  // The caller ends the provider resource; elapsed time is not Environment policy.
  for (const [environment, provider] of Object.entries(control)) {
    await provider.handle({ contract: "environment/v1", operation: { session_id: session.id, environment, sequence: 999, request: { type: "teardown" } } });
  }
  await session.send("use both again");
  assert.equal(allocations, 2, "resource loss must not trigger allocation or retry");
  assert.ok(sawUnavailable, "environment failure must reach the model");
  const calls = modelCalls;
  const before = await session.transcript();
  await stop();
  await start(baseUrl);
  assert.deepEqual(await session.transcript(), before);
  assert.equal(modelCalls, calls, "restart and history reads must not activate a loop");
  const histories = [];
  for (let i = 0; i < 2; i++) {
    const retained = await brain.sessions.create(options);
    for (let turn = 0; turn < 2; turn++) await retained.send(`turn ${turn}`);
    histories.push(retained);
  }
  for (const retained of histories) assert.equal((await retained.transcript()).messages.length, 8);
  console.log("lazy providers, caller teardown, restart, and suspended transcripts passed");
} finally {
  await stop();
  await Promise.all(listeners.map((listener) => new Promise((resolve) => listener.close(resolve))));
  await rm(root, { recursive: true, force: true });
}
