import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";

import {
  component,
  componentContracts,
  defineComponent,
  prepareComponent,
  prepareComponents,
} from "../dist/index.js";

test("component factories are declarative and wire preparation seals the bytes", async () => {
  const bytes = new Uint8Array([0, 97, 115, 109]);
  const options = { instructions: "be concise" };
  const pi = defineComponent({
    kind: "agentloop",
    asset: bytes,
    metadata: { name: "pi", source: "@aexhq/loop-pi" },
    configure(options) {
      return { instructions: options.instructions };
    },
  });
  const value = pi(options);
  options.instructions = "mutated";
  bytes[0] = 9;
  assert.equal(value.kind, "brain.component");
  assert.equal(value.world, componentContracts.agentloop.world);
  assert.deepEqual(value.config, { instructions: "be concise" });
  assert.ok(Object.isFrozen(value));
  assert.ok(Object.isFrozen(value.config));

  const wire = await prepareComponent(value);
  const original = new Uint8Array([0, 97, 115, 109]);
  assert.equal(wire.component_digest, createHash("sha256").update(original).digest("hex"));
  assert.equal(wire.component_base64, Buffer.from(original).toString("base64"));
  assert.equal(wire.contract_digest, componentContracts.agentloop.digest);
});

test("Tool context grants are explicit, unique, and Tool-only", () => {
  const value = component("tool", new Uint8Array([1]), {}, { grants: ["storage", "children"] });
  assert.deepEqual(value.grants, ["children", "storage"]);
  assert.throws(
    () => component("tool", new Uint8Array([1]), {}, { grants: ["storage", "storage"] }),
    /repeated/u,
  );
  assert.throws(
    () => component("environment", new Uint8Array([1]), {}, { grants: ["storage"] }),
    /cannot request/u,
  );
});

test("component config rejects values that cannot cross the JSON boundary", () => {
  assert.throws(() => component("environment", new Uint8Array([1]), { value: 1n }), /JSON serializable/u);
  assert.throws(() => component("environment", new Uint8Array([1]), undefined), /JSON serializable/u);
});

test("component preparation deduplicates artifacts without merging bindings", async () => {
  const bytes = new Uint8Array([1, 2, 3]);
  const prepared = await prepareComponents([
    component("tool", bytes, { name: "read" }, { grants: ["environment"] }),
    component("tool", bytes, { name: "write" }, { grants: ["environment"] }),
  ]);
  assert.equal(prepared.artifacts.length, 1);
  assert.equal(prepared.bindings.length, 2);
  assert.equal(prepared.bindings[0].component_digest, prepared.bindings[1].component_digest);
  assert.notDeepEqual(prepared.bindings[0].config, prepared.bindings[1].config);
});
