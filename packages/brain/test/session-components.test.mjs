import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";

import { Brain, component } from "../dist/index.js";

test("session creation wires the model dialect and uploads only component bytes", async () => {
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
          dialect: "anthropic",
          base_url: "https://api.anthropic.com/v1",
          name: "fixture-model",
          context_window_tokens: 32768,
        },
      }), { status: 201, headers: { "content-type": "application/json" } });
    },
  });
  const bytes = new Uint8Array([0, 97, 115, 109]);

  await client.sessions.create({
    model: {
      dialect: "anthropic",
      baseUrl: "https://api.anthropic.com/v1",
      name: "fixture-model",
      apiKey: "secret",
    },
    agentloop: component("agentloop", bytes, { policy: "sequential" }),
  });

  assert.equal(body.component_artifacts.length, 1, "only the Agentloop uploads bytes");
  assert.equal(body.model.dialect, "anthropic");
  assert.equal(body.model.base_url, "https://api.anthropic.com/v1");
  assert.equal(body.model.component_digest, undefined, "a session binds no Model component");
  assert.deepEqual(body.agentloop.config, { policy: "sequential" });
});

test("session creation binds ordinary Tool and Environment component values", async () => {
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
          dialect: "anthropic",
          base_url: "https://api.anthropic.com/v1",
          name: "fixture-model",
          context_window_tokens: 32768,
        },
      }), { status: 201, headers: { "content-type": "application/json" } });
    },
  });
  const bytes = new Uint8Array([0, 97, 115, 109]);
  const definition = {
    name: "echo",
    contract_digest: "a".repeat(64),
    input_schema: { type: "object" },
    output_schema: { type: "object" },
  };

  await client.sessions.create({
    model: {
      dialect: "anthropic",
      baseUrl: "https://api.anthropic.com/v1",
      name: "fixture-model",
      apiKey: "secret",
    },
    agentloop: component("agentloop", bytes, {}),
    tools: [component("tool", bytes, { definition, mode: "echo" }, { grants: ["environment"] })],
    environments: { workspace: component("environment", bytes, { provider: "fixture" }) },
  });

  assert.equal(body.component_artifacts.length, 1);
  assert.deepEqual(body.tools.items, [{
    definition,
    executor: {
      kind: "component",
      component_digest: body.agentloop.component_digest,
      world: "aex:tool/tool@1.0.0",
      config: { definition, mode: "echo" },
      grants: ["environment"],
      environment: "workspace",
    },
  }]);
  assert.deepEqual(body.environments.workspace, {
    component_digest: body.agentloop.component_digest,
    world: "aex:environment/environment@1.0.0",
    config: { provider: "fixture" },
  });
});

test("a Tool Environment bundle rides a content-addressed layer, never the sealed config", async () => {
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
          dialect: "anthropic",
          base_url: "https://api.anthropic.com/v1",
          name: "fixture-model",
          context_window_tokens: 32768,
        },
      }), { status: 201, headers: { "content-type": "application/json" } });
    },
  });
  const bytes = new Uint8Array([0, 97, 115, 109]);
  const bundle = new TextEncoder().encode("export default () => 'tool';\n");
  const digest = createHash("sha256").update(bundle).digest("hex");
  const definition = {
    name: "echo",
    contract_digest: "a".repeat(64),
    input_schema: { type: "object" },
    output_schema: { type: "object" },
  };

  await client.sessions.create({
    model: {
      dialect: "anthropic",
      baseUrl: "https://api.anthropic.com/v1",
      name: "fixture-model",
      apiKey: "secret",
    },
    agentloop: component("agentloop", bytes, {}),
    tools: [
      component("tool", bytes, { definition }, { grants: ["environment"], bundle }),
    ],
    environments: { workspace: component("environment", bytes, {}) },
  });

  assert.equal(body.tools.items[0].executor.bundle_digest, digest);
  assert.deepEqual(body.tools.items[0].executor.config, { definition });
  assert.deepEqual(body.tool_artifact_layers, [{
    checksum: digest,
    content_base64: Buffer.from(bundle).toString("base64"),
    bytes: bundle.byteLength,
    media_type: "application/javascript+esm",
  }]);
});

test("an Environment bundle without the environment grant is refused", () => {
  const bytes = new Uint8Array([0, 97, 115, 109]);
  assert.throws(
    () => component("tool", bytes, {}, { bundle: new Uint8Array([1]) }),
    /environment Tool context grant/u,
  );
});
