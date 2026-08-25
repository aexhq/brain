import assert from "node:assert/strict";
import test from "node:test";

import { z } from "zod";

import { CustomerEnvironment, compileTools, customerTerminalDigest, tool } from "../dist/index.js";

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

  open() { this.emit("open", {}); }
  message(frame) { this.emit("message", { data: JSON.stringify(frame) }); }
}

const turn = () => new Promise((resolve) => setImmediate(resolve));

async function readyHand(value) {
  const registrations = (await compileTools([value])).clientRegistrations;
  const socket = new FakeSocket();
  let request;
  const observations = [];
  const environment = new CustomerEnvironment(
    async () => ({
      request: { url: "wss://environment.invalid", protocol: "aex-grant.short-lived" },
      async observe(observation) { observations.push(observation); },
    }),
    registrations,
    (value) => { request = value; return socket; },
    { clientId: "app.primary", processId: "process.one" },
  );
  await turn();
  socket.open();
  const registration = JSON.parse(socket.sent.shift());
  assert.equal(registration.type, "register");
  assert.equal(registration.client_id, "app.primary");
  assert.equal(registration.process_id, "process.one");
  assert.deepEqual(request, { url: "wss://environment.invalid", protocol: "aex-grant.short-lived" });
  socket.message({ type: "ready", epoch: 7 });
  await turn();
  const batch = JSON.parse(socket.sent.shift());
  assert.equal(batch.type, "register_tools");
  socket.message({ type: "registered", epoch: 7, batch_id: batch.batch_id });
  await environment.ready;
  return { socket, environment, registration: registrations[0], observations };
}

test("customer Environment receipts, validates, executes, and retains one exact operation", async () => {
  let executions = 0;
  const echo = tool(z.object({ text: z.string() }), async function echo(input) {
    executions += 1;
    return input;
  }).returns(z.object({ text: z.string() })).client({ registration: "echo.current" });
  const { socket, environment, registration, observations } = await readyHand(echo);
  const offer = {
    type: "offer",
    epoch: 7,
    operation_id: "op-1",
    request_digest: "a".repeat(64),
    session_id: "ses-1",
    registration: registration.registration,
    name: registration.name,
    contract_digest: registration.contractDigest,
    input: { text: "hello" },
    deadline_at_ms: Date.now() + 10_000,
  };
  socket.message(offer);
  await turn();
  assert.equal(observations.at(-2).type, "receipt");
  assert.deepEqual(observations.at(-1), {
    type: "terminal",
    epoch: 7,
    operation_id: "op-1",
    request_digest: "a".repeat(64),
    ok: true,
    output: { text: "hello" },
  });
  const retained = observations.at(-1);
  socket.message(offer);
  await turn();
  assert.deepEqual(observations.at(-1), retained);
  assert.equal(executions, 1);
  environment.close();
});

test("customer Environment cancellation reaches the exact running operation", async () => {
  const wait = tool(z.object({}), async function wait(_input, context) {
    return await new Promise((_resolve, reject) => {
      context.signal.addEventListener("abort", () => reject(new Error("observed abort")), { once: true });
    });
  }).client({ registration: "wait.current" });
  const { socket, environment, registration, observations } = await readyHand(wait);
  socket.message({
    type: "offer",
    epoch: 7,
    operation_id: "op-cancel",
    request_digest: "b".repeat(64),
    session_id: "ses-1",
    registration: registration.registration,
    name: registration.name,
    contract_digest: registration.contractDigest,
    input: {},
    deadline_at_ms: Date.now() + 10_000,
  });
  await turn();
  socket.message({ type: "cancel", epoch: 7, operation_id: "op-cancel", reason: "user cancelled" });
  await turn();
  const terminal = observations.at(-1);
  assert.equal(terminal.type, "terminal");
  assert.equal(terminal.ok, false);
  assert.equal(terminal.error, "observed abort");
  environment.close();
});

test("routine socket loss reconnects without cancelling or redispatching an assigned operation", async () => {
  let executions = 0;
  let finish;
  const value = tool(z.object({}), async function work(_input, context) {
    executions += 1;
    return await new Promise((resolve, reject) => {
      finish = resolve;
      context.signal.addEventListener("abort", () => reject(new Error("aborted")), { once: true });
    });
  }).client({ registration: "work.current" });
  const registration = (await compileTools([value])).clientRegistrations[0];
  const sockets = [];
  const observations = [];
  let grants = 0;
  const environment = new CustomerEnvironment(
    async () => ({
      request: { url: `wss://environment.invalid/${++grants}`, protocol: `grant.${grants}` },
      async observe(observation) { observations.push(observation); },
    }),
    [registration],
    () => {
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket;
    },
    { clientId: "app.primary", processId: "process.stable", reconnectDelayMs: 0 },
  );
  await turn();
  const first = sockets[0];
  first.open();
  const firstHello = JSON.parse(first.sent.shift());
  first.message({ type: "ready", epoch: 1 });
  await turn();
  const firstBatch = JSON.parse(first.sent.shift());
  first.message({ type: "registered", epoch: 1, batch_id: firstBatch.batch_id });
  await environment.ready;
  const offer = {
    type: "offer",
    epoch: 1,
    operation_id: "op-unknown",
    request_digest: "c".repeat(64),
    session_id: "ses-1",
    registration: registration.registration,
    name: registration.name,
    contract_digest: registration.contractDigest,
    input: {},
    deadline_at_ms: Date.now() + 10_000,
  };
  first.message(offer);
  await turn();
  assert.equal(observations.at(-1).type, "receipt");
  first.close();
  await new Promise((resolve) => setTimeout(resolve, 5));
  await turn();
  const second = sockets[1];
  second.open();
  const secondHello = JSON.parse(second.sent.shift());
  assert.equal(secondHello.process_id, firstHello.process_id);
  second.message({ type: "ready", epoch: 2 });
  await turn();
  const secondBatch = JSON.parse(second.sent.shift());
  second.message({ type: "registered", epoch: 2, batch_id: secondBatch.batch_id });
  await turn();
  second.message({ ...offer, epoch: 2 });
  await turn();
  assert.equal(executions, 1);
  assert.equal(observations.at(-1).type, "receipt");
  assert.equal(observations.at(-1).replayed, true);
  finish({ survived: true });
  await turn();
  assert.equal(observations.at(-1).type, "terminal");
  assert.deepEqual(observations.at(-1).output, { survived: true });
  second.message({ ...offer, epoch: 2 });
  await turn();
  assert.equal(observations.at(-1).epoch, 2);
  assert.equal(executions, 1);
  assert.equal(grants, 2);
  environment.close();
});

test("retention capacity applies backpressure and never evicts an unacknowledged terminal", async () => {
  let executions = 0;
  const value = tool(z.object({ value: z.number() }), async function bounded(input) {
    executions += 1;
    return input;
  }).client({ registration: "bounded.current" });
  const registration = (await compileTools([value])).clientRegistrations[0];
  const socket = new FakeSocket();
  const observations = [];
  const environment = new CustomerEnvironment(
    async () => ({
      request: { url: "wss://environment.invalid", protocol: "grant.capacity" },
      async observe(observation) { observations.push(observation); },
    }),
    [registration],
    () => socket,
    { clientId: "app.primary", maxRetainedOperations: 1 },
  );
  await turn();
  socket.open();
  socket.sent.shift();
  socket.message({ type: "ready", epoch: 1 });
  await turn();
  const batch = JSON.parse(socket.sent.shift());
  socket.message({ type: "registered", epoch: 1, batch_id: batch.batch_id });
  await environment.ready;
  const offer = (id, value) => ({
    type: "offer", epoch: 1, operation_id: id, request_digest: value.toString().repeat(64).slice(0, 64),
    session_id: "ses-1", registration: registration.registration, name: registration.name,
    contract_digest: registration.contractDigest, input: { value }, deadline_at_ms: Date.now() + 10_000,
  });
  socket.message(offer("op-first", 1));
  await turn();
  const firstTerminal = observations.at(-1);
  socket.message(offer("op-second", 2));
  await turn();
  assert.equal(executions, 1);
  assert.deepEqual(observations.at(-1), firstTerminal);
  socket.message({
    type: "ack",
    epoch: 1,
    operation_id: "op-first",
    request_digest: firstTerminal.request_digest,
    terminal_digest: "0".repeat(64),
  });
  socket.message(offer("op-second", 2));
  await turn();
  assert.equal(executions, 1, "a mismatched terminal acknowledgement released capacity");
  socket.message({
    type: "ack",
    epoch: 1,
    operation_id: "op-first",
    request_digest: firstTerminal.request_digest,
    terminal_digest: customerTerminalDigest(firstTerminal),
  });
  socket.message(offer("op-second", 2));
  await turn();
  assert.equal(executions, 2);
  environment.close();
});

test("inline result bound counts UTF-8 bytes", async () => {
  const value = tool(z.object({}), async function unicode() {
    return "界".repeat(45_000);
  }).client({ registration: "unicode.current" });
  const { socket, environment, registration, observations } = await readyHand(value);
  socket.message({
    type: "offer", epoch: 7, operation_id: "op-unicode", request_digest: "d".repeat(64),
    session_id: "ses-1", registration: registration.registration, name: registration.name,
    contract_digest: registration.contractDigest, input: {}, deadline_at_ms: Date.now() + 10_000,
  });
  await turn();
  assert.equal(observations.at(-1).ok, false);
  assert.match(observations.at(-1).error, /94208 inline bytes/);
  environment.close();
});

test("a running operation rejects a different digest without aliasing its receipt", async () => {
  let executions = 0;
  let finish;
  const value = tool(z.object({}), async function blocked() {
    executions += 1;
    return await new Promise((resolve) => { finish = resolve; });
  }).client({ registration: "blocked.digest" });
  const { socket, environment, registration, observations } = await readyHand(value);
  const offer = {
    type: "offer", epoch: 7, operation_id: "op-digest", request_digest: "a".repeat(64),
    session_id: "ses-1", registration: registration.registration, name: registration.name,
    contract_digest: registration.contractDigest, input: {}, deadline_at_ms: Date.now() + 10_000,
  };
  socket.message(offer);
  await turn();
  socket.message({ ...offer, request_digest: "b".repeat(64) });
  await turn();
  assert.equal(executions, 1);
  assert.equal(observations.at(-1).ok, false);
  assert.match(observations.at(-1).error, /different request_digest/);
  finish({ kept: "original" });
  await turn();
  assert.equal(observations.at(-1).ok, true);
  assert.equal(observations.at(-1).request_digest, "a".repeat(64));
  environment.close();
});

test("a failed receipt delivery removes pre-effect admission and retries safely", async () => {
  let executions = 0;
  let receiptFailures = 2;
  const value = tool(z.object({}), async function once() {
    executions += 1;
    return { executions };
  }).client({ registration: "receipt.retry" });
  const registration = (await compileTools([value])).clientRegistrations[0];
  const socket = new FakeSocket();
  const observations = [];
  const environment = new CustomerEnvironment(
    async () => ({
      request: { url: "wss://environment.invalid", protocol: "grant.receipt" },
      async observe(observation) {
        observations.push(observation);
        if (observation.type === "receipt" && receiptFailures-- > 0) throw new Error("lost receipt response");
      },
    }),
    [registration],
    () => socket,
    { clientId: "app.primary" },
  );
  await turn();
  socket.open(); socket.sent.shift(); socket.message({ type: "ready", epoch: 1 }); await turn();
  const batch = JSON.parse(socket.sent.shift());
  socket.message({ type: "registered", epoch: 1, batch_id: batch.batch_id });
  await environment.ready;
  const offer = {
    type: "offer", epoch: 1, operation_id: "op-receipt", request_digest: "c".repeat(64),
    session_id: "ses-1", registration: registration.registration, name: registration.name,
    contract_digest: registration.contractDigest, input: {}, deadline_at_ms: Date.now() + 10_000,
  };
  socket.message(offer);
  await turn();
  assert.equal(executions, 0);
  socket.message(offer);
  await turn();
  assert.equal(executions, 1);
  assert.equal(observations.at(-1).type, "terminal");
  environment.close();
});

test("an ambiguous success delivery retains one immutable terminal outcome", async () => {
  let executions = 0;
  let serializations = 0;
  let terminalFailures = 2;
  const value = tool(z.object({}), async function mutableResult() {
    executions += 1;
    return {
      toJSON() {
        serializations += 1;
        return { version: serializations };
      },
    };
  }).client({ registration: "terminal.immutable" });
  const registration = (await compileTools([value])).clientRegistrations[0];
  const socket = new FakeSocket();
  const observations = [];
  const environment = new CustomerEnvironment(
    async () => ({
      request: { url: "wss://environment.invalid", protocol: "grant.terminal" },
      async observe(observation) {
        observations.push(structuredClone(observation));
        if (observation.type === "terminal" && terminalFailures-- > 0) {
          throw new Error("terminal applied but response was lost");
        }
      },
    }),
    [registration],
    () => socket,
    { clientId: "app.primary" },
  );
  await turn();
  socket.open(); socket.sent.shift(); socket.message({ type: "ready", epoch: 1 }); await turn();
  const batch = JSON.parse(socket.sent.shift());
  socket.message({ type: "registered", epoch: 1, batch_id: batch.batch_id });
  await environment.ready;
  const offer = {
    type: "offer", epoch: 1, operation_id: "op-terminal", request_digest: "d".repeat(64),
    session_id: "ses-1", registration: registration.registration, name: registration.name,
    contract_digest: registration.contractDigest, input: {}, deadline_at_ms: Date.now() + 10_000,
  };
  socket.message(offer);
  await turn();
  socket.message(offer);
  await turn();
  const terminals = observations.filter((entry) => entry.type === "terminal");
  assert.equal(executions, 1);
  assert.equal(serializations, 1);
  assert.ok(terminals.length >= 3);
  assert.ok(terminals.every((entry) => entry.ok && entry.output.version === 1));
  environment.close();
});

test("same registration and contract never aliases a different closure", async () => {
  const first = tool(z.object({}), async function same() { return { source: "first" }; })
    .client({ registration: "same.contract" });
  const second = tool(z.object({}), async function same() { return { source: "second" }; })
    .client({ registration: "same.contract" });
  const [firstRegistration] = (await compileTools([first])).clientRegistrations;
  const [secondRegistration] = (await compileTools([second])).clientRegistrations;
  assert.equal(firstRegistration.contractDigest, secondRegistration.contractDigest);
  assert.throws(
    () => new CustomerEnvironment(
      async () => ({
        request: { url: "wss://environment.invalid", protocol: "grant.conflict" },
        async observe() {},
      }),
      [firstRegistration, secondRegistration],
      () => new FakeSocket(),
      { clientId: "app.primary" },
    ),
    /conflicts with its existing contract or handler/,
  );

  const socket = new FakeSocket();
  const environment = new CustomerEnvironment(
    async () => ({
      request: { url: "wss://environment.invalid", protocol: "grant.conflict" },
      async observe() {},
    }),
    [firstRegistration],
    () => socket,
    { clientId: "app.primary" },
  );
  await assert.rejects(
    environment.register([secondRegistration]),
    /conflicts with its existing contract or handler/,
  );
  environment.close();
});

test("a missing heartbeat echo reconnects and re-registers after server state loss", async () => {
  const sockets = [];
  let grants = 0;
  const environment = new CustomerEnvironment(
    async () => ({
      request: { url: "wss://environment.invalid", protocol: `grant.heartbeat.${++grants}` },
      async observe() {},
    }),
    [],
    () => {
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket;
    },
    {
      clientId: "app.primary",
      reconnectDelayMs: 0,
      maxReconnectDelayMs: 0,
      heartbeatIntervalMs: 5,
      heartbeatTimeoutMs: 10,
    },
  );
  await turn();
  const first = sockets[0];
  first.open();
  first.sent.shift();
  first.message({ type: "ready", epoch: 1 });
  await environment.ready;
  for (let attempt = 0; attempt < 100 && first.sent.length === 0; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  const heartbeat = JSON.parse(first.sent.shift());
  assert.equal(heartbeat.type, "heartbeat");
  // A restarted coordinator cannot echo the old nonce. The runner must not remain falsely ready
  // until API Gateway's two-hour connection cap.
  for (let attempt = 0; attempt < 100 && sockets.length < 2; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  assert.equal(first.closed, true);
  assert.equal(sockets.length, 2);
  assert.equal(grants, 2);
  environment.close();
});

test("a connection-level error after ready closes and reconnects", async () => {
  const sockets = [];
  const environment = new CustomerEnvironment(
    async () => ({
      request: { url: "wss://environment.invalid", protocol: `grant.error.${sockets.length}` },
      async observe() {},
    }),
    [],
    () => {
      const socket = new FakeSocket();
      sockets.push(socket);
      return socket;
    },
    { clientId: "app.primary", reconnectDelayMs: 0, maxReconnectDelayMs: 0, heartbeatIntervalMs: 0 },
  );
  await turn();
  const first = sockets[0];
  first.open();
  first.sent.shift();
  first.message({ type: "ready", epoch: 1 });
  await environment.ready;
  first.message({ type: "error", code: "coordinator_state_lost", message: "register again" });
  for (let attempt = 0; attempt < 100 && sockets.length < 2; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  assert.equal(first.closed, true);
  assert.equal(sockets.length, 2);
  environment.close();
});

test("close interrupts a pending connector without leaving a reconnect timer", async () => {
  let finishConnector;
  const environment = new CustomerEnvironment(
    () => new Promise((resolve) => { finishConnector = resolve; }),
    [],
    () => new FakeSocket(),
    { clientId: "app.primary", reconnectDelayMs: 30_000 },
  );
  await turn();
  environment.close();
  await assert.rejects(environment.ready, /Customer Environment is closed/);
  finishConnector({
    request: { url: "wss://environment.invalid", protocol: "grant.too-late" },
    async observe() {},
  });
});
