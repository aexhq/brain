import assert from "node:assert/strict";
import test from "node:test";

import { z } from "zod";
import { Brain, agentloop, inspectServedTool, installExtensionIdentity, tool } from "../dist/index.js";

const simple = agentloop((author) => {
  author.on.message((message, turn) => turn.finish());
});
installExtensionIdentity(simple, "simple", new Uint8Array([1, 2, 3]));

const SHARE_KEY = `sk.ses_12345678901234567890.${"f".repeat(64)}`;

const session = () =>
  Response.json({ session_id: "ses_12345678901234567890", journal_id: "jrn_test", status: "idle", last_sequence: 0, config_hash: "b".repeat(64), share_key: SHARE_KEY });

const sse = (frames) => `${frames.map(({ id, event, data }) => `${id === undefined ? "" : `id: ${id}\n`}event: ${event}\ndata: ${JSON.stringify(data)}`).join("\n\n")}\n\n`;

const intent = (operationId, name, input, id = 3) => ({
  id,
  event: "tool_intent",
  data: {
    operation_id: operationId,
    request_identity: "c".repeat(64),
    session_id: "ses_12345678901234567890",
    binding: { name, hosting: "client", needs: [], binding_names: [] },
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

  const lookupOrder = tool({
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
  assert.equal(handle.shareKey, SHARE_KEY, "the share key rides the create response");

  const create = await requests[1].json();
  assert.equal(create.environments.length, 0, "a client tool must mint no environment");
  assert.equal(create.tools.length, 1);
  assert.equal(create.tools[0].hosting, "client");
  assert.equal(create.tools[0].environment_id, undefined);
  assert.equal(create.tools[0].name, "lookup_order");
  assert.deepEqual(create.tools[0].needs, []);

  await answered;
  const result = requests.find((request) => request.url.includes("/tool-results/"));
  assert.ok(result, "the outcome must be POSTed back");
  assert.equal(new URL(result.url).pathname, `/v1/sessions/ses_12345678901234567890/tool-results/op_${"d".repeat(32)}`);
  assert.equal(result.headers.get("idempotency-key"), `tool-result-op_${"d".repeat(32)}`);
  assert.deepEqual(await result.json(), { status: "ok", value: { status: "shipped" } });

  await handle.delete();
});

test("an intent for a tool this process does not serve is left for whoever does", async () => {
  const requests = [];
  const client = new Brain({
    baseUrl: "https://brain.example",
    fetch: async (input, init) => {
      const request = new Request(input, init);
      requests.push(request);
      if (request.url.endsWith("/v1/agentloops")) return Response.json({ identity: "a".repeat(64), status: "admitted" });
      if (request.url.includes("/events")) {
        return new Response(new ReadableStream({
          start(controller) {
            const encoder = new TextEncoder();
            controller.enqueue(encoder.encode(sse([
              intent(`op_${"e".repeat(32)}`, "someone_elses_tool", {}),
              { id: 6, event: "session_ended", data: {} },
            ])));
            controller.close();
          },
        }), { headers: { "content-type": "text/event-stream" } });
      }
      if (request.url.includes("/tool-results/")) return new Response(null, { status: 204 });
      return session();
    },
  });

  const handle = await client.sessions.create({
    model: { provider: "vercel-ai-gateway", name: "openai/gpt-5-mini", apiKey: "model-secret" },
    agentloop: simple(),
    tools: [
      tool({ name: "mine", description: "Registered here.", input: z.object({}), execute: () => null }),
      tool({ name: "someone_elses_tool", description: "Served elsewhere.", input: z.object({}) }),
    ],
  });

  // Give the pump its ticks; a wrong answer would already be in flight.
  await new Promise((resolve) => setTimeout(resolve, 50));
  assert.equal(
    requests.find((request) => request.url.includes("/tool-results/")),
    undefined,
    "the creator's pump must not answer a tool it has no handler for",
  );
  const create = await requests[1].json();
  assert.deepEqual(create.tools.map(({ name, hosting }) => [name, hosting]), [["mine", "client"], ["someone_elses_tool", "client"]]);

  await handle.delete();
});

test("joining with the share key serves a tool off the serve feed", async () => {
  const requests = [];
  let resolveAnswered;
  const answered = new Promise((resolve) => { resolveAnswered = resolve; });
  const fetchStub = async (input, init) => {
    const request = new Request(input, init);
    requests.push(request);
    if (request.url.includes("/serve")) {
      return new Response(new ReadableStream({
        async start(controller) {
          const encoder = new TextEncoder();
          controller.enqueue(encoder.encode(sse([intent(`op_${"d".repeat(32)}`, "highlight_row", { row: 4 })])));
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
  };

  const highlightRow = tool({
    name: "highlight_row",
    description: "Highlight a row in the visible grid.",
    input: z.object({ row: z.number().int() }),
  });
  const highlighted = [];
  const client = new Brain({ baseUrl: "https://brain.example", fetch: fetchStub });
  const remote = client.sessions.join(SHARE_KEY);
  assert.equal(remote.sessionId, "ses_12345678901234567890", "the share key names its session");
  remote.serve(highlightRow, ({ row }) => { highlighted.push(row); return null; });

  await answered;
  assert.deepEqual(highlighted, [4]);
  const feed = requests.find((request) => request.url.includes("/serve"));
  assert.ok(feed, "serve must open the serve feed");
  const feedUrl = new URL(feed.url);
  assert.equal(feedUrl.pathname, "/v1/sessions/ses_12345678901234567890/serve");
  assert.equal(feedUrl.searchParams.get("tools"), "highlight_row");
  assert.equal(feed.headers.get("authorization"), `Bearer ${SHARE_KEY}`, "the share key authorizes the feed");
  const result = requests.find((request) => request.url.includes("/tool-results/"));
  assert.equal(result.headers.get("authorization"), `Bearer ${SHARE_KEY}`, "the share key authorizes the answer");
  assert.deepEqual(await result.json(), { status: "ok", value: null });

  remote.close();
});

test("tool() forms: served tools are inspectable, execute must be a function, join checks its key", () => {
  const declared = tool({ name: "plain", description: "Served shape.", input: z.object({}) });
  assert.ok(inspectServedTool(declared), "a tool without execute is a served tool");
  assert.throws(() => tool({ name: "bad", description: "x", input: z.object({}), execute: "not a function" }), /execute must be a function/u);
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async () => new Response(null, { status: 500 }) });
  assert.throws(() => client.sessions.join("not-a-key"), /share key/u);
  assert.throws(() => client.sessions.join(SHARE_KEY).serve(declared, "not a function"), /handler function/u);
});
