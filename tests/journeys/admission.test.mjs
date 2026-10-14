import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { z } from "zod";
import { brainEnv, component, inspectTool, inspectComponent, inspectEnvironment, tool } from "@aexhq/brain";
import { fixture } from "./support.mjs";

const f = fixture();

test("file, bytes, and URL admission identify the same Tool executable", { timeout: 30_000 }, async () => {
  const bytes = component(new Uint8Array(await readFile(process.env.BRAIN_TEST_TOOL_COMPONENT)));
  const remote = component(new URL("/artifact.wasm", f.upstreamUrl));
  const ids = await Promise.all([f.brain.admitTool(f.toolComponent), f.brain.admitTool(bytes), f.brain.admitTool(remote)]);
  assert.equal(new Set(ids).size, 1);
  assert.match(ids[0], /^[a-f0-9]{64}$/u);
  assert(inspectComponent(bytes).artifact instanceof Uint8Array);
});

test("prepare a Tool placement once and use it in multiple sessions", { timeout: 30_000 }, async t => {
  const env = brainEnv({ name: "brain" });
  const placed = tool({ name: "test", description: "Test", input: z.object({}), implementation: f.toolComponent })({ env });
  const prepared = await f.brain.prepare(placed);
  assert.equal(inspectTool(prepared).implementation.id, await f.brain.admitTool(f.toolComponent));
  assert.deepEqual(inspectEnvironment(env).driver, { driver: "brain" });
  const sessions = await Promise.all([f.create(t, { tools: [prepared] }), f.create(t, { tools: [prepared] })]);
  await Promise.all(sessions.map(session => session.send("prepared")));
  assert(sessions.every(({ state }) => state.status === "idle"));
});

test("rejected bytes do not prevent a later valid Tool admission", { timeout: 30_000 }, async () => {
  const client = f.client();
  await assert.rejects(client.admitTool(component(new Uint8Array([1, 2, 3]))));
  assert.match(await client.admitTool(f.toolComponent), /^[a-f0-9]{64}$/u);
});

test("simultaneous preparation coalesces one artifact upload", { timeout: 30_000 }, async () => {
  let uploads = 0;
  const client = f.client({ fetch: (url, init) => {
    if (String(url).endsWith("/v1/tools") && init.method === "POST") uploads++;
    return fetch(url, init);
  } });
  const ids = await Promise.all(Array.from({ length: 3 }, () => client.admitTool(f.toolComponent)));
  assert.equal(new Set(ids).size, 1);
  assert.equal(uploads, 1);
});
