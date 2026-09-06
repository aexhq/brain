import assert from "node:assert/strict";
import test from "node:test";

import { z } from "zod";
import { Brain, BrainError, agentloop, brainEnv, component, environment, hostEnv, tool } from "../dist/index.js";

const sessionResponse = { session_id: "ses_12345678901234567890", status: "idle", last_sequence: 1 };

test("remote implementations need no Brain admission and a Tool can have two placements", async () => {
  const requests = [];
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async (url, init) => {
    const request = new Request(url, init);
    assert.equal(new URL(request.url).pathname, "/v1/sessions");
    requests.push(await request.json());
    return Response.json(sessionResponse);
  } });
  const remote = environment({ url: () => "https://sandbox.example" });
  const first = remote({ name: "first" });
  const second = remote({ name: "second" });
  const loop = agentloop({ implementation: { runtime: "python", module: "loop.py" } });
  const read = tool({ name: "read", description: "Read", input: z.object({ path: z.string() }), implementation: { runtime: "python", module: "read.py" } });
  const options = { model: { provider: "openai", name: "gpt-5", apiKey: "key" }, agentloop: loop({ env: first }), tools: [read({ env: first }), read({ env: second })] };
  await client.sessions.create(options);
  assert.deepEqual(requests[0].agentloop.implementation, { runtime: "python", module: "loop.py" });
  assert.equal(requests[0].tools.length, 1);
  assert.deepEqual(Object.keys(requests[0].tools[0].placements), ["first", "second"]);
  await assert.rejects(client.sessions.create({ ...options, tools: [read({ env: first }), read({ env: first })] }), /duplicate|already/iu);
});

test("a transcript can be read without sending a turn", async () => {
  const calls = [];
  const brain = new Brain({ baseUrl: "http://localhost:8080", fetch: async (url) => {
    calls.push(String(url));
    return Response.json(String(url).endsWith("/transcript") ? { messages: [], through_sequence: 7 } : sessionResponse);
  } });
  const session = await brain.sessions.get(sessionResponse.session_id);
  assert.deepEqual(await session.transcript(), { messages: [], through_sequence: 7 });
  assert.equal(calls.length, 2);
  assert(calls[1].endsWith("/transcript"));
});

test("session creation admits Components and names every placement", async () => {
  const requests = [];
  const fetchStub = async (input, init) => {
    const request = new Request(input, init);
    requests.push(request);
    if (request.url.endsWith("/v1/agentloops")) return Response.json({ id: "a".repeat(64), status: "admitted" });
    if (request.url.endsWith("/v1/tools")) return Response.json({ id: "b".repeat(64), status: "admitted" });
    if (request.url.endsWith("/v1/sessions")) return Response.json(sessionResponse);
    throw new Error(`unexpected request ${request.url}`);
  };
  const runtime = brainEnv({ name: "brain", secrets: ["MODEL_TOKEN"] });
  const sandbox = environment({
    options: z.object({ url: z.url(), region: z.string(), token: z.string() }),
    url: ({ url }) => url,
    credential: ({ token }) => token,
    configure: ({ region }) => ({ region }),
  })({ name: "sandbox", url: "https://sandbox.example", region: "eu", token: "sandbox-key" });
  const pi = agentloop({ implementation: component(new Uint8Array([1])), needs: ["https://api.example.com"] });
  const read = tool({
    name: "read",
    description: "Read one file.",
    input: z.object({ path: z.string() }),
    implementation: component(new Uint8Array([2])),
    needs: ["file:///workspace"],
  });
  const bash = tool({
    name: "bash",
    description: "Run a command.",
    input: z.object({ command: z.string() }),
    implementation: { artifact: "bash_v1" },
    needs: ["pkg:apt/bash", "file:///workspace?access=write"],
  });
  const client = new Brain({ baseUrl: "https://brain.example/", token: "brain-token", fetch: fetchStub });
  const session = await client.sessions.create({
    model: { provider: "openai", name: "gpt-5", apiKey: "model-token" },
    agentloop: pi({ env: runtime }),
    tools: [read({ env: runtime }), bash({ env: sandbox })],
  });

  assert.equal(session.id, sessionResponse.session_id);
  assert.deepEqual(requests.map((request) => new URL(request.url).pathname), ["/v1/agentloops", "/v1/tools", "/v1/sessions"]);
  assert.equal(requests[0].headers.get("content-type"), "application/octet-stream");
  assert.equal(requests[1].headers.get("content-type"), "application/octet-stream");
  const body = await requests[2].json();
  assert.deepEqual(body.environments, [
    { name: "brain", driver: "brain", configuration: { secrets: ["MODEL_TOKEN"] } },
    { name: "sandbox", driver: "http", url: "https://sandbox.example", credential: "sandbox-key", configuration: { region: "eu" } },
  ]);
  assert.deepEqual(body.agentloop, { implementation: { type: "brain_component", entrypoint: "turn", id: "a".repeat(64) }, configuration: {}, environment: "brain", needs: ["https://api.example.com"] });
  assert.deepEqual(body.tools[0], {
    name: "read",
    description: "Read one file.",
    input_schema: body.tools[0].input_schema,
    placements: { brain: { needs: ["file:///workspace"],
    implementation: { type: "brain_component", entrypoint: "run", id: "b".repeat(64), configuration: {} } } },
  });
  assert.deepEqual(Object.keys(body.tools[1].placements), ["sandbox"]);
  assert.deepEqual(body.tools[1].placements.sandbox.implementation, { artifact: "bash_v1" });
  assert.deepEqual(body.tools[1].placements.sandbox.needs, ["pkg:apt/bash", "file:///workspace?access=write"]);
});

test("one name for two different Environments is refused before any request", async () => {
  const requests = [];
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async (input, init) => {
    const request = new Request(input, init);
    requests.push(request);
    if (request.url.endsWith("/v1/agentloops")) return Response.json({ id: "a".repeat(64), status: "admitted" });
    return Response.json(sessionResponse);
  } });
  const pi = agentloop({ implementation: component(new Uint8Array([1])) });
  const read = tool({ name: "read", description: "Read.", input: z.object({}), implementation: { kind: "test" } });
  await assert.rejects(client.sessions.create({
    model: { provider: "openai", name: "gpt-5", apiKey: "key" },
    agentloop: pi({ env: brainEnv({ name: "brain" }) }),
    tools: [read({ env: brainEnv({ name: "brain", secrets: ["TOKEN"] }) })],
  }), /two Environments are named brain/u);
  assert.equal(requests.length, 0);
  // Two values that say the same thing are one declaration.
  await client.sessions.create({
    model: { provider: "openai", name: "gpt-5", apiKey: "key" },
    agentloop: pi({ env: brainEnv({ name: "brain" }) }),
    tools: [read({ env: brainEnv({ name: "brain" }) })],
  });
  const body = await requests.at(-1).json();
  assert.equal(body.environments.length, 1);
});

test("a Tool with run belongs in a hostEnv and a Tool with an implementation does not", async () => {
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async () => { throw new Error("no request expected"); } });
  const pi = agentloop({ implementation: component(new Uint8Array([1])) });
  const brain = brainEnv({ name: "brain" });
  const lookup = tool({ name: "lookup", description: "Lookup.", input: z.object({}), run: async () => null });
  await assert.rejects(client.sessions.create({
    model: { provider: "openai", name: "gpt-5", apiKey: "key" },
    agentloop: pi({ env: brain }),
    tools: [lookup({ env: brain })],
  }), /must be placed in a hostEnv/u);
  const read = tool({ name: "read", description: "Read.", input: z.object({}), implementation: { kind: "test" } });
  await assert.rejects(client.sessions.create({
    model: { provider: "openai", name: "gpt-5", apiKey: "key" },
    agentloop: pi({ env: brain }),
    tools: [read({ env: hostEnv({ name: "app" }) })],
  }), /must have run/u);
});

test("failed admission is not cached and successful admission is", async () => {
  let calls = 0;
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async () => {
    calls += 1;
    if (calls === 1) return Response.json({ code: "internal", message: "boom", retryable: false }, { status: 500 });
    return Response.json({ id: "a".repeat(64), status: "admitted" });
  } });
  const pi = agentloop({ implementation: component(new Uint8Array([1])) });
  const loop = pi({ env: brainEnv({ name: "brain" }) });
  await assert.rejects(client.admit(loop), (error) => error instanceof BrainError && error.status === 500);
  assert.equal(await client.admit(loop), "a".repeat(64));
  assert.equal(await client.admit(loop), "a".repeat(64));
  assert.equal(calls, 2);
});

test("retrying create with the same key sends the same body", async () => {
  const bodies = [];
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async (input, init) => {
    const request = new Request(input, init);
    if (request.url.endsWith("/v1/agentloops")) return Response.json({ id: "a".repeat(64), status: "admitted" });
    bodies.push(await request.json());
    if (bodies.length === 1) throw new TypeError("response lost after server accepted create");
    return Response.json(sessionResponse);
  } });
  const pi = agentloop({ implementation: component(new Uint8Array([1])) });
  const options = { model: { provider: "openai", name: "gpt-5", apiKey: "key" }, agentloop: pi({ env: brainEnv({ name: "brain" }) }) };
  await assert.rejects(client.sessions.create(options, { idempotencyKey: "create-once" }));
  await client.sessions.create(options, { idempotencyKey: "create-once" });
  assert.deepEqual(bodies[1], bodies[0]);
});

test("long operations have no implicit client timeout", async () => {
  let defaultSignal;
  const client = new Brain({
    baseUrl: "https://brain.example",
    fetch: async (_input, init) => {
      defaultSignal = init.signal;
      return Response.json({ sessions: [] });
    },
  });
  await client.sessions.list();
  assert.equal(defaultSignal, undefined);

  let explicitSignal;
  const bounded = new Brain({
    baseUrl: "https://brain.example",
    timeoutMs: 1_000,
    fetch: async (_input, init) => {
      explicitSignal = init.signal;
      return Response.json({ sessions: [] });
    },
  });
  await bounded.sessions.list();
  assert.ok(explicitSignal instanceof AbortSignal);
});
