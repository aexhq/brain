import assert from "node:assert/strict";
import test from "node:test";

import { z } from "zod";

import { AttachedWorker } from "../dist/attached.js";
import { defineTool } from "../dist/index.js";

class FakeSocket {
  listeners = new Map();
  sent = [];
  closed = false;

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  send(text) {
    if (this.closed) throw new Error("socket is closed");
    this.sent.push(String(text));
  }

  close() {
    if (this.closed) return;
    this.closed = true;
    this.emit("close", {});
  }

  emit(type, event) {
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }

  open() {
    this.emit("open", {});
  }

  message(frame) {
    this.emit("message", { data: JSON.stringify(frame) });
  }
}

const turn = () => new Promise((resolve) => setImmediate(resolve));

async function readyWorker(tool) {
  const socket = new FakeSocket();
  const worker = new AttachedWorker(
    "ws://brain.invalid/v1/sessions/ses/attached",
    "secret-token",
    new Map([[tool.executor.callbackId, tool]]),
    () => socket,
  );
  socket.open();
  assert.deepEqual(JSON.parse(socket.sent.shift()), {
    type: "hello",
    token: "secret-token",
    callbacks: [tool.executor.callbackId],
  });
  socket.message({ type: "ready" });
  await worker.ready;
  return { socket, worker };
}

test("attached worker validates, executes, and deduplicates one exact call id", async () => {
  let executions = 0;
  const tool = defineTool({
    name: "renamed_echo",
    description: "Echo text.",
    input: z.object({ text: z.string() }),
    output: z.object({ text: z.string() }),
    async execute(input) {
      executions += 1;
      return input;
    },
  }).local({ callbackId: "callback.echo" });
  const { socket, worker } = await readyWorker(tool);
  const call = {
    type: "call",
    call_id: "call-1",
    callback_id: "callback.echo",
    name: "renamed_echo",
    input: { text: "hello" },
  };
  socket.message(call);
  await turn();
  const first = socket.sent.at(-1);
  assert.deepEqual(JSON.parse(first), {
    type: "result",
    call_id: "call-1",
    ok: true,
    output: { text: "hello" },
  });
  socket.message(call);
  await turn();
  assert.equal(socket.sent.at(-1), first);
  assert.equal(executions, 1);
  worker.close();
});

test("abort reaches the callback and an oversized value becomes a bounded error", async () => {
  const abortable = defineTool({
    name: "wait",
    description: "Wait until cancelled.",
    input: z.object({}),
    output: z.string(),
    execute(_input, context) {
      return new Promise((resolve, reject) => {
        context.signal.addEventListener("abort", () => reject(new Error("observed abort")), {
          once: true,
        });
      });
    },
  }).local({ callbackId: "callback.wait" });
  const first = await readyWorker(abortable);
  first.socket.message({
    type: "call",
    call_id: "call-abort",
    callback_id: "callback.wait",
    name: "wait",
    input: {},
  });
  await turn();
  first.socket.message({ type: "abort", call_id: "call-abort" });
  await turn();
  const aborted = JSON.parse(first.socket.sent.at(-1));
  assert.equal(aborted.ok, false);
  assert.equal(aborted.error, "observed abort");
  first.worker.close();

  const oversized = defineTool({
    name: "large",
    description: "Return an oversized value.",
    input: z.object({}),
    output: z.string(),
    execute() {
      return "x".repeat(130 * 1024);
    },
  }).local({ callbackId: "callback.large" });
  const second = await readyWorker(oversized);
  second.socket.message({
    type: "call",
    call_id: "call-large",
    callback_id: "callback.large",
    name: "large",
    input: {},
  });
  await turn();
  const bounded = JSON.parse(second.socket.sent.at(-1));
  assert.equal(bounded.ok, false);
  assert.equal(bounded.error, "attached Tool result exceeds 128 KiB");
  assert.ok(second.socket.sent.at(-1).length < 1024);
  second.worker.close();
});
