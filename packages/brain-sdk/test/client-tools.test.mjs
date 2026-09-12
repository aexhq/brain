import assert from "node:assert/strict";
import test from "node:test";

import { z } from "zod";
import { HostToolRegistry } from "../dist/host.js";
import { HostPump } from "../dist/client-pump.js";
import { Brain, agentloop, brainEnv, component, hostEnv, tool } from "../dist/index.js";

const sessionId = "ses_12345678901234567890";
const sse = (data) => `event: command\ndata: ${JSON.stringify(data)}\n\n`;
const model = { provider: "openai", name: "gpt-5", apiKey: "model-token" };

test("saved host credentials reattach handlers to an existing session", async () => {
  const credentials = { hostId: "host_12345678901234567890", token: "saved-token" };
  let controller;
  let finish;
  const result = new Promise((resolve) => { finish = resolve; });
  const client = new Brain({ baseUrl: "https://brain.example", credentials, fetch: async (input, init) => {
    const request = new Request(input, init);
    const path = new URL(request.url).pathname;
    if (path.endsWith("/commands")) {
      assert.equal(request.headers.get("authorization"), "Bearer saved-token");
      return new Response(new ReadableStream({ start(value) { controller = value; } }), { headers: { "content-type": "text/event-stream" } });
    }
    if (path.endsWith("/results")) { finish(await request.json()); return new Response(null, { status: 204 }); }
    if (path.endsWith("/events")) {
      return Response.json({ events: [{ sequence: 1, recorded_at_ms: 0, event_type: "session_creation_ended", data: { configuration: {
        tools: [{ name: "lookup", placements: { app: { implementation: { type: "host_function", name: "lookup" } } } }],
        environments: [{ name: "brain", driver: "brain" }, { name: "app", driver: "host", host_id: credentials.hostId }],
      } } }], next_cursor: 1 });
    }
    if (path.endsWith("/end")) return Response.json({ session_id: sessionId, environment: "app", status: "ended", last_sequence: 4 });
    if (path === `/v1/sessions/${sessionId}`) return Response.json({ session_id: sessionId, environment: "app", status: "idle", last_sequence: 1 });
    throw new Error(`unexpected request ${path}`);
  } });
  const lookup = tool({ name: "lookup", description: "Lookup.", input: z.object({}), run: async () => "restored" });
  const session = await client.sessions.get(sessionId, { tools: [lookup({ env: hostEnv({ name: "app" }) })] });
  assert.deepEqual(await client.credentials(), credentials);
  controller.enqueue(new TextEncoder().encode(sse({ session_id: sessionId, environment: "app", sequence: 2, deadline_at_ms: Date.now() + 5000, operation: { type: "invoke_tool", name: "lookup", input: {} } })));
  assert.deepEqual((await result).outcome, { status: "ok", value: "restored" });
  await session.end();
});

test("one host runs the Tools placed in it and commits ctx.emit before its result", async () => {
  const requests = [];
  let releaseStream;
  let releaseCommand;
  const completed = new Promise((resolve) => { releaseStream = resolve; });
  const sessionCreated = new Promise((resolve) => { releaseCommand = resolve; });
  const fetchStub = async (input, init) => {
    const request = new Request(input, init);
    requests.push(request);
    const path = new URL(request.url).pathname;
    if (path === "/v1/agentloops") return Response.json({ id: "a".repeat(64), status: "admitted" });
    if (path === "/v1/hosts") return Response.json({ host_id: "host_12345678901234567890", token: "host-token" });
    if (path.endsWith("/commands")) {
      return new Response(new ReadableStream({
        async start(controller) {
          await sessionCreated;
          controller.enqueue(new TextEncoder().encode(sse({
            session_id: sessionId, environment: "app",
            sequence: 3,
            deadline_at_ms: Date.now() + 5_000,
            operation: { type: "invoke_tool", name: "lookup", input: { id: "1" } },
          })));
          completed.then(() => controller.close());
        },
      }), { headers: { "content-type": "text/event-stream" } });
    }
    if (path.endsWith("/events")) return Response.json({ sequence: 4 });
    if (path.endsWith("/results")) {
      releaseStream();
      return new Response(null, { status: 204 });
    }
    if (path === "/v1/sessions") {
      setTimeout(() => releaseCommand(), 0);
      return Response.json({ session_id: sessionId, environment: "app", status: "idle", last_sequence: 1 });
    }
    if (path === `/v1/sessions/${sessionId}` && request.method === "DELETE") return new Response(null, { status: 204 });
    throw new Error(`unexpected request ${request.method} ${path}`);
  };
  const lookup = tool({
    name: "lookup",
    description: "Look up one value.",
    input: z.object({ id: z.string() }),
    run: async ({ id }, ctx) => {
      assert.equal(ctx.sequence, 3);
      assert.equal(await ctx.emit("lookup_progress", { id }), 4);
      return { id };
    },
  });
  const pi = agentloop({ implementation: component(new Uint8Array([1])) });
  const client = new Brain({ baseUrl: "https://brain.example", token: "brain-token", fetch: fetchStub });
  const app = hostEnv({ name: "app" });
  const session = await client.sessions.create({
    model,
    agentloop: pi({ env: brainEnv({ name: "brain" }) }),
    tools: [lookup({ env: app })],
  });
  await completed;
  const createRequest = requests.find((request) => new URL(request.url).pathname === "/v1/sessions");
  const body = await createRequest.json();
  assert.deepEqual(body.environments[1], { name: "app", driver: "host", host_id: "host_12345678901234567890", configuration: {} });
  assert.deepEqual(body.tools[0].placements.app, { implementation: { type: "host_function", name: "lookup" } });
  assert.equal("implementation" in body.tools[0], false);
  const eventRequest = requests.find((request) => new URL(request.url).pathname.endsWith("/events"));
  const resultRequest = requests.find((request) => new URL(request.url).pathname.endsWith("/results"));
  assert.ok(eventRequest);
  assert.ok(resultRequest);
  assert.equal(eventRequest.headers.get("authorization"), "Bearer host-token");
  assert.deepEqual(await eventRequest.json(), { session_id: sessionId, sequence: 3, event_type: "lookup_progress", data: { id: "1" } });
  assert.deepEqual(await resultRequest.json(), { session_id: sessionId, sequence: 3, outcome: { status: "ok", value: { id: "1" } } });
  await session.delete();
});

test("session creation waits for the host command stream", async () => {
  const paths = [];
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async (input, init) => {
    const request = new Request(input, init);
    const path = new URL(request.url).pathname;
    paths.push(path);
    if (path === "/v1/agentloops") return Response.json({ id: "a".repeat(64), status: "admitted" });
    if (path === "/v1/hosts") return Response.json({ host_id: "host_12345678901234567890", token: "host-token" });
    if (path.endsWith("/commands")) return Response.json({ code: "unavailable", message: "offline", retryable: true }, { status: 503 });
    if (path === "/v1/sessions") throw new Error("session create raced the host connection");
    throw new Error(`unexpected request ${request.method} ${path}`);
  } });
  const pi = agentloop({ implementation: component(new Uint8Array([1])) });
  const local = tool({
    name: "local",
    description: "Run here.",
    input: z.object({}),
    run: async () => null,
  });
  await assert.rejects(client.sessions.create({
    model,
    agentloop: pi({ env: brainEnv({ name: "brain" }) }),
    tools: [local({ env: hostEnv({ name: "app" }) })],
  }), /offline/u);
  assert.equal(paths.includes("/v1/sessions"), false);
});

test("a host reconnects without replaying old commands", async () => {
  let connections = 0;
  let finish;
  const finished = new Promise((resolve) => { finish = resolve; });
  const pump = new HostPump({
    stream: async function* (signal, onOpen) {
      connections += 1;
      onOpen?.();
      if (connections === 1) return;
      yield {
        type: "command",
        data: {
          session_id: sessionId, environment: "app",
          sequence: 9,
          deadline_at_ms: Date.now() + 5_000,
          operation: { type: "invoke_tool", name: "lookup", input: { id: "2" } },
        },
      };
      await new Promise((resolve) => {
        if (signal?.aborted) resolve();
        else signal?.addEventListener("abort", resolve, { once: true });
      });
    },
    result: async (result) => finish(result),
    emit: async () => ({ sequence: 10 }),
  });
  const registry = new HostToolRegistry();
  registry.register("app", {
    name: "lookup",
    description: "Look up one value.",
    input: z.object({ id: z.string() }),
  }, async ({ id }) => ({ id }));
  pump.register(sessionId, registry);

  await pump.start();
  const result = await finished;
  assert.equal(connections, 2);
  assert.deepEqual(result.outcome, { status: "ok", value: { id: "2" } });
  pump.unregister(sessionId);
  await pump.closed;
});

test("losing the command stream reports an in-flight call as unknown without replay", async () => {
  let calls = 0;
  let finish;
  const finished = new Promise(resolve => { finish = resolve; });
  const pump = new HostPump({
    stream: async function* (_signal, onOpen) {
      onOpen();
      yield { type: "command", data: {
        session_id: sessionId, environment: "app", sequence: 9,
        deadline_at_ms: Date.now() + 5_000,
        operation: { type: "invoke_tool", name: "lookup", input: {} },
      } };
    },
    result: async result => { finish(result); pump.stop(); },
    emit: async () => ({ sequence: 10 }),
  });
  const registry = new HostToolRegistry();
  registry.register("app", { name: "lookup", description: "Lookup.", input: z.object({}) }, () => {
    calls += 1;
    return new Promise(() => {});
  });
  pump.register(sessionId, registry);
  await pump.start();
  assert.equal((await finished).outcome.status, "unknown");
  await pump.closed;
  assert.equal(calls, 1);
});

test("a new session reconnects the durable host after its previous stream stopped", async () => {
  let hosts = 0;
  let sessions = 0;
  const placements = [];
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async (input, init) => {
    const request = new Request(input, init);
    const path = new URL(request.url).pathname;
    if (path === "/v1/agentloops") {
      return Response.json({ id: "a".repeat(64), status: "admitted" });
    }
    if (path === "/v1/hosts") {
      hosts += 1;
      return Response.json({ host_id: `host_${hosts}`, token: `token_${hosts}` });
    }
    if (path.endsWith("/commands")) {
      return new Response(new ReadableStream({
        start(controller) {
          request.signal.addEventListener("abort", () => controller.close(), { once: true });
        },
      }), { headers: { "content-type": "text/event-stream" } });
    }
    if (path === "/v1/sessions") {
      sessions += 1;
      const body = await request.json();
      placements.push(body.environments.find((environment) => environment.driver === "host").host_id);
      return Response.json({ session_id: `ses_${sessions}`, status: "idle", last_sequence: 1 });
    }
    if (path.endsWith("/end")) {
      return Response.json({ session_id: path.split("/")[3], status: "ended", last_sequence: 2 });
    }
    throw new Error(`unexpected request ${request.method} ${path}`);
  } });
  const pi = agentloop({ implementation: component(new Uint8Array([1])) });
  const local = tool({
    name: "local",
    description: "Run here.",
    input: z.object({}),
    run: async () => null,
  });
  const options = {
    model,
    agentloop: pi({ env: brainEnv({ name: "brain" }) }),
    tools: [local({ env: hostEnv({ name: "app" }) })],
  };

  const first = await client.sessions.create(options);
  await first.end();
  const second = await client.sessions.create(options);
  await second.end();

  assert.equal(hosts, 1);
  assert.deepEqual(placements, ["host_1", "host_1"]);
});
