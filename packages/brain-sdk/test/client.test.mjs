import assert from "node:assert/strict";
import test from "node:test";

import { z } from "zod";
import { Brain, BrainError, agentloop, brainWasm, component, tool } from "../dist/index.js";

const sessionResponse = { session_id: "ses_12345678901234567890", status: "idle", last_sequence: 1 };

test("session creation admits Components and sends explicit Environment placement", async () => {
  const requests = [];
  const fetchStub = async (input, init) => {
    const request = new Request(input, init);
    requests.push(request);
    if (request.url.endsWith("/v1/agentloops")) return Response.json({ identity: "a".repeat(64), status: "admitted" });
    if (request.url.endsWith("/v1/tools")) return Response.json({ identity: "b".repeat(64), status: "admitted" });
    if (request.url.endsWith("/v1/sessions")) return Response.json(sessionResponse);
    throw new Error(`unexpected request ${request.url}`);
  };
  const runtime = brainWasm({
    network: { allow: ["api.example.com"] },
    filesystem: { workspace: true },
  });
  const pi = agentloop({ implementation: component(new Uint8Array([1])) });
  const read = tool({
    name: "read",
    description: "Read one file.",
    input: z.object({ path: z.string() }),
    implementation: component(new Uint8Array([2])),
    needs: ["fs"],
  });
  const client = new Brain({ baseUrl: "https://brain.example/", token: "brain-token", fetch: fetchStub });
  const session = await client.sessions.create({
    model: { provider: "openai", name: "gpt-5", apiKey: "model-token" },
    agentloop: pi({ env: runtime }),
    tools: [read({ env: runtime })],
  });

  assert.equal(session.id, sessionResponse.session_id);
  assert.deepEqual(requests.map((request) => new URL(request.url).pathname), ["/v1/agentloops", "/v1/tools", "/v1/sessions"]);
  assert.equal(requests[0].headers.get("content-type"), "application/octet-stream");
  assert.equal(requests[1].headers.get("content-type"), "application/octet-stream");
  const body = await requests[2].json();
  assert.equal(body.environments.length, 1, "one shared Environment is declared once");
  assert.equal(body.environments[0].configuration.driver, "brain_wasm");
  assert.equal(body.agentloop.environment_id, body.environments[0].environment_id);
  assert.deepEqual(body.tools[0].implementation, {
    type: "brain_component",
    identity: "b".repeat(64),
    configuration: {},
  });
  assert.equal(body.tools[0].environment_id, body.agentloop.environment_id);
});

test("failed admission is not cached and successful admission is", async () => {
  let calls = 0;
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async () => {
    calls += 1;
    if (calls === 1) return Response.json({ code: "internal", message: "boom", retryable: false }, { status: 500 });
    return Response.json({ identity: "a".repeat(64), status: "admitted" });
  } });
  const pi = agentloop({ implementation: component(new Uint8Array([1])) });
  const loop = pi({ env: brainWasm() });
  await assert.rejects(client.admit(loop), (error) => error instanceof BrainError && error.status === 500);
  assert.equal(await client.admit(loop), "a".repeat(64));
  assert.equal(await client.admit(loop), "a".repeat(64));
  assert.equal(calls, 2);
});

test("retrying create with the same key sends the same Environment identities", async () => {
  const bodies = [];
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async (input, init) => {
    const request = new Request(input, init);
    if (request.url.endsWith("/v1/agentloops")) return Response.json({ identity: "a".repeat(64), status: "admitted" });
    bodies.push(await request.json());
    if (bodies.length === 1) throw new TypeError("response lost after server accepted create");
    return Response.json(sessionResponse);
  } });
  const pi = agentloop({ implementation: component(new Uint8Array([1])) });
  const options = { model: { provider: "openai", name: "gpt-5", apiKey: "key" }, agentloop: pi({ env: brainWasm() }) };
  await assert.rejects(client.sessions.create(options, { idempotencyKey: "create-once" }));
  await client.sessions.create(options, { idempotencyKey: "create-once" });
  assert.deepEqual(bodies[1], bodies[0]);
  await client.sessions.create(options, { idempotencyKey: "create-another" });
  assert.notEqual(bodies[2].environments[0].environment_id, bodies[0].environments[0].environment_id);
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
