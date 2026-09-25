import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { once } from "node:events";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { before, beforeEach, after } from "node:test";
import { pathToFileURL } from "node:url";
import { Brain, BrainError, agentloop, brainEnv, component, environment } from "@aexhq/brain";
import { z } from "zod";

export const collect = async (events) => { const result = []; for await (const event of events) result.push(event); return result; };
export const deferred = () => Promise.withResolvers();
export const failure = (status) => (error) => error instanceof BrainError && error.status === status;
export const text = (message) => message.content.filter((block) => block.type === "text").map((block) => block.text).join("");

export function reply(response, content = "answered") {
  const frames = [
    { type: "response.output_text.delta", output_index: 0, delta: content },
    { type: "response.output_item.done", output_index: 0, item: { type: "message", content: [{ type: "output_text", text: content }] } },
    { type: "response.completed", response: { output: [] } },
  ];
  response.writeHead(200, { "content-type": "text/event-stream" });
  response.end(frames.map(frame => `data: ${JSON.stringify(frame)}\n\n`).join(""));
}

export function callTools(response, calls) {
  const output = calls.map(({ name, input }) => ({ type: "function_call", call_id: randomUUID(), name, arguments: JSON.stringify(input) }));
  const frames = output.map((item, output_index) => ({ type: "response.output_item.done", output_index, item }));
  frames.push({ type: "response.completed", response: { output } });
  response.writeHead(200, { "content-type": "text/event-stream" });
  response.end(frames.map(frame => `data: ${JSON.stringify(frame)}\n\n`).join(""));
}

export function fixture({ providers = {} } = {}) {
  const f = { modelRequests: [], model: (_request, response) => reply(response) };
  let child;
  let upstream;
  let directory;
  let logs = "";
  const clients = new Set();
  beforeEach(() => {
    f.modelRequests.length = 0;
    f.model = (_request, response) => reply(response);
  });
  before(async () => {
    for (const name of ["BRAIN_TEST_SERVER", "BRAIN_TEST_WORKER", "BRAIN_TEST_REFERENCE_AGENTLOOP", "BRAIN_TEST_AGENTLOOP_PACKAGE", "BRAIN_TEST_TOOL_COMPONENT"]) {
      assert.ok(process.env[name], `${name} is required; see tests/journeys/README.md`);
    }
    directory = await mkdtemp(join(tmpdir(), "brain-sdk-journey-"));
    f.token = randomUUID();
    f.reference = component(pathToFileURL(process.env.BRAIN_TEST_REFERENCE_AGENTLOOP));
    f.diagnostic = component(pathToFileURL(process.env.BRAIN_TEST_AGENTLOOP_PACKAGE));
    f.toolComponent = component(pathToFileURL(process.env.BRAIN_TEST_TOOL_COMPONENT));
    upstream = createServer(async (request, response) => {
      try {
        if (request.url === "/artifact.wasm") {
          response.end(await readFile(process.env.BRAIN_TEST_REFERENCE_AGENTLOOP));
          return;
        }
        const chunks = [];
        for await (const chunk of request) chunks.push(chunk);
        const body = JSON.parse(Buffer.concat(chunks).toString() || "{}");
        const provider = request.url.split("/")[1];
        if (providers[provider]) {
          assert.equal(request.headers.authorization, `Bearer ${provider}-token`);
          response.setHeader("content-type", "application/json");
          response.end(JSON.stringify(await providers[provider](body)));
        } else {
          assert.equal(request.headers.authorization, "Bearer journey-model-token");
          f.modelRequests.push(body);
          await f.model(body, response);
        }
      } catch (error) {
        response.writeHead(500).end(String(error));
      }
    });
    upstream.listen(0, "127.0.0.1");
    await once(upstream, "listening");
    f.upstreamUrl = `http://127.0.0.1:${upstream.address().port}`;
    // Each provider is an Environment extension: the application configures every
    // instance with the address it is reached at and the credential it expects.
    f.provider = (name, options = {}) => environment({
      options: z.object({ label: z.string().optional() }),
      url: () => `${f.upstreamUrl}/${name}`,
      credential: () => `${name}-token`,
    })({ name, ...options });
    const reservation = createServer();
    reservation.listen(0, "127.0.0.1");
    await once(reservation, "listening");
    f.baseUrl = `http://127.0.0.1:${reservation.address().port}`;
    await new Promise((resolve) => reservation.close(resolve));
    await f.start();
    f.brain = f.client();
    f.options = (extra = {}) => ({
      model: { provider: "vercel-ai-gateway", name: "test/journey", apiKey: "journey-model-token" },
      agentloop: agentloop({ implementation: f.reference })({ env: brainEnv({ name: "brain" }) }),
      ...extra,
    });
  }, { timeout: 60_000 });
  f.client = (options = {}) => {
    const client = new Brain({ baseUrl: f.baseUrl, token: f.token, ...options });
    clients.add(client);
    return client;
  };
  f.start = async () => {
    child = spawn(process.env.BRAIN_TEST_SERVER, [], { env: {
      ...process.env,
      BRAIN_LISTEN: new URL(f.baseUrl).host, BRAIN_DATA_DIR: join(directory, "data"),
      BRAIN_API_TOKEN: f.token, BRAIN_ENV_WORKER: process.env.BRAIN_TEST_WORKER,
      BRAIN_MODEL_BASE_URL: `${f.upstreamUrl}/v1`,
      BRAIN_ENV_FILESYSTEM_ALLOW: "scratch,workspace",
    }, detached: true, stdio: ["ignore", "pipe", "pipe"] });
    child.stdout.on("data", (chunk) => { logs += chunk; });
    child.stderr.on("data", (chunk) => { logs += chunk; });
    for (let attempt = 0; attempt < 1200; attempt++) {
      if (child.exitCode !== null) throw new Error(`Brain exited: ${logs}`);
      try { if ((await fetch(`${f.baseUrl}/health/ready`)).ok) return; } catch {}
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    throw new Error(`Brain readiness timed out: ${logs}`);
  };
  f.stop = async (signal = "SIGTERM") => {
    if (child && child.exitCode === null) {
      const exited = once(child, "exit");
      if (signal === "SIGTERM") child.kill(signal);
      else process.kill(-child.pid, signal);
      await exited;
    }
  };
  f.create = async (t, extra = {}, client = f.brain, operation) => {
    const session = await client.sessions.create(f.options(extra), operation);
    t.after(async () => {
      try {
        const retained = await f.brain.sessions.get(session.id);
        await retained.end();
        await retained.delete();
      } catch (error) { if (!failure(404)(error)) throw error; }
    }, { timeout: 10_000 });
    return session;
  };
  after(async () => {
    await Promise.all([...clients].map((client) => client.close()));
    await f.stop();
    if (upstream) {
      upstream.closeAllConnections();
      await new Promise((resolve) => upstream.close(resolve));
    }
    if (directory) await rm(directory, { recursive: true, force: true });
  });
  return f;
}
