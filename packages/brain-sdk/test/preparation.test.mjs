import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";
import { z } from "zod";
import { Brain, agentloop, brainEnv, component, inspectAgentloop, inspectTool, program, tool } from "../dist/index.js";

const digest = bytes => createHash("sha256").update(bytes).digest("hex");

test("preparation shares runtimes across programs, clients and sessions", async () => {
  const stored = new Set();
  const uploads = [];
  const prepared = [];
  const sessions = [];
  const transport = async (url, init) => {
    const request = new Request(url, init);
    const path = new URL(url).pathname;
    if (request.method === "GET") {
      return stored.has(path) ? Response.json({ id: path.split("/").at(-1), status: "admitted" })
        : Response.json({ code: "not_found", message: "missing", retryable: false }, { status: 404 });
    }
    if (path === "/v1/brain-env/prepare") {
      prepared.push(await request.json());
      return new Response(null, { status: 204 });
    }
    if (path === "/v1/sessions") {
      sessions.push(await request.json());
      return Response.json({ session_id: "ses_12345678901234567890", status: "idle", last_sequence: 1 });
    }
    const bytes = new Uint8Array(await request.arrayBuffer());
    const id = digest(bytes);
    uploads.push({ path, id });
    stored.add(`${path}/${id}`);
    return Response.json({ id, status: "admitted" });
  };
  const runner = component(new Uint8Array([0, 97, 115, 109]));
  const make = source => agentloop({ implementation: program({ runtime: runner, source: new TextEncoder().encode(source) }) })({ env: brainEnv({ name: "brain" }) });
  const first = new Brain({ baseUrl: "https://brain.example", fetch: transport });
  const loop = await first.prepare(make("first program"));
  assert.equal(inspectAgentloop(loop).implementation.type, "brain_program");
  assert.equal(sessions.length, 0);
  await first.close();
  const second = new Brain({ baseUrl: "https://brain.example", fetch: transport });
  await second.prepare(make("first program"));
  await second.prepare(make("new program"));
  for (let count = 0; count < 2; count++) {
    await second.sessions.create({ agentloop: loop, model: { provider: "openai", name: "gpt-5", apiKey: "fixture" } });
  }
  assert.deepEqual(uploads.map(upload => upload.path), ["/v1/agentloops", "/v1/programs", "/v1/programs"]);
  assert.equal(prepared.length, 3);
  assert.equal(prepared[0].agentloops[0], digest(new Uint8Array([0, 97, 115, 109])));
  assert.deepEqual(sessions[0].agentloop, sessions[1].agentloop);
  await second.close();
});

test("program Tools retain their configuration and preparation failures remain explicit", async () => {
  const runtime = "a".repeat(64);
  const source = new TextEncoder().encode("tool program");
  const factory = tool({ name: "echo", description: "Echo", input: z.object({}), options: z.object({ prefix: z.string() }),
    implementation: program({ runtime, source }) });
  const placed = factory({ env: brainEnv({ name: "brain" }), prefix: "hello" });
  let fail = true;
  const requests = [];
  const brain = new Brain({ baseUrl: "https://brain.example", fetch: async (url, init) => {
    const path = new URL(url).pathname;
    if (path.startsWith("/v1/programs/")) return Response.json({ id: digest(source), status: "admitted" });
    assert.equal(path, "/v1/brain-env/prepare");
    requests.push(JSON.parse(init.body));
    return fail ? Response.json({ code: "preparation_failed", message: "worker unavailable", retryable: false }, { status: 409 }) : new Response(null, { status: 204 });
  } });
  await assert.rejects(brain.prepare(placed), error => error.code === "preparation_failed");
  fail = false;
  const prepared = await brain.prepare(placed);
  assert.equal(requests.length, 2);
  assert.deepEqual(requests[1], { agentloops: [], tools: [runtime], programs: [digest(source)] });
  assert.deepEqual(inspectTool(prepared).implementation.configuration, { prefix: "hello" });
  await brain.close();
});
