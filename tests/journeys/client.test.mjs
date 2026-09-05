import assert from "node:assert/strict";
import test from "node:test";
import { Brain, BrainClient, BrainError } from "@aexhq/brain";
import { fixture, failure } from "./support.mjs";

const f = fixture();

test("authenticate a client and derive another without mutating the first", { timeout: 30_000 }, async () => {
  assert.equal(Brain, BrainClient);
  const anonymous = new Brain({ baseUrl: `${f.baseUrl}///` });
  assert.equal(anonymous.baseUrl, f.baseUrl);
  await assert.rejects(anonymous.sessions.list(), failure(401));
  assert.ok(Array.isArray(await anonymous.withToken(f.token).sessions.list()));
  await assert.rejects(anonymous.sessions.list(), failure(401));
  await assert.rejects(f.brain.withToken("wrong-token").sessions.list(), failure(401));
});

test("custom transport observes real requests and structured errors remain actionable", { timeout: 30_000 }, async (t) => {
  const paths = [];
  const client = f.client({ fetch: (url, init) => { paths.push(new URL(url).pathname); return fetch(url, init); } });
  const session = await f.create(t, {}, client);
  assert.equal((await client.request("GET", `/v1/sessions/${session.id}`)).session_id, session.id);
  await session.end();
  await session.delete();
  await assert.rejects(session.transcript(), (error) => {
    assert.ok(error instanceof BrainError);
    assert.equal(error.status, 404);
    assert.equal(error.code, "not_found");
    assert.equal(error.retryable, false);
    assert.ok(error.message.length > 0);
    return true;
  });
  assert.ok(paths.includes(`/v1/sessions/${session.id}/transcript`));
});

test("invalid configuration fails before any network work", async () => {
  for (const options of [{ baseUrl: "" }, { baseUrl: "file:///tmp" }, { baseUrl: f.baseUrl, token: " " }, { baseUrl: f.baseUrl, timeoutMs: 0 }]) {
    assert.throws(() => new Brain(options), TypeError);
  }
  let requests = 0;
  const client = f.client({ fetch: (...args) => { requests++; return fetch(...args); } });
  await assert.rejects(client.sessions.create(f.options({ model: { provider: "openai", name: "test", apiKey: "" } })), TypeError);
  assert.equal(requests, 0);
});
