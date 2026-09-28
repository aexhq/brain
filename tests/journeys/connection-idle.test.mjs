import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { tool } from "@aexhq/brain";
import { fixture, callTools, reply, deferred } from "./support.mjs";

const f = fixture();
const dispatch = (name, input) => (request, response) => request.input.at(-1).type === "function_call_output"
  ? reply(response) : callTools(response, [{ name, input }]);

test("idle host connections wait for delayed model work and wake once for two sessions", { timeout: 30_000 }, async t => {
  const sleeping = deferred();
  const entered = deferred();
  const release = deferred();
  let connections = 0;
  let suspensions = 0;
  const client = f.client({ connectionIdleTimeoutMs: 25, fetch: async (url, options) => {
    if (new URL(url).pathname.endsWith("/commands")) connections++;
    const response = await fetch(url, options);
    if (new URL(url).pathname.endsWith("/suspend") && (await response.clone().json()).suspended) {
      suspensions++;
      sleeping.resolve();
    }
    return response;
  } });
  let calls = 0;
  const lookup = tool({ name: "lookup", description: "Retained closure", input: z.object({}), run: (_, ctx) => ctx.finish(++calls) });
  const first = await f.create(t, { tools: [lookup()] }, client);
  const second = await f.create(t, { tools: [lookup()] }, client);
  const credentials = await client.credentials();
  f.model = (request, response) => {
    if (request.input.at(-1).type === "function_call_output") return reply(response);
    entered.resolve();
    void release.promise.then(() => callTools(response, [{ name: "lookup", input: {} }]));
  };
  await first.submit("wait for the model");
  await entered.promise;
  await new Promise(resolve => setTimeout(resolve, 100));
  assert.equal(suspensions, 0);
  release.resolve();
  await sleeping.promise;
  assert.equal(calls, 1);
  assert.equal(connections, 1);
  await first.transcript();
  assert.equal(connections, 1);
  f.model = dispatch("lookup", {});
  await Promise.all([first.send("wake first"), second.send("wake second")]);
  assert.equal(calls, 3);
  assert.equal(connections, 2);
  assert.deepEqual(await client.credentials(), credentials);
  await client.close();
});
