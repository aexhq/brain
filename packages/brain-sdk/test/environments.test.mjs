import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { Brain, agentloop, environment, environmentHandler } from "../dist/index.js";
import { HostToolRegistry } from "../dist/host.js";

test("lifecycle defaults to automatic and grants do not install a management Tool", async () => {
  const requests = [];
  const brain = new Brain({ baseUrl: "https://brain.example", fetch: async (_, init) => {
    requests.push(JSON.parse(init.body));
    return Response.json({ session_id: "session", status: "idle", last_sequence: 1 });
  } });
  const remote = environment({ url: () => "https://env.example" })({ name: "controller" });
  const options = { model: { provider: "openai", name: "test", apiKey: "fixture" }, agentloop: agentloop({ implementation: {} })({ env: remote, environments: [{ environment: "controller", permissions: ["read"], methods: [] }] }) };
  await brain.sessions.create(options);
  assert.deepEqual(requests[0].tools, []);
  assert.equal(requests[0].environments[0].lifecycle, "automatic");
  assert.deepEqual(requests[0].agentloop.environments[0].permissions, ["read"]);
});

test("nested lifecycle defaults and binding overrides compile into explicit wire policies", async () => {
  const requests = [];
  const brain = new Brain({ baseUrl: "https://brain.example", fetch: async (_, init) => {
    requests.push(JSON.parse(init.body));
    return Response.json({ session_id: "session", status: "idle", last_sequence: 1 });
  } });
  const remote = environment({ url: () => "https://env.example" });
  const options = {
    model: { provider: "openai", name: "test", apiKey: "fixture" },
    agentloop: agentloop({ implementation: {} })({ env: remote({ name: "controller" }) }),
    environments: [remote({ name: "workspace" })],
  };
  for (const [configuration, expected] of [
    [{}, ["automatic", "automatic"]],
    [{ lifecycle: {} }, ["automatic", "automatic"]],
    [{ lifecycle: { default: "automatic" } }, ["automatic", "automatic"]],
    [{ lifecycle: { bindings: { workspace: "manual" } } }, ["automatic", "manual"]],
    [{ lifecycle: { default: "manual" } }, ["manual", "manual"]],
    [{ lifecycle: { default: "manual", bindings: { controller: "automatic" } } }, ["automatic", "manual"]],
  ]) {
    await brain.sessions.create({ ...options, environment: configuration });
    assert.deepEqual(requests.at(-1).environments.map(binding => binding.lifecycle), expected);
  }
  const count = requests.length;
  for (const configuration of [
    null, "manual", [], { lifecycle: null }, { lifecycle: "manual" }, { lifecycle: [] },
    { lifecycle: { default: "invalid" } }, { lifecycle: { default: null } },
    { lifecycle: { bindings: null } }, { lifecycle: { bindings: [] } },
    { lifecycle: { bindings: { workspace: "invalid" } } },
  ]) {
    await assert.rejects(brain.sessions.create({ ...options, environment: configuration }), /environment|lifecycle/i);
  }
  await assert.rejects(brain.sessions.create({ ...options, environment: { lifecycle: { bindings: { typo: "manual" } } } }), /undeclared binding/);
  await assert.rejects(brain.sessions.create({ ...options, environmentLifecycle: { default: "manual" } }), /use environment\.lifecycle/);
  assert.equal(requests.length, count);
});

test("an ordinary custom Tool uses scoped Environment services and loses them at finish", async () => {
  const tools = new HostToolRegistry();
  const requests = [];
  let context;
  tools.register("host", { name: "custom_debugger", description: "Custom debugger", input: z.object({}) }, async (_, ctx) => {
    context = ctx;
    const views = await ctx.environments.list();
    return ctx.finish(await ctx.environments.call(views[0].reference, "inventory", {}));
  });
  await tools.run({ sessionId: "session", sequence: 2, environment: "host", name: "custom_debugger", arguments: {},
    environments: async request => { requests.push(request); return request.operation === "list" ? [{ reference: { name: "workspace", sequence: 1 } }] : { processes: 2 }; },
    update: async () => 3, emit: async () => 4,
  });
  assert.deepEqual(requests.map(request => request.operation), ["list", "call"]);
  assert.equal(requests[1].method, "inventory");
  assert.throws(() => context.environments.list(), /finished/);
});

test("Environment handlers share context vocabulary while only their reporter outlives an operation", async t => {
  const calls = [];
  t.mock.method(globalThis, "fetch", async (url, init) => {
    calls.push({ url, token: init.headers.authorization, input: JSON.parse(init.body) });
    return Response.json(String(url).endsWith("events") ? { sequence: 8 } : calls.length);
  });
  let context;
  const handler = environmentHandler({
    options: z.object({ region: z.literal("eu") }),
    methods: {
      inventory: { description: "List resources", input: z.object({}), output: z.object({ region: z.string() }), run: async (_, ctx) => {
        context = ctx;
        await ctx.environments.list();
        await ctx.emitResult({ scope: "resource", resource: "process-1", code: "exited", message: "Process exited" });
        return { region: ctx.options.region };
      } },
    },
  });
  const result = await handler.run({ contract: "environment/v1", operation: {
    session_id: "session", sequence: 2, environment: "workspace", binding: { name: "workspace", sequence: 1 }, configuration: { region: "eu" },
    context: { url: "https://brain.example/call", token: "operation", methods: ["environments", "emit", "result"] },
    reporter: { url: "https://brain.example/events", token: "binding", methods: ["emit", "result"] },
    request: { type: "call", name: "inventory", input: {} },
  } });
  assert.deepEqual(result.receipt, { type: "result", output: { region: "eu" } });
  assert.equal(context.signal.aborted, true);
  assert.throws(() => context.environments.list(), /abort/i);
  assert.equal(await context.reporter.emitResult({ scope: "environment", availability: "unavailable", message: "Provider disconnected" }), 8);
  assert.deepEqual(calls.map(call => call.token), ["Bearer operation", "Bearer operation", "Bearer binding"]);
  assert.equal("inspect" in handler.methods, false);
});
