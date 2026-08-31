import assert from "node:assert/strict";
import test from "node:test";

import { z } from "zod";
import { Brain, BrainError, activateAgentloop, agentloop, createEnvironmentHandler, environment, executeTool, installExtensionIdentity, tool } from "../dist/index.js";

const simple = agentloop((author) => {
  const state = author.state(z.object({ messages: z.array(z.unknown()) }), () => ({ messages: [] }));
  author.on.message((message, turn) => {
    state.messages.push({ role: "user", content: [{ type: "text", text: String(message.content) }] });
    return turn.model({ messages: state.messages });
  });
});
installExtensionIdentity(simple, "simple", new Uint8Array([1, 2, 3]));

const workspace = environment((author) => {
  const instance = author.open(async () => ({}));
  instance.run(async () => undefined);
  instance.close(async () => undefined);
  return { suspend: instance.method(async () => undefined) };
});
installExtensionIdentity(workspace, "workspace");

const read = tool({ description: "Read a file.", input: z.object({ path: z.string() }) }, (author) => {
  author.run(async ({ path }) => path);
});
installExtensionIdentity(read, "read");

test("composes extensions through sessions, useIn, and object identity", async () => {
  const requests = [];
  const client = new Brain({
    baseUrl: "https://brain.example/",
    token: "test-token",
    fetch: async (input, init) => {
      const request = new Request(input, init);
      requests.push(request);
      if (request.url.endsWith("/v1/agentloops")) return Response.json({ identity: "a".repeat(64), status: "admitted" });
      if (request.url.includes("/calls/suspend")) return Response.json({ output: null });
      return Response.json({ session_id: "ses_12345678901234567890", journal_id: "jrn_test", status: "idle", last_sequence: 1, config_hash: "b".repeat(64) });
    },
  });
  const vm = workspace();
  const session = await client.sessions.create({
    model: { provider: "vercel-ai-gateway", name: "openai/gpt-5-mini", apiKey: "model-secret" },
    agentloop: simple(),
    tools: [read().useIn(vm)],
  });

  assert.equal(session.id, "ses_12345678901234567890");
  assert.equal(requests.length, 2);
  assert.equal(requests[0].headers.get("authorization"), "Bearer test-token");
  assert.match(requests[0].headers.get("idempotency-key"), /^agentloop-[0-9a-f]{64}$/u);
  const body = await requests[1].json();
  assert.deepEqual(body.agentloop, { identity: "a".repeat(64), configuration: {} });
  assert.equal(body.environments.length, 1);
  assert.equal(body.environments[0].environment_id, "env_1");
  assert.deepEqual(body.tools.map(({ name, environment_id, requires, binding_names }) => [name, environment_id, requires, binding_names]), [["read", "env_1", [], []]]);
  assert.equal(body.system, "");

  await vm.suspend();
  assert.match(requests[2].url, /\/environments\/env_1\/calls\/suspend$/u);
});

test("retries admission after a failure instead of caching the rejection", async () => {
  let calls = 0;
  const client = new Brain({
    baseUrl: "https://brain.example",
    fetch: async () => {
      calls += 1;
      if (calls === 1) return Response.json({ code: "internal", message: "boom", retryable: true }, { status: 500 });
      return Response.json({ identity: "a".repeat(64), status: "admitted" });
    },
  });
  const loop = simple();
  await assert.rejects(client.admit(loop), (error) => error instanceof BrainError && error.status === 500);
  assert.equal(await client.admit(loop), "a".repeat(64));
  assert.equal(await client.admit(loop), "a".repeat(64));
  assert.equal(calls, 2, "a successful admission stays cached");
});

test("runs synchronous Agentloop hooks and persists validated state", () => {
  const result = activateAgentloop(simple, {
    context: {},
    observation: { type: "user_message", content: "hello" },
    configuration: {},
    runtime: { logicalTimeMs: 1n },
  });
  assert.equal(result.decision.type, "model");
  assert.deepEqual(result.context.state.slots[0].messages, [
    { role: "user", content: [{ type: "text", text: "hello" }] },
  ]);
});

test("executes zero-configuration Tools from their serialized configuration", async () => {
  const context = {
    signal: AbortSignal.timeout(1_000),
    deadlineMs: Date.now() + 1_000,
  };
  assert.equal(await executeTool(read, {}, { path: "README.md" }, context), "README.md");
  assert.equal(await executeTool(read, undefined, { path: "README.md" }, context), "README.md");
  await assert.rejects(executeTool(read, { unexpected: true }, { path: "README.md" }, context), /does not accept options/u);
});

test("surfaces structured errors and rejects detached Environment calls", async () => {
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async () => Response.json({ code: "conflict", message: "key changed", retryable: false }, { status: 409 }) });
  await assert.rejects(client.sessions.list(), (error) => error instanceof BrainError && error.code === "conflict" && error.status === 409);
  await assert.rejects(workspace().suspend(), /only while its session is attached/u);
  assert.throws(() => new Brain({ baseUrl: "" }), /baseUrl is required/u);
  assert.throws(() => new Brain({ baseUrl: "file:\/\/\/tmp\/brain" }), /baseUrl must be HTTP/u);
});

test("runs async Environment lifecycle, methods, and streams through the generated adapter", async () => {
  const managed = environment({ options: z.object({ prefix: z.string() }) }, (author) => {
    const instance = author.open(async ({ options }) => ({ prefix: options.prefix }));
    instance.run(async (request, context) => ({ prefix: context.instance.prefix, request }));
    instance.close(async () => undefined);
    return {
      echo: instance.method({ input: z.string(), output: z.string() }, async (input, context) => `${context.instance.prefix}${input}`),
      values: instance.stream({ input: z.number().int(), item: z.number().int() }, async function* (count) { for (let value = 0; value < count; value += 1) yield value; }),
    };
  });
  installExtensionIdentity(managed, "managed", undefined, "managed");
  const handle = createEnvironmentHandler(managed);
  const command = (id, request, attachment_id) => ({
    contract: "environment/v2",
    binding: {},
    operation: { operation_id: id, request_identity: id.padEnd(64, "a"), environment_id: "env_1", session_id: "ses_test", ...(attachment_id === undefined ? {} : { attachment_id }), request },
  });
  assert.equal((await handle(command("setup", { type: "setup", configuration: { driver: "managed", prefix: ">" } }))).receipt.type, "accepted");
  const attached = await handle(command("attach", { type: "attach", grants: {}, provisions: [], bindings: {} }, "att_1"));
  assert.equal(attached.receipt.type, "accepted");
  assert.deepEqual(attached.receipt.provides, [], "a generated environment provides no capabilities yet");
  assert.equal((await handle(command("call", { type: "call", name: "echo", input: "ok" }, "att_1"))).receipt.output, ">ok");
  assert.deepEqual((await handle(command("stream", { type: "call", name: "values", input: 3 }, "att_1"))).receipt.output, [0, 1, 2]);
  const invoked = await handle(command("invoke", { type: "invoke", call_id: "call_1", tool: "anything", input: { x: 1 }, deadline_ms: 1000 }, "att_1"));
  assert.equal(invoked.receipt.type, "outcome");
  assert.equal(invoked.receipt.outcome.status, "ok");
  assert.deepEqual(invoked.receipt.outcome.value, { prefix: ">", request: { type: "invoke", call_id: "call_1", tool: "anything", input: { x: 1 }, deadline_ms: 1000 } });
});

test("admits any identifier-shaped provider client-side and leaves admission to the server", async () => {
  const fetchStub = async (input, init) => {
    const request = new Request(input, init);
    if (request.url.endsWith("/v1/agentloops")) return Response.json({ identity: "a".repeat(64), status: "admitted" });
    return Response.json({ session_id: "ses_12345678901234567890", journal_id: "jrn_test", status: "idle", last_sequence: 1, config_hash: "b".repeat(64) });
  };
  const client = new Brain({ baseUrl: "https://brain.example/", token: "t", fetch: fetchStub });
  // A custom provider the SDK has never heard of passes shape validation.
  const session = await client.sessions.create({
    model: { provider: "ollama-local", name: "llama3.3", apiKey: "k" },
    agentloop: simple(),
  });
  assert.equal(session.id, "ses_12345678901234567890");
  // Shape rules still hold.
  await assert.rejects(
    client.sessions.create({ model: { provider: "not a provider", name: "m", apiKey: "k" }, agentloop: simple() }),
    /model provider is invalid/u,
  );
  await assert.rejects(
    client.sessions.create({ model: { provider: "", name: "m", apiKey: "k" }, agentloop: simple() }),
    /model provider is invalid/u,
  );
  // The gateway keeps its namespace rule.
  await assert.rejects(
    client.sessions.create({ model: { provider: "vercel-ai-gateway", name: "gpt-5-mini", apiKey: "k" }, agentloop: simple() }),
    /provider namespace/u,
  );
});
