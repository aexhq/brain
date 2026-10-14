import assert from "node:assert/strict";
import test from "node:test";
import { PassThrough } from "node:stream";
import { createInterface } from "node:readline";
import { mkdtemp, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { serveHarness, HARNESS_PROTOCOL } from "../dist/harness.js";

const input = (session_id = "ses_12345678901234567890") => ({ session_id, sequence: 3, model: { id: "test" }, input: {
  input: { message: "hello" }, transcript: [], kv: {}, events: [], configuration: {}, system: "help", tools: [],
  runtime: { logical_time_ms: 1, deterministic_seed: Array(32).fill(0) },
} });
async function fixture(t, harness, callback = async () => null) {
  const directory = await mkdtemp(join(tmpdir(), "brain-protocol-"));
  const incoming = new PassThrough(), outgoing = new PassThrough();
  let next = 1;
  const pending = new Map();
  const frames = [];
  const write = frame => incoming.write(`${JSON.stringify({ jsonrpc: "2.0", ...frame })}\n`);
  const reader = createInterface({ input: outgoing });
  reader.on("line", line => {
    const frame = JSON.parse(line); frames.push(frame);
    if (frame.method === "brain/call") {
      void Promise.resolve().then(() => callback(frame.params)).then(
        result => write({ id: frame.id, result }),
        error => write({ id: frame.id, error: { code: -32000, message: error.message, data: { code: "journal", fatal: true } } }),
      );
    } else {
      const reply = pending.get(frame.id); pending.delete(frame.id);
      if (frame.error) reply.reject(new Error(frame.error.message)); else reply.resolve(frame.result);
    }
  });
  const served = serveHarness(harness, { input: incoming, output: outgoing });
  const request = (method, params) => {
    const id = next++, reply = Promise.withResolvers(); pending.set(id, reply); write({ id, method, params }); return reply.promise;
  };
  t.after(async () => {
    incoming.end(); await served; reader.close(); outgoing.end(); await rm(directory, { recursive: true, force: true });
  });
  assert.deepEqual(await request("initialize", { protocol: HARNESS_PROTOCOL, state_directory: directory, configuration: {} }),
    { protocol: HARNESS_PROTOCOL, name: harness.name, state_version: harness.stateVersion });
  await request("session/open", { session_id: "ses_12345678901234567890" });
  return { request, frames, directory };
}

test("stdio lifecycle scopes callbacks, preserves missing usage, and multiplexes sessions", async t => {
  const seen = [], closed = [];
  const f = await fixture(t, { name: "fixture", stateVersion: "1", async open({ sessionId, stateDirectory }) {
    assert.equal(stateDirectory.endsWith(sessionId), true);
    await writeFile(join(stateDirectory, "native.json"), sessionId);
    return { async run(ctx) {
      assert.equal(ctx.sessionId, sessionId);
      assert.equal(await ctx.kv.get("missing"), undefined);
      await ctx.kv.set("null", null);
      const reply = await ctx.model({ messages: [{ role: "user", content: [{ type: "text", text: "hello" }] }], tools: [] });
      assert.deepEqual(reply.usage, {});
      await ctx.setTranscript([reply.message]);
      return { answer: "hello" };
    }, close() { closed.push(sessionId); } };
  } }, async call => {
    seen.push(call); assert.equal(call.sequence, 3);
    if (call.method === "kv_read") return {};
    if (call.method === "model") return { message: { role: "assistant", content: [{ type: "text", text: "hello" }] }, stop_reason: "end_turn", usage: {} };
    return 4;
  });
  await f.request("session/open", { session_id: "ses_22345678901234567890" });
  assert.deepEqual(await Promise.all([f.request("session/run", input()), f.request("session/run", input("ses_22345678901234567890"))]),
    [{ result: { answer: "hello" } }, { result: { answer: "hello" } }]);
  assert.deepEqual(new Set(seen.map(call => call.session_id)), new Set(["ses_12345678901234567890", "ses_22345678901234567890"]));
  const native = join(f.directory, "ses_12345678901234567890", "native.json");
  await f.request("session/close", { session_id: "ses_12345678901234567890" });
  assert.equal(await readFile(native, "utf8"), "ses_12345678901234567890");
  await f.request("session/delete", { session_id: "ses_12345678901234567890" });
  await assert.rejects(readFile(native), { code: "ENOENT" });
  await f.request("shutdown", {});
  assert.deepEqual(closed.sort(), ["ses_12345678901234567890", "ses_22345678901234567890"]);
});

test("a swallowed fatal callback still fails the activation and prevents later effects", async t => {
  let calls = 0;
  const f = await fixture(t, { name: "fixture", stateVersion: "1", open() { return {
    async run(ctx) {
      await ctx.emit("first", {}).catch(() => undefined);
      await ctx.emit("second", {}).catch(() => undefined);
    }, close() {},
  }; } }, async () => { calls++; throw new Error("commit failed"); });
  await assert.rejects(f.request("session/run", input()), /commit failed/);
  assert.equal(calls, 1);
});

test("cancel is handled while run waits, and a captured context expires", async t => {
  const started = Promise.withResolvers();
  let context;
  const f = await fixture(t, { name: "fixture", stateVersion: "1", open() { return {
    async run(ctx) {
      context = ctx; started.resolve();
      await new Promise(resolve => ctx.signal.addEventListener("abort", resolve, { once: true }));
    }, close() {},
  }; } });
  const run = f.request("session/run", input());
  const rejected = assert.rejects(run, /cancelled/);
  await started.promise;
  await assert.rejects(f.request("session/run", input()), /busy/);
  await f.request("session/cancel", { session_id: "ses_12345678901234567890" });
  await rejected;
  await assert.rejects(context.emit("late", {}), /ended/);
});

test("startup and session lifecycle reject implementation selection and malformed input", async t => {
  const f = await fixture(t, { name: "fixture", stateVersion: "1", open() { return { run() {}, close() {} }; } });
  await assert.rejects(f.request("initialize", {}), /already initialized/);
  await assert.rejects(f.request("session/open", { session_id: "ses_22345678901234567890", command: "anything" }), /Unrecognized/);
  await assert.rejects(f.request("session/run", { ...input(), session_id: "ses_32345678901234567890" }), /not open/);
  await assert.rejects(f.request("session/run", { ...input(), input: {} }));
});
