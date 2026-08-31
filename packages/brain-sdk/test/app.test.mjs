import assert from "node:assert/strict";
import { createHmac } from "node:crypto";
import test from "node:test";

import { z } from "zod";
import { Brain, appTool, appTools, environment, installExtensionIdentity } from "../dist/index.js";

const signingKey = "app-secret";
const sign = (body) => createHmac("sha256", signingKey).update(body).digest("hex");
const post = (handler, frame, options = {}) => {
  const body = typeof frame === "string" ? frame : JSON.stringify(frame);
  const headers = { "content-type": "application/json" };
  if (options.signature !== null) headers["x-brain-signature"] = options.signature ?? sign(body);
  return handler(new Request("https://app.example/tools", { method: "POST", body, headers }));
};

function invoiceTools() {
  return appTools({ signingKey }).register({
    name: "create_invoice",
    description: "Create an invoice in this app.",
    input: z.object({ customer_id: z.string(), amount_cents: z.number().int() }),
    output: z.object({ invoice_id: z.string() }),
  }, async ({ customer_id }) => ({ invoice_id: `inv_${customer_id}` }));
}

test("answers a signed invocation with an ok outcome", async () => {
  const handler = invoiceTools().fetchHandler();
  const response = await post(handler, { call_id: "call_1", name: "create_invoice", arguments: { customer_id: "c9", amount_cents: 100 }, deadline_ms: 5_000 });
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { status: "ok", value: { invoice_id: "inv_c9" } });
});

test("refuses unsigned and garbage requests without invoking anything", async () => {
  let invoked = false;
  const handler = appTools({ signingKey }).register(
    { name: "observe", description: "Record that the handler ran.", input: z.object({}) },
    () => { invoked = true; return null; },
  ).fetchHandler();
  const frame = { call_id: "call_1", name: "observe", arguments: {}, deadline_ms: 5_000 };
  assert.equal((await post(handler, frame, { signature: null })).status, 401);
  assert.equal((await post(handler, frame, { signature: "f".repeat(64) })).status, 401);
  assert.equal((await post(handler, frame, { signature: "not-hex" })).status, 401);
  assert.equal((await post(handler, "not json")).status, 400);
  assert.equal((await post(handler, { call_id: "call_1" })).status, 400);
  assert.equal((await handler(new Request("https://app.example/tools", { method: "GET" }))).status, 405);
  assert.equal(invoked, false);
});

test("maps schema violations and thrown errors into error outcomes", async () => {
  const handler = appTools({ signingKey })
    .register({ name: "typed", description: "Validate input.", input: z.object({ count: z.number() }), output: z.object({ doubled: z.number() }) }, ({ count }) => ({ doubled: count * 2 }))
    .register({ name: "broken", description: "Always throws.", input: z.object({}) }, () => { throw new Error("kaboom"); })
    .register({ name: "malformed", description: "Breaks its output contract.", input: z.object({}), output: z.object({ ok: z.boolean() }) }, () => ({ ok: "yes" }))
    .fetchHandler();
  const outcomeOf = async (name, args) => (await post(handler, { call_id: "call_1", name, arguments: args, deadline_ms: 5_000 })).json();
  assert.equal((await outcomeOf("typed", { count: "three" })).error.code, "invalid_input");
  const broken = await outcomeOf("broken", {});
  assert.equal(broken.error.code, "tool_error");
  assert.match(broken.error.message, /kaboom/u);
  assert.equal((await outcomeOf("malformed", {})).error.code, "invalid_output");
  assert.equal((await outcomeOf("missing", {})).error.code, "unknown_tool");
});

test("honors the deadline and best-effort cancel", async () => {
  const handler = appTools({ signingKey }).register(
    { name: "wait", description: "Wait for the signal.", input: z.object({}) },
    (_input, call) => new Promise(() => { void call; }),
  ).fetchHandler();
  const timedOut = await (await post(handler, { call_id: "slow_1", name: "wait", arguments: {}, deadline_ms: 50 })).json();
  assert.deepEqual(timedOut, { status: "timeout" });
  const pending = post(handler, { call_id: "slow_2", name: "wait", arguments: {}, deadline_ms: 60_000 });
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal((await post(handler, { cancel: "slow_2" })).status, 200);
  assert.deepEqual(await (await pending).json(), { status: "cancelled" });
});

test("publishes callback manifests with no payload and no requirements", () => {
  const manifests = invoiceTools().manifests();
  assert.equal(manifests.length, 1);
  const manifest = manifests[0];
  assert.equal(manifest.name, "create_invoice");
  assert.equal(manifest.hosting, "callback");
  assert.deepEqual(manifest.requires, []);
  assert.deepEqual(manifest.binding_names, []);
  assert.equal("payload" in manifest, false);
  assert.equal(manifest.input_schema.type, "object");
  assert.equal(manifest.output_schema.type, "object");
});

test("rejects a missing signing key and duplicate registrations", () => {
  assert.throws(() => appTools({ signingKey: undefined }), /signingKey/u);
  assert.throws(() => appTools({ signingKey }).register({ name: "twin", description: "d", input: z.object({}) }, () => null).register({ name: "twin", description: "d", input: z.object({}) }, () => null), /already registered/u);
});

test("composes appTool bindings into a callback-shaped create request", async () => {
  const app = environment((author) => {
    const instance = author.open(async () => ({}));
    instance.run(async () => undefined);
    instance.close(async () => undefined);
    author.route.callbacks();
    return {};
  });
  installExtensionIdentity(app, "app");
  const requests = [];
  const client = new Brain({
    baseUrl: "https://brain.example",
    fetch: async (input, init) => {
      const request = new Request(input, init);
      requests.push(request);
      if (request.url.endsWith("/v1/agentloops")) return Response.json({ identity: "a".repeat(64), status: "admitted" });
      return Response.json({ session_id: "ses_12345678901234567890", journal_id: "jrn_test", status: "idle", last_sequence: 1, config_hash: "b".repeat(64) });
    },
  });
  const { agentloop } = await import("../dist/index.js");
  const loop = agentloop((author) => { author.on.message((_message, turn) => turn.done()); });
  installExtensionIdentity(loop, "loop", new Uint8Array([1]));
  await client.sessions.create({
    model: { provider: "anthropic", name: "claude-sonnet-4-5", apiKey: "k" },
    agentloop: loop(),
    tools: [appTool({ name: "create_invoice", description: "Create an invoice.", input: z.object({ customer_id: z.string() }) }).useIn(app())],
  });
  const body = await requests[1].json();
  assert.equal(body.tools.length, 1);
  const tool = body.tools[0];
  assert.equal(tool.name, "create_invoice");
  assert.equal(tool.hosting, "callback");
  assert.deepEqual(tool.requires, []);
  assert.deepEqual(tool.binding_names, []);
  assert.equal("payload" in tool, false);
  assert.equal(tool.environment_id, "env_1");
});
