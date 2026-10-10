import assert from "node:assert/strict";
import test from "node:test";
import { BrainError } from "../dist/index.js";
import { callExecution, environmentProtocol, finishExecution, reportEnvironment, serveEnvironment } from "../dist/runtime.js";

const command = request => ({ contract: "environment/v1", operation: {
  session_id: "ses_000000000000000000000001", environment: "workspace", sequence: 2, request,
} });
const grant = { url: "https://brain.example/call", token: "invocation", methods: ["finish"] };

test("generated envelopes reject malformed commands before effects and support both execution roles", async () => {
  let effects = 0;
  const handle = environmentProtocol(() => { effects++; return { type: "accepted" }; });
  const valid = command({ type: "execute", implementation: {}, input: {}, callback: grant });
  for (const invalid of [
    { ...valid, contract: "environment/v9" },
    { ...valid, operation: { ...valid.operation, sequence: 0 } },
    { ...valid, operation: { ...valid.operation, session_id: "wrong" } },
    command({ type: "unrecognized" }),
  ]) await assert.rejects(handle(invalid));
  assert.equal(effects, 0);
  await handle(valid);
  await handle(command({ type: "execute", implementation: {}, input: {}, callback: { ...grant, methods: ["set_transcript"] } }));
  assert.equal(effects, 2);
});

test("protocol errors retain structured failure and uncertain outcome semantics", async () => {
  const failure = new BrainError(409, "busy", "Resource busy", true, { domain: "resource" });
  const handle = environmentProtocol(() => { throw failure; });
  assert.deepEqual((await handle(command({ type: "teardown" }))).receipt,
    { type: "failure", code: "busy", message: "Resource busy", retryable: true, details: { domain: "resource" } });
  const unknown = environmentProtocol(() => { throw new BrainError(0, "ambiguous", "Receipt lost", false); });
  assert.deepEqual((await unknown(command({ type: "teardown" }))).receipt, { type: "unknown", message: "Receipt lost" });
});

test("a completion acknowledgement is validated and never retried when it is lost", async () => {
  const operation = command({ type: "execute", implementation: {}, input: {}, callback: grant }).operation;
  for (const [respond, expected] of [
    [() => Response.json(9), "result"],
    [() => Response.json({}), "unknown"],
    [() => new Response("lost", { status: 503 }), "unknown"],
    [() => { throw new TypeError("network lost"); }, "unknown"],
  ]) {
    let calls = 0;
    const result = await finishExecution(operation, { type: "result", output: "saved" }, { fetch: async (_, init) => {
      calls++;
      assert.equal(init.redirect, "error");
      assert.equal(init.headers.authorization, "Bearer invocation");
      assert.deepEqual(JSON.parse(init.body), { method: "finish", input: { status: "ok", value: "saved" } });
      return respond();
    } });
    assert.equal(result.type, expected);
    assert.equal(calls, 1);
  }
});

test("services reject absent authority or cancelled contexts before I/O and reporters use separate envelopes", async () => {
  const fetch = async () => assert.fail("denied service reached fetch");
  assert.throws(() => callExecution(grant, "model", {}, { fetch }), /not granted/);
  assert.throws(() => callExecution(grant, "finish", null, { fetch, signal: AbortSignal.abort() }), { name: "AbortError" });
  const reporter = { ...grant, token: "reporter", methods: ["emit", "result"] };
  const event = { type: "event", event_type: "resource_note", data: {} };
  assert.equal(await reportEnvironment(reporter, event, { fetch: async (_, init) => {
    assert.equal(init.headers.authorization, "Bearer reporter");
    assert.deepEqual(JSON.parse(init.body), event);
    return Response.json({ sequence: 12 });
  } }), 12);
});

test("the Node binding authenticates and bounds requests before executing an environment", async t => {
  let effects = 0;
  const handler = environmentProtocol(() => { effects++; return { type: "accepted" }; });
  const server = await serveEnvironment(handler, { token: "fixture", maxBodyBytes: 1024 });
  t.after(server.close);
  const post = (body, token = "fixture") => fetch(`${server.url}/v1/operations`, {
    method: "POST", headers: { authorization: `Bearer ${token}` }, body: JSON.stringify(body),
  });
  assert.equal((await post(command({ type: "teardown" }), "wrong")).status, 401);
  assert.equal((await post(command({ type: "other" }))).status, 400);
  assert.equal((await post({ data: "x".repeat(1024) })).status, 413);
  assert.equal(effects, 0);
  const result = await post(command({ type: "teardown" }));
  assert.equal(result.status, 200);
  assert.deepEqual(await result.json(), { contract: "environment/v1", sequence: 2, receipt: { type: "accepted" } });
  assert.equal(effects, 1);
});
