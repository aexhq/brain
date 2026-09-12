import assert from "node:assert/strict";
import test from "node:test";
import { HostToolRegistry } from "../dist/host.js";

import { z } from "zod";
import {
  agentloop, brainEnv, component, environment, hostEnv, inspectAgentloop, inspectEnvironment,
  inspectTool, tool,
} from "../dist/index.js";

test("extensions are immutable factories with explicit placement", () => {
  const wasm = component(new Uint8Array([0, 97, 115, 109]));
  const native = brainEnv({ name: "brain", secrets: ["MODEL_TOKEN"] });
  assert.deepEqual(inspectEnvironment(native), {
    kind: "environment",
    name: "brain",
    driver: { driver: "brain" },
    configuration: { secrets: ["MODEL_TOKEN"] },
  });
  assert.deepEqual(inspectEnvironment(hostEnv({ name: "app" })).driver, { driver: "host" });

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
  });
  const placed = read({ env: native });
  assert.equal(inspectTool(placed).implementation, wasm);
  assert.equal(inspectTool(placed).handler, undefined);
  assert.equal(Object.isFrozen(placed), true);
});

test("a Tool with run stays in the declaring process and receives options", async () => {
  const seen = [];
  const lookup = tool({
    name: "lookup",
    description: "Look up a value.",
    input: z.object({ id: z.string() }),
    output: z.object({ value: z.string() }),
    options: z.object({ prefix: z.string() }),
    run: async ({ id }, ctx) => {
      seen.push([ctx.options.prefix, id, ctx.sequence]);
      return { value: `${ctx.options.prefix}${id}` };
    },
  });
  const placed = lookup({ env: hostEnv({ name: "app" }), prefix: ">" });
  const source = inspectTool(placed);
  assert.equal(source.implementation, undefined);
  assert.deepEqual(await source.handler({ id: "1" }, {
    sequence: 7,
    deadline: new Date(),
    signal: new AbortController().signal,
    emit: async () => 1,
  }), { value: ">1" });
  assert.deepEqual(seen, [[">", "1", 7]]);
});

test("an environment extension configures each instance and says how it is reached", () => {
  const remote = environment({
    options: z.object({ url: z.url(), region: z.string(), token: z.string().optional() }),
    url: ({ url }) => url,
    credential: ({ token }) => token,
    configure: ({ region }) => ({ region }),
  });
  const value = remote({ name: "sandbox", url: "https://tool.example", region: "eu", token: "secret" });
  const source = inspectEnvironment(value);
  assert.equal(source.name, "sandbox");
  assert.deepEqual(source.driver, { driver: "http", url: "https://tool.example", credential: "secret" });
  assert.deepEqual(source.configuration, { region: "eu" });
  const anonymous = remote({ name: "other", url: "https://tool.example", region: "us" });
  assert.deepEqual(inspectEnvironment(anonymous).driver, { driver: "http", url: "https://tool.example" });
  assert.throws(() => remote({ url: "https://tool.example", region: "eu" }), /requires \{ name \}/u);
  assert.throws(() => remote({ name: "bad name", url: "https://tool.example", region: "eu" }), /identifier/u);
  assert.throws(() => environment({ url: () => "ftp://tool.example" })({ name: "x" }), /HTTP\(S\)/u);
  assert.throws(() => environment({ url: () => "https://user:pw@tool.example" })({ name: "x" }), /credentials/u);
});

test("the built-in Environments validate their own grant options", () => {
  const wasm = component(new Uint8Array([1]));
  assert.throws(() => tool({ name: "t", description: "d", input: z.object({}) }), /exactly one of run or implementation/u);
  assert.throws(() => tool({ name: "t", description: "d", input: z.object({}), implementation: wasm, run: async () => null }), /exactly one of run or implementation/u);
  assert.throws(() => brainEnv({ name: "brain", secrets: ["bad/name"] }), /invalid name/u);
  assert.throws(() => brainEnv({ name: "brain", network: { allow: [] } }), /array/u);
  assert.throws(() => brainEnv({}), /name/u);
  assert.throws(() => hostEnv({ name: "../x" }), /identifier/u);
});

test("native grants are explicit, validated, and independent between bindings", () => {
  const configured = brainEnv({ name: "writer", filesystem: { workspace: "write", scratch: "read" }, network: ["https://*.example.com"] });
  assert.deepEqual(inspectEnvironment(configured).configuration, {
    filesystem: { workspace: "write", scratch: "read" }, network: ["https://*.example.com"],
  });
  assert.deepEqual(inspectEnvironment(brainEnv({ name: "other" })).configuration, {});
  for (const network of [["file:///workspace"], ["https://example.com/path"], ["https://user:password@example.com"], ["https://example.com?x"]]) {
    assert.throws(() => brainEnv({ name: "bad", network }), /HTTP\(S\) origins/u);
  }
  assert.throws(() => brainEnv({ name: "bad", filesystem: { workspace: "root" } }), /read/u);
  assert.throws(() => brainEnv({ name: "bad", filesystem: { home: "read" } }), /Unrecognized/u);
});

for (const strict of [false, true]) {
  test(`Tool input schemas describe accepted ${strict ? "strict" : "ordinary"} inputs before parsing`, async () => {
    let called = 0;
    const shape = { text: z.string().transform(value => value.toUpperCase()), limit: z.number().default(10) };
    const input = strict ? z.strictObject(shape) : z.object(shape);
    const lookup = tool({ name: "lookup", description: "Look up values.", input,
      output: z.object({ text: z.string(), limit: z.number().default(10) }),
      run: value => { called++; return value; },
    })({ env: hostEnv({ name: "app" }) });
    const source = inspectTool(lookup);
    assert.deepEqual(source.definition.inputSchema.required, ["text"]);
    assert.equal(source.definition.inputSchema.properties.text.type, "string");
    assert.equal(source.definition.inputSchema.additionalProperties, strict ? false : undefined);
    assert.deepEqual(source.definition.outputSchema.required, ["text", "limit"]);
    const registry = new HostToolRegistry();
    registry.register("app", source.contract, source.handler);
    const invoke = arguments_ => registry.run({ sessionId: "session", environment: "app", sequence: 1,
      name: "lookup", arguments: arguments_, deadline_ms: 1_000, emit: async () => 1,
    });
    assert.deepEqual(await invoke({ text: "cyan" }), { status: "ok", value: { text: "CYAN", limit: 10 } });
    assert.equal((await invoke({})).error.code, "invalid_input");
    assert.equal(called, 1);
    const extra = await invoke({ text: "blue", extra: true });
    if (strict) assert.equal(extra.error.code, "invalid_input");
    else assert.deepEqual(extra, { status: "ok", value: { text: "BLUE", limit: 10 } });
  });
}

test("unrepresentable Tool schemas fail during authoring", () => {
  assert.throws(() => tool({ name: "date", description: "Date.", input: z.object({ date: z.date() }), run: () => null }), /Date/u);
});
