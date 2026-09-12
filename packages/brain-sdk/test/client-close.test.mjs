import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { Brain, agentloop, brainEnv, component, hostEnv, tool } from "../dist/index.js";

const registration = { host_id: "host_lifecycle", token: "fixture-host-token" };
const state = (id, status = "idle") => ({ session_id: id, status, last_sequence: 1 });
const aborted = { name: "AbortError" };
const untilAbort = (signal) => new Promise((_, reject) => {
  if (signal.aborted) reject(signal.reason);
  else signal.addEventListener("abort", () => reject(signal.reason), { once: true });
});

function fixture(t, { route, run = () => "done" } = {}) {
  const requests = [];
  const connections = [];
  const result = Promise.withResolvers();
  let sessions = 0;
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async (input, init) => {
    const request = new Request(input, init);
    const path = new URL(request.url).pathname;
    requests.push({ path, method: request.method, signal: request.signal });
    const response = await route?.(path, request);
    if (response !== undefined) return response;
    if (path === "/v1/agentloops") return Response.json({ id: "a".repeat(64), status: "admitted" });
    if (path === "/v1/hosts") return Response.json(registration);
    if (path.endsWith("/commands")) {
      const connection = { signal: request.signal, cancelled: false };
      connections.push(connection);
      return new Response(new ReadableStream({
        start(controller) { connection.controller = controller; },
        cancel() { connection.cancelled = true; },
      }));
    }
    if (path.endsWith("/results")) { result.resolve(await request.json()); return new Response(null, { status: 204 }); }
    if (path === "/v1/sessions") return Response.json(state(`session-${++sessions}`));
    if (path.endsWith("/end")) return Response.json(state(path.split("/")[3], "ended"));
    if (path === "/v1/models") return Response.json({ providers: [] });
    throw new Error(`unexpected request ${request.method} ${path}`);
  } });
  t.after(() => client.close());
  const loop = agentloop({ implementation: component(new Uint8Array([1])) });
  const lookup = tool({ name: "lookup", description: "Look up a value.", input: z.object({}), run });
  const options = {
    agentloop: loop({ env: brainEnv({ name: "brain" }) }),
    model: { provider: "openai", name: "gpt-5", apiKey: "fixture-model-token" },
    tools: [lookup({ env: hostEnv({ name: "app" }) })],
  };
  const invoke = (sessionId) => connections.at(-1).controller.enqueue(new TextEncoder().encode(
    `event: command\ndata: ${JSON.stringify({ session_id: sessionId, environment: "app", sequence: 2,
      deadline_at_ms: Date.now() + 10_000, operation: { type: "invoke_tool", name: "lookup", input: {} } })}\n\n`,
  ));
  return { client, options, requests, connections, invoke, result: result.promise };
}

test("close is idempotent before use and rejects new work", async (t) => {
  const f = fixture(t);
  const closing = f.client.close();
  assert.equal(f.client.close(), closing);
  await closing;
  for (const operation of [
    () => f.client.models(), () => f.client.register(), () => f.client.credentials(),
    () => f.client.sessions.create(f.options), () => f.client.admit(f.options.agentloop),
    () => f.client.stream("session").next(),
  ]) await assert.rejects(operation(), aborted);
  assert.throws(() => f.client.withToken("another-token"), aborted);
  assert.deepEqual(f.requests, []);
});

test("a failed create retains the shared stream until client close", async (t) => {
  const f = fixture(t, { route: (path) => path === "/v1/sessions"
    ? Response.json({ code: "invalid_request", message: "creation refused" }, { status: 400 }) : undefined });
  await assert.rejects(f.client.sessions.create(f.options), /creation refused/u);
  assert.equal(f.connections.length, 1);
  assert.equal(f.connections[0].cancelled, false);
  await f.client.close();
  assert.equal(f.connections[0].cancelled, true);
  assert.equal(f.connections[0].signal.aborted, true);
  assert.equal(f.requests.some(request => request.method === "DELETE" || /\/(cancel|end)$/u.test(request.path)), false);
});

test("ending the last session does not stop a concurrent create", async (t) => {
  const entered = Promise.withResolvers();
  const response = Promise.withResolvers();
  let creates = 0;
  const f = fixture(t, { route: (path) => {
    if (path === "/v1/sessions" && ++creates === 2) { entered.resolve(); return response.promise; }
  } });
  const first = await f.client.sessions.create(f.options);
  const pending = f.client.sessions.create(f.options);
  await entered.promise;
  await first.end();
  assert.equal(f.connections[0].signal.aborted, false);
  response.resolve(Response.json(state("second")));
  const second = await pending;
  f.invoke(second.id);
  assert.deepEqual((await f.result).outcome, { status: "ok", value: "done" });
  assert.equal(f.connections.length, 1);
  await assert.rejects((async () => { await f.client.close(); await f.client.admit(f.options.agentloop); })(), aborted);
});

test("a failed reattachment leaves the client usable and closes with it", async (t) => {
  const f = fixture(t, { route: (path) => {
    if (path === "/v1/sessions/retained") return Response.json(state("retained"));
    if (path === "/v1/sessions/retained/events") return Response.json({ events: [{
      sequence: 1, recorded_at_ms: 0, event_type: "session_creation_ended", data: { configuration: {
        environments: [{ name: "other", driver: "host", host_id: registration.host_id }], tools: [],
      } },
    }], next_cursor: 1 });
  } });
  await assert.rejects(f.client.sessions.get("retained", { tools: f.options.tools }), /exactly those/u);
  const session = await f.client.sessions.create(f.options);
  f.invoke(session.id);
  assert.equal((await f.result).outcome.status, "ok");
  assert.equal(f.connections.length, 1);
  await f.client.close();
  assert.equal(f.connections[0].cancelled, true);
});

for (const phase of ["registration", "stream opening"]) {
  test(`close aborts host ${phase} without opening a session`, async (t) => {
    const entered = Promise.withResolvers();
    const f = fixture(t, { route: (path, request) => {
      if (phase === "registration" ? path === "/v1/hosts" : path.endsWith("/commands")) {
        entered.resolve();
        return untilAbort(request.signal);
      }
    } });
    const rejected = assert.rejects(f.client.sessions.create(f.options), aborted);
    await entered.promise;
    await f.client.close();
    await rejected;
    assert.equal(f.requests.some(request => request.path === "/v1/sessions"), false);
  });
}

for (const phase of ["request body", "Component download"]) {
  test(`close aborts a pending ${phase}`, async (t) => {
    const entered = Promise.withResolvers();
    const f = fixture(t, { route: (path, request) => {
      if (path === "/v1/models" || path === "/loop.wasm") return new Response(new ReadableStream({
        start(controller) {
          request.signal.addEventListener("abort", () => controller.error(request.signal.reason), { once: true });
          entered.resolve();
        },
      }));
    } });
    const pending = phase === "request body" ? f.client.models()
      : f.client.admitAgentloop(component(new URL("https://artifacts.example/loop.wasm")));
    const rejected = assert.rejects(pending, aborted);
    await entered.promise;
    await f.client.close();
    await rejected;
    assert.equal(f.requests.some(request => request.path === "/v1/agentloops"), false);
  });
}

test("close cancels a paused event reader and does not end its session", async (t) => {
  let cancelled = false;
  const f = fixture(t, { route: (path) => path.endsWith("/events") ? new Response(new ReadableStream({
    start(controller) { controller.enqueue(new TextEncoder().encode("event: first\ndata: {}\n\nevent: buffered\ndata: {}\n\n")); },
    cancel() { cancelled = true; },
  })) : undefined });
  const events = f.client.stream("retained");
  assert.equal((await events.next()).value.type, "first");
  await f.client.close();
  assert.equal(cancelled, true);
  await assert.rejects(events.next(), aborted);
  assert.equal(f.requests.length, 1);
});

for (const operation of ["events", "results"]) {
  test(`close aborts a host ${operation} post`, async (t) => {
    const entered = Promise.withResolvers();
    let signal;
    const f = fixture(t, {
      route: (path, request) => {
        if (path.endsWith(`/${operation}`)) { signal = request.signal; entered.resolve(); return untilAbort(signal); }
      },
      run: async (_, context) => { if (operation === "events") await context.emit("progress", {}); return "done"; },
    });
    const session = await f.client.sessions.create(f.options);
    f.invoke(session.id);
    await entered.promise;
    await f.client.close();
    assert.equal(signal.aborted, true);
    assert.equal(f.connections[0].cancelled, true);
  });
}

test("close does not await an uncooperative handler or publish its late output", { timeout: 2_000 }, async (t) => {
  const entered = Promise.withResolvers();
  const result = Promise.withResolvers();
  let signal;
  const f = fixture(t, { run: (_, context) => { signal = context.signal; entered.resolve(); return result.promise; } });
  const session = await f.client.sessions.create(f.options);
  f.invoke(session.id);
  await entered.promise;
  await f.client.close();
  assert.equal(signal.aborted, true);
  result.resolve("late result");
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(f.requests.some(request => request.path.endsWith("/results")), false);
});

test("withToken clients have independent lifetimes", async (t) => {
  const f = fixture(t);
  const other = f.client.withToken("other-token");
  t.after(() => other.close());
  await f.client.close();
  assert.deepEqual(await other.models(), { providers: [] });
});
