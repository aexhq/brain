import assert from "node:assert/strict";
import test from "node:test";

import { z } from "zod";
import {
  agentloop, brainWasm, component, environment, inspectAgentloop, inspectEnvironment,
  inspectPlacedTool, inspectResidentTool, tool,
} from "../dist/index.js";

test("extensions are immutable factories with explicit placement", () => {
  const wasm = component(new Uint8Array([0, 97, 115, 109]));
  const native = brainWasm({
    network: { allow: ["api.example.com", "https://models.example.com/"] },
    filesystem: { scratch: true, workspace: false },
    secrets: ["MODEL_TOKEN"],
  });
  assert.deepEqual(inspectEnvironment(native).configuration, {
    driver: "brain_wasm",
    network: { allow: ["api.example.com", "https://models.example.com"] },
    filesystem: { scratch: true, workspace: false },
    secrets: ["MODEL_TOKEN"],
  });

  const pi = agentloop({ options: z.object({ compactAt: z.number() }), implementation: wasm });
  const loop = pi({ env: native, compactAt: 0.8 });
  assert.deepEqual(inspectAgentloop(loop).configuration, { compactAt: 0.8 });
  assert.equal(inspectAgentloop(loop).environment, native);
  assert.equal("use" in loop, false);

  const read = tool({
    name: "read",
    description: "Read a file.",
    input: z.object({ path: z.string() }),
    implementation: wasm,
    needs: ["fs"],
  });
  const placed = read({ env: native });
  assert.equal(inspectPlacedTool(placed).implementation, wasm);
  assert.deepEqual(inspectPlacedTool(placed).needs, ["fs"]);
  assert.equal(Object.isFrozen(placed), true);
});

test("resident Tools stay in the declaring process and receive options", async () => {
  const seen = [];
  const lookup = tool({
    name: "lookup",
    description: "Look up a value.",
    input: z.object({ id: z.string() }),
    output: z.object({ value: z.string() }),
    options: z.object({ prefix: z.string() }),
    run: async ({ id }, ctx) => {
      seen.push([ctx.options.prefix, id]);
      return { value: `${ctx.options.prefix}${id}` };
    },
  });
  const bound = lookup({ prefix: ">" });
  const resident = inspectResidentTool(bound);
  assert.ok(resident);
  assert.deepEqual(await resident.handler({ id: "1" }, {
    callId: "call_1",
    deadline: new Date(),
    signal: new AbortController().signal,
    emit: async () => 1,
  }), { value: ">1" });
  assert.deepEqual(seen, [[">", "1"]]);
});

test("environment factories serialize only configuration and bindings", () => {
  const remote = environment({
    driver: "remote_service",
    options: z.object({ url: z.url(), token: z.string() }),
    configure: ({ url }) => ({ url }),
    bindings: ({ token }) => ({ API_TOKEN: token }),
  });
  const value = remote({ url: "https://tool.example", token: "secret" });
  const source = inspectEnvironment(value);
  assert.deepEqual(source.configuration, { driver: "remote_service", url: "https://tool.example" });
  assert.deepEqual(source.bindings, { API_TOKEN: "secret" });
});

test("invalid native capabilities fail at declaration", () => {
  assert.throws(() => brainWasm({ network: { allow: ["ftp://example.com"] } }), /HTTP\(S\)/u);
  assert.throws(() => brainWasm({ network: { allow: ["https://example.com/path"] } }), /origin or authority/u);
  assert.throws(() => brainWasm({ secrets: ["bad/name"] }), /identifiers/u);
  assert.throws(() => brainWasm({ filesystem: { scratch: "yes" } }), /boolean scratch/u);
  assert.throws(() => brainWasm({ filesystem: { root: true } }), /boolean scratch/u);
});
