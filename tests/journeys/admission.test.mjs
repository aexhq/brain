import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { z } from "zod";
import { agentloop, brainWasm, component, inspectAgentloop, inspectComponent, inspectEnvironment } from "@aexhq/brain";
import { fixture, collect } from "./support.mjs";

const f = fixture();

test("file, bytes, and URL admission identify the same executable", { timeout: 30_000 }, async () => {
  const bytes = component(new Uint8Array(await readFile(process.env.BRAIN_TEST_REFERENCE_AGENTLOOP)));
  const remote = component(new URL("/artifact.wasm", f.upstreamUrl));
  const identities = await Promise.all([f.brain.admitAgentloop(f.reference), f.brain.admitAgentloop(bytes), f.brain.admitAgentloop(remote)]);
  assert.equal(new Set(identities).size, 1);
  assert.match(identities[0], /^[a-f0-9]{64}$/u);
  assert.ok(inspectComponent(bytes).artifact instanceof Uint8Array);
});

test("prepare a binding once and create multiple ready-to-use sessions", { timeout: 30_000 }, async (t) => {
  const env = brainWasm();
  const binding = agentloop({ implementation: f.reference })({ env });
  const identity = await f.brain.admit(binding);
  assert.equal(await f.brain.admitAgentloop(f.reference), identity);
  assert.equal(inspectAgentloop(binding).environment, env);
  assert.equal(inspectEnvironment(env).configuration.driver, "brain_wasm");
  const sessions = await Promise.all([f.create(t, { agentloop: binding }), f.create(t, { agentloop: binding })]);
  await Promise.all(sessions.map((session) => session.send("prepared")));
  assert.ok(sessions.every(({ state }) => state.status === "idle"));
});

test("rejected bytes do not prevent a later valid admission", { timeout: 30_000 }, async () => {
  const client = f.client();
  await assert.rejects(client.admitAgentloop(component(new Uint8Array([1, 2, 3]))));
  assert.match(await client.admitAgentloop(f.reference), /^[a-f0-9]{64}$/u);
  await assert.rejects(client.admitTool(f.reference));
  assert.match(await client.admitTool(f.toolComponent), /^[a-f0-9]{64}$/u);
});

test("simultaneous preparation coalesces one artifact upload", { timeout: 30_000 }, async () => {
  let uploads = 0;
  const client = f.client({ fetch: (url, init) => {
    if (String(url).endsWith("/v1/agentloops") && init.method === "POST") uploads++;
    return fetch(url, init);
  } });
  const ids = await Promise.all(Array.from({ length: 3 }, () => client.admitAgentloop(f.reference)));
  assert.equal(new Set(ids).size, 1);
  assert.equal(uploads, 1);
});

test("configured agentloops retain explicit slots across fresh activations", { timeout: 30_000 }, async (t) => {
  const loop = agentloop({ implementation: f.diagnostic, options: z.object({ label: z.string().default("configured") }) });
  const binding = loop({ env: brainWasm() });
  assert.deepEqual(inspectAgentloop(binding).configuration, { label: "configured" });
  const session = await f.create(t, { agentloop: binding });
  await session.send("one");
  await session.send("two");
  assert.deepEqual((await collect(session.events())).filter(({ type }) => type === "turn_ended").map(({ data }) => data.result), [
    { turns: 1, message: "one" }, { turns: 2, message: "two" },
  ]);
  assert.equal(f.modelRequests.length, 0);
});
