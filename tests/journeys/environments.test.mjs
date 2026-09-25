import assert from "node:assert/strict";
import test from "node:test";
import { environment, environmentHandler } from "@aexhq/brain";
import { z } from "zod";
import { fixture, collect } from "./support.mjs";

const reporters = new Map();
let setups = 0;
const handler = environmentHandler({
  options: z.strictObject({}),
  setup(context) {
    setups++;
    reporters.set(context.environment.name, context.reporter);
  },
  methods: {
    inspect: { description: "Inspect this test provider", input: z.strictObject({}), output: z.object({ ready: z.boolean() }),
      async run(_, context) {
        await context.emitResult({ scope: "environment", availability: "available", message: "ready" });
        return { ready: true };
      } },
    restart: { description: "Replace this test binding", effect: "replace", input: z.strictObject({}),
      run(_, context) { reporters.set(context.environment.name, context.reporter); return { restarted: true }; } },
  },
});
const f = fixture({ providers: { managed: command => handler.run(command) } });

test("manual HTTP environments keep scoped reporters across Brain restart and revoke old incarnations", { timeout: 60_000 }, async t => {
  const remote = environment({ options: z.strictObject({}), methods: handler.methods,
    url: () => `${f.upstreamUrl}/managed`, credential: () => "managed-token",
  })({ name: "workspace", template: { max_instances: 2, configuration_schema: { type: "object", additionalProperties: false } } });
  const session = await f.create(t, { environments: [remote], environmentLifecycle: { default: "automatic", bindings: { workspace: "manual" } } });
  assert.equal(setups, 0);
  const binding = (await session.environments.list()).find(view => view.reference.name === "workspace");
  assert.equal(binding.state, "declared");
  await session.environments.setup(binding.reference);
  assert.deepEqual(await session.environments.call(binding.reference, "inspect", {}), { ready: true });
  const created = await session.environments.create("workspace", "second", {});
  assert.equal(created.state, "declared");
  await session.environments.setup(created.reference);
  const report = reporters.get("second");
  for (let attempt = 0; attempt < 300; attempt++) {
    if (f.modelRequests.length > 0 && (await f.brain.sessions.get(session.id)).state.status === "idle") break;
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.ok(f.modelRequests.length > 0);
  assert.equal((await f.brain.sessions.get(session.id)).state.status, "idle");
  const before = (await collect(session.events())).filter(event => event.type === "environment_setup_started").length;
  await f.stop();
  await f.start();
  const observation = await report.emitResult({ scope: "resource", resource: "job", code: "stopped", message: "job died after restart" });
  for (let attempt = 0; attempt < 300 && !f.modelRequests.some(request => JSON.stringify(request.input).includes("job died after restart")); attempt++) {
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.ok(f.modelRequests.some(request => JSON.stringify(request.input).includes("job died after restart")),
    JSON.stringify((await collect(session.events())).filter(event => event.type.endsWith("_failed"))));
  const events = await collect(session.events());
  assert.equal(events.filter(event => event.type === "environment_setup_started").length, before);
  assert.deepEqual(events.find(event => event.sequence === observation).origin, { kind: "environment", environment: created.reference });
  await session.environments.call(created.reference, "restart", {});
  await assert.rejects(() => report.emitResult({ scope: "resource", resource: "old-job", code: "stopped", message: "stale" }), error => error.status === 404);
  const current = (await session.environments.list()).find(view => view.reference.name === "second");
  assert.notEqual(current.reference.sequence, created.reference.sequence);
  await session.environments.delete(current.reference);
  await assert.rejects(() => reporters.get("second").emitResult({ scope: "resource", resource: "job", code: "stopped", message: "closed" }), error => error.status === 404);
});
