import assert from "node:assert/strict";
import test from "node:test";

import { z } from "zod";
import { Brain, BrainError, activateBrain, brain, createEnvironmentHandler, environment, executeTool, installExtensionIdentity, tool } from "../dist/index.js";

const simple = brain((author) => {
  const state = author.state(z.object({ messages: z.array(z.unknown()) }), () => ({ messages: [] }));
  author.on.message((message, turn) => {
    state.messages.push({ role: "user", content: message.content });
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
      if (request.url.endsWith("/v1/agentloops")) return Response.json({ digest: "a".repeat(64), status: "admitted" });
      if (request.url.includes("/calls/suspend")) return Response.json({ output: null });
      return Response.json({ session_id: "ses_12345678901234567890", journal_id: "jrn_test", status: "idle", through_sequence: 1, presentation_digest: "b".repeat(64) });
    },
  });
  const vm = workspace();
  const session = await client.sessions.create({
    model: { provider: "vercel-ai-gateway", name: "openai/gpt-5-mini", apiKey: "model-secret" },
    brain: simple(),
    tools: [read().useIn(vm)],
  });

  assert.equal(session.id, "ses_12345678901234567890");
  assert.equal(requests.length, 2);
  assert.equal(requests[0].headers.get("authorization"), "Bearer test-token");
  assert.match(requests[0].headers.get("idempotency-key"), /^brain-[0-9a-f]{64}$/u);
  const body = await requests[1].json();
  assert.deepEqual(body.brain_configuration, {});
  assert.equal(body.environments.length, 1);
  assert.equal(body.environments[0].environment_id, "env_1");
  assert.deepEqual(body.tool_bindings.map(({ name, environment_id }) => [name, environment_id]), [["read", "env_1"]]);

  await vm.suspend();
  assert.match(requests[2].url, /\/environments\/env_1\/calls\/suspend$/u);
});

test("runs synchronous Brain hooks and persists validated state", () => {
  const result = activateBrain(simple, {
    context: {},
    observation: { type: "user_message", content: "hello" },
    configuration: {},
    runtime: { logicalTimeMs: 1n },
  });
  assert.equal(result.decision.type, "model");
  assert.deepEqual(result.context.state.slots[0].messages, [{ role: "user", content: "hello" }]);
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
    contract: "environment/v1",
    binding: {},
    operation: { operation_id: id, request_digest: id.padEnd(64, "a"), environment_id: "env_1", session_id: "ses_test", ...(attachment_id === undefined ? {} : { attachment_id }), request },
  });
  assert.equal((await handle(command("setup", { type: "setup", configuration: { driver: "managed", prefix: ">" } }))).receipt.type, "accepted");
  assert.equal((await handle(command("attach", { type: "attach", grants: {} }, "att_1"))).receipt.type, "accepted");
  assert.equal((await handle(command("call", { type: "call", name: "echo", input: "ok" }, "att_1"))).receipt.output, ">ok");
  assert.deepEqual((await handle(command("stream", { type: "call", name: "values", input: 3 }, "att_1"))).receipt.output, [0, 1, 2]);
});
