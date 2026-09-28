import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:http";
import test from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { Brain, agentloop, brainEnv, tool } from "../dist/index.js";
import { z } from "zod";

const frame = (type, data) => new TextEncoder().encode(`event: ${type}\ndata: ${JSON.stringify(data)}\n\n`);
const state = id => ({ session_id: id, status: "idle", last_sequence: 1 });

function fixture(t, { timeout = 15, suspend, message, create } = {}) {
  const streams = [];
  const requests = [];
  const results = [];
  let sequence = 1;
  let idle = true;
  let denied = false;
  let finished = Promise.withResolvers();
  const client = new Brain({ baseUrl: "https://brain.example", connectionIdleTimeoutMs: timeout, fetch: async (url, init) => {
    const request = new Request(url, init);
    const path = new URL(url).pathname;
    requests.push(path);
    if (path === "/v1/hosts") return Response.json({ host_id: "host_idle", token: "fixture" });
    if (path.endsWith("/commands")) {
      if (denied) return Response.json({ code: "unauthorized", message: "grant expired" }, { status: 401 });
      const stream = { generation: streams.length + 1, closed: false };
      streams.push(stream);
      return new Response(new ReadableStream({
        start(controller) {
          stream.controller = controller;
          controller.enqueue(frame("activity", { connection: stream.generation, idle }));
          request.signal.addEventListener("abort", () => { if (!stream.closed) { stream.closed = true; controller.close(); } }, { once: true });
        },
        cancel() { stream.closed = true; },
      }));
    }
    if (path.endsWith("/suspend")) {
      const input = await request.json();
      const stream = streams.at(-1);
      assert.equal(input.connection, stream.generation);
      const accepted = await (suspend?.(stream, request) ?? idle);
      if (accepted && !stream.closed) { stream.closed = true; stream.controller.close(); }
      if (accepted) { finished.resolve(); finished = Promise.withResolvers(); }
      return Response.json({ suspended: accepted });
    }
    if (path === "/v1/sessions") return create?.() ?? Response.json(state(`session-${sequence++}`));
    if (path.endsWith("/messages")) {
      assert.equal(streams.at(-1).closed, false, "execution dispatched before reconnect");
      return await message?.(path) ?? Response.json(request.headers.has("prefer") ? sequence++ : state(path.split("/")[3]));
    }
    if (path.endsWith("/results")) { results.push(await request.json()); return Response.json({ sequence: sequence++ }); }
    if (path.endsWith("/transcript")) return Response.json({ messages: [] });
    if (path.endsWith("/end")) return Response.json({ ...state(path.split("/")[3]), status: "ended" });
    if (request.method === "DELETE") return new Response(null, { status: 204 });
    throw new Error(`unexpected ${path}`);
  } });
  t.after(() => client.close());
  const lookup = tool({ name: "lookup", description: "Look up", input: z.object({}), run: (_, ctx) => ctx.finish("retained closure") });
  const options = {
    agentloop: agentloop({ implementation: { type: "fixture" } })({ env: brainEnv({ name: "brain" }) }),
    tools: [lookup()], model: { provider: "openai", name: "gpt-5", apiKey: "fixture" },
  };
  return {
    client, options, streams, requests, results,
    sleeping: () => finished.promise,
    deny: () => { denied = true; },
    activity(value) {
      idle = value;
      const stream = streams.at(-1);
      stream.controller.enqueue(frame("activity", { connection: stream.generation, idle }));
    },
    invoke(session) {
      streams.at(-1).controller.enqueue(frame("command", { session_id: session.id, environment: "brain-sdk-host", sequence: sequence++, operation: { type: "invoke_tool", name: "lookup", input: {} } }));
    },
  };
}

test("idle suspension retains sessions, reconnects once and preserves callbacks", { timeout: 3000 }, async t => {
  const f = fixture(t);
  const first = await f.client.sessions.create(f.options);
  const second = await f.client.sessions.create(f.options);
  const credentials = await f.client.credentials();
  await f.sleeping();
  await first.transcript();
  assert.equal(f.streams.length, 1);
  await Promise.all([first.send("wake"), second.submit("also wake")]);
  assert.equal(f.streams.length, 2);
  assert.deepEqual(await f.client.credentials(), credentials);
  f.activity(false);
  f.invoke(first);
  while (!f.results.length) await delay(1);
  assert.equal(f.results.at(-1).update.outcome.value, "retained closure");
  await first.end();
  f.invoke(first);
  while (f.results.length < 2) await delay(1);
  assert.equal(f.results.at(-1).update.outcome.error.code, "unknown_session");
  await second.end();
  await second.delete();
});

test("server activity keeps a shared connection alive after submit returns", { timeout: 3000 }, async t => {
  const f = fixture(t);
  const session = await f.client.sessions.create(f.options);
  f.activity(false);
  await session.submit("background work");
  await delay(60);
  assert.equal(f.requests.some(path => path.endsWith("/suspend")), false);
  f.activity(true);
  await f.sleeping();
  assert.equal(f.streams[0].closed, true);
});

test("new sends wait for a pending suspension before opening their stream", { timeout: 3000 }, async t => {
  const entered = Promise.withResolvers();
  const release = Promise.withResolvers();
  const f = fixture(t, { suspend: () => { entered.resolve(); return release.promise; } });
  const session = await f.client.sessions.create(f.options);
  await entered.promise;
  const pending = session.send("racing wake");
  await delay(5);
  assert.equal(f.requests.some(path => path.endsWith("/messages")), false);
  release.resolve(true);
  await pending;
  assert.equal(f.streams.length, 2);
});

test("a busy cutover retains the stream and a later idle opportunity suspends it", { timeout: 3000 }, async t => {
  let attempts = 0;
  const f = fixture(t, { suspend: () => ++attempts > 1 });
  await f.client.sessions.create(f.options);
  await f.sleeping();
  assert.equal(attempts, 2);
  assert.equal(f.streams.length, 1);
});

test("failed creation still releases its idle connection without fabricating cleanup", { timeout: 3000 }, async t => {
  const f = fixture(t, { create: () => Response.json({ code: "invalid_request", message: "refused" }, { status: 400 }) });
  await assert.rejects(f.client.sessions.create(f.options), /refused/u);
  await f.sleeping();
  assert.equal(f.requests.some(path => /\/(end|cancel)$/u.test(path)), false);
});

test("expired access fails wake before submitting a message", { timeout: 3000 }, async t => {
  const f = fixture(t);
  const session = await f.client.sessions.create(f.options);
  await f.sleeping();
  f.deny();
  await assert.rejects(session.send("cannot wake"), /grant expired/u);
  assert.equal(f.requests.some(path => path.endsWith("/messages")), false);
});

test("keep-alive mode and timeout validation are explicit", async t => {
  for (const value of [-1, 0.5, NaN, Infinity, 2_147_483_648, "10", null]) {
    assert.throws(() => new Brain({ baseUrl: "https://brain.example", connectionIdleTimeoutMs: value }), /connectionIdleTimeoutMs/u);
  }
  const f = fixture(t, { timeout: 0 });
  await f.client.sessions.create(f.options);
  await delay(45);
  assert.equal(f.requests.some(path => path.endsWith("/suspend")), false);
});

test("an actual Node process exits after releasing the command stream without close", { timeout: 5000 }, async t => {
  let stream;
  const server = createServer((request, response) => {
    response.setHeader("content-type", "application/json");
    if (request.url === "/v1/hosts") response.end(JSON.stringify({ host_id: "host_exit", token: "fixture" }));
    else if (request.url.endsWith("/commands")) {
      stream = response;
      response.setHeader("content-type", "text/event-stream");
      response.write(frame("activity", { connection: 1, idle: true }));
    } else if (request.url.endsWith("/suspend")) {
      stream.end();
      response.end(JSON.stringify({ suspended: true }));
    } else { response.statusCode = 404; response.end(); }
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => { server.closeAllConnections(); server.close(); });
  const source = `import { Brain } from ${JSON.stringify(new URL("../dist/index.js", import.meta.url).href)};
    const client = new Brain({ baseUrl: "http://127.0.0.1:${server.address().port}", connectionIdleTimeoutMs: 20 });
    await client.credentials();
    setTimeout(() => console.log("user timer survived"), 100);`;
  const child = spawn(process.execPath, ["--input-type=module", "--eval", source], { stdio: ["ignore", "pipe", "pipe"] });
  t.after(() => { if (child.exitCode === null) child.kill(); });
  let output = "";
  child.stdout.on("data", value => { output += value; });
  child.stderr.on("data", value => { output += value; });
  const [code] = await once(child, "exit");
  assert.equal(code, 0, output);
  assert.match(output, /user timer survived/u);
});


test("close during suspension aborts a queued wake without another stream", { timeout: 3000 }, async t => {
  const entered = Promise.withResolvers();
  const f = fixture(t, { suspend: (_, request) => {
    entered.resolve();
    return new Promise((_, reject) => request.signal.addEventListener("abort", () => reject(request.signal.reason), { once: true }));
  } });
  const session = await f.client.sessions.create(f.options);
  await entered.promise;
  const wake = assert.rejects(session.send("racing close"), { name: "AbortError" });
  await f.client.close();
  await wake;
  assert.equal(f.streams.length, 1);
  assert.equal(f.streams[0].closed, true);
  assert.equal(f.requests.some(path => path.endsWith("/messages")), false);
});

test("a creating operation pins the connection until its registry is attached", { timeout: 3000 }, async t => {
  const entered = Promise.withResolvers();
  const response = Promise.withResolvers();
  const f = fixture(t, { create: () => { entered.resolve(); return response.promise; } });
  const creation = f.client.sessions.create(f.options);
  await entered.promise;
  await delay(60);
  assert.equal(f.requests.some(path => path.endsWith("/suspend")), false);
  response.resolve(Response.json(state("created")));
  await creation;
  await f.sleeping();
});
