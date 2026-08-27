import assert from "node:assert/strict";
import test from "node:test";

import { Brain, BrainError, defineAgentLoop, defineEnvironment, defineTool } from "../dist/index.js";

test("composes extensions while hiding admission, bindings, and operation keys", async () => {
  const requests = [];
  const brain = new Brain({
    baseUrl: "https://brain.example/",
    token: "test-token",
    fetch: async (input, init) => {
      const request = new Request(input, init);
      requests.push(request);
      if (request.url.endsWith("/v1/agentloops")) {
        return Response.json({ digest: "a".repeat(64), status: "admitted" });
      }
      return Response.json({
        session_id: "ses_12345678901234567890",
        journal_id: "jrn_test",
        status: "idle",
        through_sequence: 1,
        presentation_digest: "b".repeat(64),
      });
    },
  });
  const workspace = defineEnvironment({ capability: "workspace", configuration: { driver: "test" } });
  const read = defineTool({
    environmentCapability: "workspace",
    definition: { name: "read", description: "Read a file.", inputSchema: { type: "object" } },
  });
  const write = defineTool({
    environmentCapability: "workspace",
    definition: { name: "write", description: "Write a file.", inputSchema: { type: "object" } },
  });
  const session = await brain.createSession({
    model: { provider: "vercel-ai-gateway", name: "openai/gpt-5-mini", apiKey: "model-secret" },
    agentLoop: defineAgentLoop(new Uint8Array([1, 2, 3])),
    tools: [read.runIn(workspace), write.runIn(workspace)],
  });

  assert.equal(session.id, "ses_12345678901234567890");
  assert.equal(requests.length, 2);
  assert.equal(requests[0].headers.get("authorization"), "Bearer test-token");
  assert.match(requests[0].headers.get("idempotency-key"), /^agent-loop-[0-9a-f]{64}$/u);
  assert.deepEqual([...new Uint8Array(await requests[0].arrayBuffer())], [1, 2, 3]);
  assert.match(requests[1].headers.get("idempotency-key"), /^[0-9a-f-]{36}$/u);
  const body = await requests[1].json();
  assert.deepEqual(body.model, { provider: "vercel-ai-gateway", name: "openai/gpt-5-mini", api_key: "model-secret" });
  assert.equal(body.environments.length, 1);
  assert.equal(body.environments[0].environment_id, "env_1");
  assert.deepEqual(body.tool_bindings.map(({ name, environment_id }) => [name, environment_id]), [["read", "env_1"], ["write", "env_1"]]);
});

test("surfaces structured errors and validates the common path", async () => {
  const brain = new Brain({
    baseUrl: "https://brain.example",
    fetch: async () => Response.json({ code: "conflict", message: "key changed", retryable: false }, { status: 409 }),
  });
  await assert.rejects(brain.listSessions(), (error) => error instanceof BrainError && error.code === "conflict" && error.status === 409);
  assert.throws(() => new Brain({ baseUrl: "" }), /baseUrl is required/u);
  assert.throws(() => new Brain({ baseUrl: "file:\/\/\/tmp\/brain" }), /baseUrl must be HTTP/u);
  assert.throws(() => new Brain({ baseUrl: "https://brain.example", token: "" }), /token cannot be empty/u);
});

test("requires a stable identity only for shared Environments", () => {
  assert.doesNotThrow(() => defineEnvironment({ capability: "workspace", configuration: {} }));
  assert.doesNotThrow(() => defineEnvironment({ capability: "workspace", configuration: {}, lifecycle: { type: "shared", id: "team-workspace" } }));
  assert.throws(
    () => defineEnvironment({ capability: "workspace", configuration: {}, lifecycle: { type: "shared", id: "" } }),
    /requires a stable id/u,
  );
});
