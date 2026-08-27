import assert from "node:assert/strict";
import test from "node:test";

import { BrainClient, BrainError } from "../dist/index.js";

test("uses the configured base URL, binary admission body, auth, and idempotency", async () => {
  let request;
  const client = new BrainClient({
    baseUrl: "https://brain.example/",
    apiKey: "test-token",
    fetch: async (input, init) => {
      request = new Request(input, init);
      return Response.json({ digest: "a".repeat(64), status: "admitted" });
    },
  });
  const admission = await client.admitAgentloop(new Uint8Array([1, 2, 3]), "admit-1");
  assert.equal(admission.status, "admitted");
  assert.equal(request?.url, "https://brain.example/v1/agentloops");
  assert.equal(request?.headers.get("authorization"), "Bearer test-token");
  assert.equal(request?.headers.get("idempotency-key"), "admit-1");
  assert.equal(request?.headers.get("content-type"), "application/octet-stream");
  assert.deepEqual([...new Uint8Array(await request?.arrayBuffer())], [1, 2, 3]);
});

test("surfaces the structured Brain error", async () => {
  const client = new BrainClient({
    baseUrl: "https://brain.example",
    fetch: async () => Response.json(
      { code: "conflict", message: "key changed", retryable: false },
      { status: 409 },
    ),
  });
  await assert.rejects(
    client.listSessions(),
    (error) => error instanceof BrainError && error.code === "conflict" && error.status === 409,
  );
});

test("rejects an empty base URL", () => {
  assert.throws(() => new BrainClient({ baseUrl: "" }), /baseUrl is required/u);
  assert.throws(() => new BrainClient({ baseUrl: "file:///tmp/brain" }), /baseUrl must be HTTP/u);
  assert.throws(() => new BrainClient({ baseUrl: "https://user:secret@brain.example" }), /baseUrl must be HTTP/u);
  assert.throws(() => new BrainClient({ baseUrl: "https://brain.example", apiKey: "" }), /apiKey cannot be empty/u);
  assert.throws(() => new BrainClient({ baseUrl: "https://brain.example", timeoutMs: 0 }), /timeoutMs must be/u);
});
