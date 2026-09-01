import assert from "node:assert/strict";
import test from "node:test";

import { z } from "zod";
import { Brain, agentloop, appTool, installExtensionIdentity } from "../dist/index.js";

const simple = agentloop((author) => {
  author.on.message((message, turn) => turn.finish());
});
installExtensionIdentity(simple, "simple", new Uint8Array([1, 2, 3]));

const session = () =>
  Response.json({ session_id: "ses_12345678901234567890", journal_id: "jrn_test", status: "idle", last_sequence: 0, config_hash: "b".repeat(64) });

const sse = (frames) => `${frames.map(({ id, event, data }) => `${id === undefined ? "" : `id: ${id}\n`}event: ${event}\ndata: ${JSON.stringify(data)}`).join("\n\n")}\n\n`;

const intent = (operationId, name, input) => ({
  id: 3,
  event: "tool_intent",
  data: {
    operation_id: operationId,
    request_identity: "c".repeat(64),
    session_id: "ses_12345678901234567890",
    binding: { name, hosting: "client", requires: [], binding_names: [] },
    invocation: { call_id: "call-1", name, input },
    deadline_ms: 120_000,
  },
});

test("a client tool compiles without an environment and answers off the stream", async () => {
  const requests = [];
  let resolveAnswered;
  const answered = new Promise((resolve) => { resolveAnswered = resolve; });
  const client = new Brain({
    baseUrl: "https://brain.example",
    token: "test-token",
    fetch: async (input, init) => {
      const request = new Request(input, init);
      requests.push(request);
      if (request.url.endsWith("/v1/agentloops")) return Response.json({ identity: "a".repeat(64), status: "admitted" });
      if (request.url.includes("/events")) {
        // The feed: the parked call's intent; the session ends once it is answered.
        return new Response(new ReadableStream({
          async start(controller) {
            const encoder = new TextEncoder();
            controller.enqueue(encoder.encode(sse([intent(`op_${"d".repeat(32)}`, "lookup_order", { id: "A-1001" })])));
            await answered;
            controller.enqueue(encoder.encode(sse([{ id: 6, event: "session_ended", data: {} }])));
            controller.close();
          },
        }), { headers: { "content-type": "text/event-stream" } });
      }
      if (request.url.includes("/tool-results/")) {
        queueMicrotask(() => resolveAnswered());
        return new Response(null, { status: 204 });
      }
      return session();
    },
  });

  const lookupOrder = appTool({
    name: "lookup_order",
    description: "Look up an order's status by id.",
    input: z.object({ id: z.string() }),
    execute: ({ id }) => ({ status: id === "A-1001" ? "shipped" : "unknown" }),
  });
  const handle = await client.sessions.create({
    model: { provider: "vercel-ai-gateway", name: "openai/gpt-5-mini", apiKey: "model-secret" },
    agentloop: simple(),
    tools: [lookupOrder],
  });

  const create = await requests[1].json();
  assert.equal(create.environments.length, 0, "a client tool must mint no environment");
  assert.equal(create.tools.length, 1);
  assert.equal(create.tools[0].hosting, "client");
  assert.equal(create.tools[0].environment_id, undefined);
  assert.equal(create.tools[0].name, "lookup_order");
  assert.deepEqual(create.tools[0].requires, []);

  await answered;
  const result = requests.find((request) => request.url.includes("/tool-results/"));
  assert.ok(result, "the outcome must be POSTed back");
  assert.equal(new URL(result.url).pathname, `/v1/sessions/ses_12345678901234567890/tool-results/op_${"d".repeat(32)}`);
  assert.equal(result.headers.get("idempotency-key"), `tool-result-op_${"d".repeat(32)}`);
  assert.deepEqual(await result.json(), { status: "ok", value: { status: "shipped" } });

  await handle.delete();
});

test("an intent for an unregistered tool is answered with a typed error, not silence", async () => {
  const requests = [];
  let resolveAnswered;
  const answered = new Promise((resolve) => { resolveAnswered = resolve; });
  const client = new Brain({
    baseUrl: "https://brain.example",
    fetch: async (input, init) => {
      const request = new Request(input, init);
      requests.push(request);
      if (request.url.endsWith("/v1/agentloops")) return Response.json({ identity: "a".repeat(64), status: "admitted" });
      if (request.url.includes("/events")) {
        return new Response(new ReadableStream({
          async start(controller) {
            const encoder = new TextEncoder();
            controller.enqueue(encoder.encode(sse([intent(`op_${"e".repeat(32)}`, "someone_elses_tool", {})])));
            await answered;
            controller.enqueue(encoder.encode(sse([{ id: 6, event: "session_ended", data: {} }])));
            controller.close();
          },
        }), { headers: { "content-type": "text/event-stream" } });
      }
      if (request.url.includes("/tool-results/")) {
        queueMicrotask(() => resolveAnswered());
        return new Response(null, { status: 204 });
      }
      return session();
    },
  });

  const handle = await client.sessions.create({
    model: { provider: "vercel-ai-gateway", name: "openai/gpt-5-mini", apiKey: "model-secret" },
    agentloop: simple(),
    tools: [appTool({ name: "mine", description: "Registered here.", input: z.object({}), execute: () => null })],
  });

  await answered;
  const result = requests.find((request) => request.url.includes("/tool-results/"));
  const outcome = await result.json();
  assert.equal(outcome.status, "error");
  assert.equal(outcome.error.code, "unknown_tool");

  await handle.delete();
});

test("appTool without execute still requires useIn, and execute must be a function", () => {
  const declared = appTool({ name: "plain", description: "Callback shape.", input: z.object({}) });
  assert.equal(typeof declared.useIn, "function");
  assert.throws(() => appTool({ name: "bad", description: "x", input: z.object({}), execute: "not a function" }), /execute must be a function/u);
});
