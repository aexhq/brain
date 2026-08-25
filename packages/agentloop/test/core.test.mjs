import assert from "node:assert/strict";
import { test } from "node:test";
import { AgentloopOpError, __bindHostCall, defineAgentloop } from "../dist/index.js";

/** A scripted host: records every envelope and answers from a queue keyed by op name. */
function scriptedHost(answers) {
  const requests = [];
  __bindHostCall((payload) => {
    const request = JSON.parse(payload);
    requests.push(request);
    const op = request.op.op;
    const next = answers[op]?.shift();
    if (!next) {
      throw new Error(`unscripted ctx op ${op}`);
    }
    return JSON.stringify(next);
  });
  return requests;
}

const session = {
  session_id: "ses_00000000000000000001",
  model: "claude-test",
  limits: { max_rounds_per_turn: 512, turn_wall_ms: 1000, max_parallel_tools: 8 },
};

function messagePayload() {
  return JSON.stringify({
    kind: "message",
    activation_id: "act-1",
    session,
    message: { seq: 4, at: "2026-08-21T00:00:00Z", content: [{ type: "text", text: "go" }] },
  });
}

test("a handler drives typed ops and returning cleanly finishes the turn", async () => {
  const requests = scriptedHost({
    kv_get: [{ op_id: "x", result: { op: "kv_get", entries: { n: 2 } } }],
    kv_set: [{ op_id: "x", result: { op: "kv_set" } }],
    model_stream: [
      {
        op_id: "x",
        result: {
          op: "model_stream",
          message: {
            content: [{ type: "text", text: "answer" }],
            stop_reason: "end_turn",
            model: "claude-test",
          },
        },
      },
    ],
    turn_finish: [{ op_id: "x", result: { op: "turn_finish" } }],
  });
  const seen = {};
  const { activate } = defineAgentloop({
    async onMessage(ctx, message) {
      seen.messageText = message.content[0].text;
      seen.model = ctx.session.model;
      const kv = await ctx.kv.get(["n"]);
      await ctx.kv.set({ n: kv.n + 1 });
      const round = await ctx.model.stream({
        messages: [{ role: "user", content: message.content }],
      });
      seen.answer = round.content[0].text;
      // No explicit finish: returning is finishing.
    },
  });
  const returned = JSON.parse(await activate("message", messagePayload()));
  assert.equal(returned.outcome, "completed");
  assert.deepEqual(seen, { messageText: "go", model: "claude-test", answer: "answer" });
  const ops = requests.map((request) => request.op.op);
  assert.deepEqual(ops, ["kv_get", "kv_set", "model_stream", "turn_finish"]);
  assert.equal(new Set(requests.map((request) => request.op_id)).size, requests.length);
  for (const request of requests) {
    assert.equal(request.activation_id, "act-1");
  }
});

test("a thrown handler error fails the turn with the message", async () => {
  const requests = scriptedHost({
    turn_fail: [{ op_id: "x", result: { op: "turn_fail" } }],
  });
  const { activate } = defineAgentloop({
    async onMessage() {
      throw new Error("policy exploded");
    },
  });
  const returned = JSON.parse(await activate("message", messagePayload()));
  assert.equal(returned.outcome, "failed");
  assert.equal(returned.error.message, "policy exploded");
  assert.equal(requests[0].op.op, "turn_fail");
  assert.equal(requests[0].op.error.message, "policy exploded");
});

test("typed op errors surface with their code and can be handled", async () => {
  scriptedHost({
    tools_dispatch: [
      {
        op_id: "x",
        error: { code: "unsealed_tool", message: "tool nope is not sealed", retryable: false },
      },
    ],
    turn_finish: [{ op_id: "x", result: { op: "turn_finish" } }],
  });
  let caught = null;
  const { activate } = defineAgentloop({
    async onMessage(ctx) {
      try {
        await ctx.tools.dispatch([{ tool_call_id: "c1", name: "nope", input: {} }]);
      } catch (error) {
        caught = error;
      }
    },
  });
  const returned = JSON.parse(await activate("message", messagePayload()));
  assert.equal(returned.outcome, "completed");
  assert.ok(caught instanceof AgentloopOpError);
  assert.equal(caught.code, "unsealed_tool");
});

test("a return_direct terminal makes the implicit finish a clean no-op", async () => {
  scriptedHost({
    tools_dispatch: [
      {
        op_id: "x",
        result: {
          op: "tools_dispatch",
          results: [{ tool_call_id: "c1", name: "emit", is_error: false, content: [] }],
        },
      },
    ],
    turn_finish: [
      {
        op_id: "x",
        error: {
          code: "turn_already_terminal",
          message: "the turn already has a terminal",
          retryable: false,
        },
      },
    ],
  });
  const { activate } = defineAgentloop({
    async onMessage(ctx) {
      await ctx.tools.dispatch([{ tool_call_id: "c1", name: "emit", input: {} }]);
    },
  });
  const returned = JSON.parse(await activate("message", messagePayload()));
  assert.equal(returned.outcome, "completed");
});

test("session_start hydration reaches the handler and later message ctx", async () => {
  scriptedHost({ turn_finish: [{ op_id: "x", result: { op: "turn_finish" } }] });
  let started = null;
  let ctxStart = null;
  const { activate } = defineAgentloop({
    onSessionStart(start) {
      started = start;
    },
    async onMessage(ctx) {
      ctxStart = ctx.start;
    },
  });
  const startPayload = JSON.stringify({
    kind: "session_start",
    activation_id: "act-0",
    session,
    resumed: true,
    kv: { n: 7 },
    tail: [],
  });
  const startReturn = JSON.parse(await activate("session_start", startPayload));
  assert.equal(startReturn.outcome, "completed");
  assert.equal(started.resumed, true);
  await activate("message", messagePayload());
  assert.equal(ctxStart.kv.n, 7);
});
