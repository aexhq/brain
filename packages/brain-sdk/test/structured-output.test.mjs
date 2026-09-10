import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { Brain, BrainError, SessionHandle, StructuredOutputError } from "../dist/index.js";

function fixture(answers, { pageSize = 1000, origin = { kind: "agentloop", sequence: 2 } } = {}) {
  const posts = [], reads = [], records = [], receipts = new Map();
  const add = (event_type, data, attribution) => records.push({ sequence: records.length + 1, recorded_at_ms: 1, event_type, data, ...(attribution ? { origin: attribution } : {}) });
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async (url, init) => {
    if (url.endsWith("/messages")) {
      const input = JSON.parse(init.body).input;
      const key = new Headers(init.headers).get("idempotency-key");
      posts.push({ input, key });
      if (receipts.has(key)) {
        const previous = receipts.get(key);
        assert.deepEqual(input, previous.input);
        return Response.json(previous.state);
      }
      assert.ok(answers.length, "unexpected correction send");
      const candidate = answers.shift();
      if (candidate instanceof Error) throw candidate;
      const raw = typeof candidate === "function" ? await candidate(input) : candidate;
      add("turn_started", { input });
      add("output_emitted", { type: "assistant_message", message: "unrelated tool output" }, { kind: "tool", sequence: 2 });
      if (raw !== undefined) add("output_emitted", { type: "assistant_message", message: raw }, origin);
      add("turn_ended", { result: null });
      const state = { session_id: "s", status: "idle", last_sequence: records.length };
      receipts.set(key, { input, state });
      return Response.json(state);
    }
    assert.ok(url.includes("/events?"));
    init.signal?.throwIfAborted();
    const after = Number(new URL(url).searchParams.get("after"));
    reads.push(after);
    const events = records.filter(event => event.sequence > after).slice(0, pageSize);
    return Response.json({ events, next_cursor: events.at(-1)?.sequence ?? after });
  } });
  const session = new SessionHandle(client, { id: "s", status: "idle", lastSequence: 0 });
  return { session, client, posts, reads, records, add };
}

test("structured send returns Zod output, preserves media, and scopes the prompt to this response", async () => {
  const f = fixture(['{"name":"Ada","age":"37","extra":true}']);
  const type = z.object({ name: z.string().describe("Full name"), age: z.string().transform(Number) });
  const media = [{ type: "image", url: "https://example.com/person.png" }];
  assert.deepEqual(await f.session.send({ message: "Extract the person", media }, { output: { type } }), { name: "Ada", age: 37 });
  assert.deepEqual(f.posts[0].input.media, media);
  assert.match(f.posts[0].input.message, /Extract the person[\s\S]*For this response only/);
  assert.match(f.posts[0].input.message, /Full name/);
  assert.equal(f.session.state.status, "idle");
});

for (const rejected of [[], ["not json"], ["```json\n{}\n```", '{"age":"wrong"}']]) {
  test(`succeeds after ${rejected.length} corrections with distinct bounded keys`, async () => {
    const f = fixture([...rejected, '{"age":37}']);
    const options = { output: { type: z.object({ age: z.number() }) }, idempotencyKey: "k".repeat(256) };
    assert.deepEqual(await f.session.send("Extract", options), { age: 37 });
    assert.equal(f.posts.length, rejected.length + 1);
    assert.equal(f.posts[0].key, options.idempotencyKey);
    assert.equal(new Set(f.posts.map(p => p.key)).size, f.posts.length);
    assert.ok(f.posts.every(p => p.key.length <= 256));
    for (const post of f.posts.slice(1)) {
      assert.match(post.input.message, /Validation feedback/);
      assert.match(post.input.message, /Do not repeat actions or call tools/);
    }
  });
}

for (const maxRetries of [undefined, 0, 1]) {
  test(`exhaustion respects maxRetries=${maxRetries}`, async () => {
    const count = (maxRetries ?? 2) + 1;
    const f = fixture(Array(count).fill('{"age":"wrong"}'));
    await assert.rejects(f.session.send("Extract", { output: { type: z.object({ age: z.number() }), maxRetries } }), error => {
      assert.ok(error instanceof StructuredOutputError);
      assert.equal(error.attempts, count);
      assert.equal(error.lastOutput, '{"age":"wrong"}');
      assert.deepEqual(error.issues[0].path, ["age"]);
      return true;
    });
    assert.equal(f.posts.length, count);
    assert.equal(f.records.at(-1).event_type, "turn_ended");
  });
}

test("invalid options, unrepresentable schemas and empty input fail before network effects", async () => {
  const f = fixture([]);
  for (const maxRetries of [-1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, "2", null]) {
    await assert.rejects(f.session.send("Extract", { output: { type: z.string(), maxRetries } }), TypeError);
  }
  await assert.rejects(f.session.send("Extract", { output: { type: {} } }), TypeError);
  await assert.rejects(f.session.send("Extract", { output: { type: z.date() } }), /cannot be represented/);
  await assert.rejects(f.session.send("", { output: { type: z.string() } }), TypeError);
  assert.equal(f.posts.length, 0);
});

test("uses exact terminal boundaries across pagination and replay of older receipts", async () => {
  const f = fixture(['"first"', '"later"'], { pageSize: 2 });
  const output = { type: z.string() };
  assert.equal(await f.session.send("First", { output, idempotencyKey: "first" }), "first");
  assert.equal(await f.session.send("Later", { output }), "later");
  assert.equal(await f.session.send("First", { output, idempotencyKey: "first" }), "first");
  assert.ok(f.reads.includes(2), "crossed event pages");
  assert.equal(f.records.filter(r => r.event_type === "turn_started").length, 2);
});

test("replaying the entire correction sequence preserves per-turn idempotency", async () => {
  const f = fixture(['"wrong"', "42"]);
  const options = { output: { type: z.number() }, idempotencyKey: "extract-once" };
  assert.equal(await f.session.send("Extract", options), 42);
  assert.equal(await f.session.send("Extract", options), 42);
  assert.equal(f.records.filter(r => r.event_type === "turn_started").length, 2);
  assert.equal(f.posts[1].key, f.posts[3].key);
});

test("normal sends are unchanged and sequential structured sends can use different schemas", async () => {
  const f = fixture(["ordinary", "null", "[1,2]", '{"name":"Ada"}']);
  assert.equal((await f.session.send("Chat", { idempotencyKey: "plain" })).status, "idle");
  assert.equal(f.posts[0].input.message, "Chat");
  assert.equal(await f.session.send("Nothing", { output: { type: z.null() } }), null);
  assert.deepEqual(await f.session.send("Numbers", { output: { type: z.array(z.number()) } }), [1, 2]);
  assert.deepEqual(await f.session.send("Default", { output: { type: z.object({ name: z.string(), age: z.number().default(37) }) } }), { name: "Ada", age: 37 });
});

test("async refinements correct their failures, while custom-code exceptions propagate", async () => {
  const f = fixture(['"wrong"', '"Ada"']);
  const type = z.string().refine(async value => value === "Ada", "Must be Ada");
  assert.equal(await f.session.send("Name", { output: { type } }), "Ada");
  assert.match(f.posts[1].input.message, /Must be Ada/);
  const bug = new Error("validator bug");
  const broken = fixture(['"Ada"']);
  await assert.rejects(broken.session.send("Name", { output: { type: z.string().transform(() => { throw bug; }) } }), error => error === bug);
  assert.equal(broken.posts.length, 1);
});

for (const options of [{}, { origin: { kind: "tool", sequence: 2 } }]) {
  test(`unsupported output fails without corrections (${JSON.stringify(options)})`, async () => {
    const f = fixture([options.origin ? '"spoofed"' : undefined], options);
    await assert.rejects(f.session.send("Extract", { output: { type: z.string() } }), /requires a completed turn/);
    assert.equal(f.posts.length, 1);
  });
}

test("provider and transport failures never trigger output corrections", async () => {
  for (const error of [new BrainError(503, "model_failed", "unavailable", false), new TypeError("fetch failed")]) {
    const f = fixture([error]);
    await assert.rejects(f.session.send("Extract", { output: { type: z.string() } }), e => e === error);
    assert.equal(f.posts.length, 1);
  }
});

test("cancellation before send or during async validation prevents further turns", async () => {
  const first = fixture([]);
  await assert.rejects(first.session.send("Extract", { output: { type: z.string() }, signal: AbortSignal.abort() }), { name: "AbortError" });
  const controller = new AbortController();
  const f = fixture(['"wrong"']);
  const type = z.string().refine(async () => { controller.abort(); return false; });
  await assert.rejects(f.session.send("Extract", { output: { type }, signal: controller.signal }), { name: "AbortError" });
  assert.equal(f.posts.length, 1);
});

test("structured sends reject overlap in either direction and release ownership on failure", async () => {
  const release = Promise.withResolvers();
  const f = fixture([() => release.promise, "ordinary"]);
  const pending = f.session.send("Extract", { output: { type: z.string(), maxRetries: 0 } });
  await assert.rejects(f.session.send("Overlap"), /exclusive sends/);
  await assert.rejects(f.session.send("Overlap", { output: { type: z.string() } }), /exclusive sends/);
  release.resolve("not json");
  await assert.rejects(pending, StructuredOutputError);
  assert.equal((await f.session.send("Now chat")).status, "idle");
  const done = Promise.withResolvers();
  const ordinary = fixture([() => done.promise]);
  const sending = ordinary.session.send("Chat");
  await assert.rejects(ordinary.session.send("Extract", { output: { type: z.string() } }), /exclusive sends/);
  done.resolve("finished");
  await sending;
});

test("cancellation interrupts an outstanding output-history read", async () => {
  const reading = Promise.withResolvers();
  const controller = new AbortController();
  const client = new Brain({ baseUrl: "https://brain.example", fetch: async (url, init) => {
    if (url.endsWith("/messages")) return Response.json({ session_id: "s", status: "idle", last_sequence: 3 });
    reading.resolve();
    return new Promise((_, reject) => init.signal.addEventListener("abort", () => reject(init.signal.reason), { once: true }));
  } });
  const session = new SessionHandle(client, { id: "s", status: "idle", lastSequence: 0 });
  const pending = session.send("Extract", { output: { type: z.string() }, signal: controller.signal });
  await reading.promise;
  controller.abort();
  await assert.rejects(pending, { name: "AbortError" });
});
