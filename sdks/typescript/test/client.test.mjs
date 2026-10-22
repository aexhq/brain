import assert from "node:assert/strict";
import { test } from "node:test";
import { Pi, Codex, OpenCode, PiWebAccess, Brain, BrainError, Model, ApiKey, CredentialReference, ManagedTool, AwsS3, Modal, E2B, Daytona } from "../dist/index.js";

test('typed harness references select installed profiles without transmitting server settings', async () => {
  const bodies = [];
  const brain = new Brain('http://brain.test', 'token', async (_url, init) => { bodies.push(JSON.parse(init.body)); return Response.json({ id: 'session' }); });
  for (const harness of [Pi({ name: 'research' }), Codex(), OpenCode()]) await brain.create({ harness, model: Model('openai/model') });
  assert.deepEqual(bodies.map(body => body.harness), ['research', 'codex', 'opencode']);
  await assert.rejects(brain.create({ harness: Pi({ plugins: [PiWebAccess({ config: { braveApiKey: 'private' } })] }), model: Model('openai/model') }), /Install harness options/);
  assert.equal(bodies.length, 3);
  assert.throws(() => Codex({ unknown: true }), /Unknown codex option/);
});

test('integration helpers keep credentials in private admission input and omit absent overrides', async () => {
  let wire;
  const brain = new Brain('http://brain.test', 'token', async (_url, init) => {
    wire = JSON.parse(init.body);
    return Response.json({ id: 'session' });
  });
  await brain.create({ harness: Pi(), model: Model('openai/model'), tools: [
    AwsS3({ bucket: 'customer-bucket', region: 'eu-west-2', accessKeyId: 'customer-access', secretAccessKey: 'private-s3-secret', sessionToken: 'temporary' }),
    Modal({ tokenId: 'customer-modal', tokenSecret: 'private-modal-secret', operations: ['inspect'] }),
    E2B(), Daytona(),
  ] });
  assert.deepEqual(wire.config.bindings.map(b => b.provider), ['aws_s3', 'modal', 'e2b', 'daytona']);
  assert.deepEqual(wire.config.bindings[0].parameters, { bucket: 'customer-bucket', region: 'eu-west-2' });
  assert.equal(wire.credentials.length, 2);
  assert.equal(wire.credentials[0].value.secretAccessKey, 'private-s3-secret');
  assert.equal(wire.credentials[1].value.tokenSecret, 'private-modal-secret');
  assert.doesNotMatch(JSON.stringify(wire.config), /private-|customer-access|customer-modal|temporary/);
  assert.throws(() => AwsS3({ secret_access_key: 'misspelled' }), /Unknown aws_s3 option/);
});

test("model and managed tool declarations lower private admission separately from config", async () => {
  const client = new Brain("http://brain.test", "token", async (_url, init) => {
    assert.deepEqual(JSON.parse(init.body), { harness: "pi", config: { model: "test/model", tools: ["lookup"], bindings: [
      { alias: "records", provider: "data", operations: ["read"], parameters: { bucket: "one" } },
    ] }, model_credential: { api_key: "customer-key" }, credentials: [{ binding: "records", value: { token: "tool-key" } }] });
    return Response.json({ id: "session" });
  });
  await client.create({ harness: Pi(), model: Model("test/model", ApiKey("customer-key")), tools: [ManagedTool("lookup"),
    ManagedTool("data", { alias: "records", operations: ["read"], parameters: { bucket: "one" }, credential: { token: "tool-key" } }),
  ] });
  assert.deepEqual(CredentialReference("tenant-key"), { reference: "tenant-key" });
  await assert.rejects(client.create({ harness: Pi(), model: Model("test/model"), tools: [ManagedTool("lookup", { credential: "ignored" })] }), /require operations/);
});

test("an unsupported callback transport fails before registration", async () => {
  const brain = new Brain("http://brain.test", "token", async () => { assert.fail("invalid transport made a request"); });
  await assert.rejects(brain.create({
    harness: "pi", config: { model: "test/model", tools: [] }, runtime_transport: "invalid",
    runtime_tools: [{ name: "read", description: "Read", input_schema: { type: "object" }, run: () => null }],
  }), /unsupported runtime transport/);
});

test("unsafe requests are never retried and errors preserve identity", async () => {
  let count = 0;
  const client = new Brain("http://brain.test", "token", async () => {
    count++;
    return Response.json({ error: { code: "overloaded", message: "busy" }, request_id: "request" }, { status: 429 });
  });
  await assert.rejects(client.send("session", { type: "user.message", text: "hello" }), e => e instanceof BrainError && e.code === "overloaded" && e.requestId === "request");
  assert.equal(count, 1);
});

test("SSE handles split UTF-8 and comments without treating keepalives as events", async () => {
  const payload = new TextEncoder().encode(': keepalive\n\nevent: assistant.message\ndata: {"id":"one","type":"assistant.message","data":{"text":"你好"}}\n\n');
  const client = new Brain("http://brain.test", "token", async () => new Response(new ReadableStream({ start(controller) { for (const byte of payload) controller.enqueue(Uint8Array.of(byte)); controller.close(); } })));
  const events = []; for await (const event of client.observe("session")) events.push(event);
  assert.equal(events.length, 1); assert.equal(events[0].data.text, "你好");
});
