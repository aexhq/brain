import assert from "node:assert/strict";
import test from "node:test";

import { Brain, component } from "../dist/index.js";

test("session creation uploads shared component bytes once and seals independent bindings", async () => {
  let body;
  const client = new Brain({
    token: "test-token",
    baseUrl: "https://brain.invalid",
    async fetch(_input, init) {
      body = JSON.parse(init.body);
      return new Response(JSON.stringify({
        id: "ses_12345678901234567890",
        root_id: "ses_12345678901234567890",
        depth: 0,
        object: "session",
        state: "open",
        turn_state: "idle",
        turns: 0,
        last_seq: 1,
        created_at: "2026-08-25T00:00:00Z",
        updated_at: "2026-08-25T00:00:00Z",
        metadata: {},
        shape: "1gb",
        storage: { session_storage_bytes: 0, upload_reserved_bytes: 0 },
        model: {
          component_digest: "0".repeat(64),
          world: "aex:model/model@1.0.0",
          provider: "fixture",
          name: "fixture-model",
          context_window_tokens: 32768,
        },
      }), { status: 201, headers: { "content-type": "application/json" } });
    },
  });
  const bytes = new Uint8Array([0, 97, 115, 109]);

  await client.sessions.create({
    model: {
      component: component("model", bytes, { endpoint: "fixture" }, { metadata: { name: "fixture" } }),
      name: "fixture-model",
      apiKey: "secret",
    },
    agentloop: component("agentloop", bytes, { policy: "sequential" }),
  });

  assert.equal(body.component_artifacts.length, 1);
  assert.equal(body.model.component_digest, body.agentloop.component_digest);
  assert.equal(body.model.provider, "fixture");
  assert.deepEqual(body.model.config, { endpoint: "fixture" });
  assert.deepEqual(body.agentloop.config, { policy: "sequential" });
});
