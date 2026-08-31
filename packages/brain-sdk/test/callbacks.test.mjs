import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

import { z } from "zod";
import { appTools, createEnvironmentHandler, environment, installExtensionIdentity } from "../dist/index.js";

const channelToken = "sesame";

function makeChannelEnvironment(name) {
  const definition = environment({ options: z.object({ channelToken: z.string() }) }, (author) => {
    const instance = author.open(async () => ({}));
    instance.run(async () => "provisioned ran");
    instance.close(async () => undefined);
    author.route.callbacks();
    return {};
  });
  installExtensionIdentity(definition, name, undefined, name);
  return definition;
}

const command = (id, request, attachmentId) => ({
  contract: "environment/v2",
  binding: {},
  operation: {
    operation_id: id,
    request_identity: id.padEnd(64, "a"),
    environment_id: "env_1",
    session_id: "ses_test",
    ...(attachmentId === undefined ? {} : { attachment_id: attachmentId }),
    request,
  },
});

async function openEnvironment(handle, configuration) {
  assert.equal((await handle(command("setup", { type: "setup", configuration }))).receipt.type, "accepted");
  const attached = await handle(command("attach", { type: "attach", grants: {}, provisions: [], bindings: {} }, "att_1"));
  assert.equal(attached.receipt.type, "accepted");
  assert.deepEqual(attached.receipt.provides, []);
}

function listen(handle) {
  const server = createServer((request, response) => {
    response.statusCode = 426;
    response.end();
  });
  const sockets = new Set();
  server.on("connection", (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  });
  server.on("upgrade", (request, socket, head) => handle.channel.upgrade(request, socket, head));
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve({ server, sockets, port: server.address().port })));
}

const outcomeOf = async (handle, id, tool, input, deadlineMs) => {
  const response = await handle(command(id, { type: "invoke", call_id: id, tool, input, deadline_ms: deadlineMs }, "att_1"));
  assert.equal(response.receipt.type, "outcome");
  return response.receipt.outcome;
};

const until = async (probe) => {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    const value = await probe();
    if (value !== undefined) return value;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  assert.fail("condition never held");
};

test("routes invocations down the app's channel and back", async () => {
  const handle = createEnvironmentHandler(makeChannelEnvironment("chanRoundtrip"));
  const { server, port } = await listen(handle);
  await openEnvironment(handle, { driver: "chanRoundtrip", channelToken });
  let cancelSeen = false;
  let hangStarted = false;
  const app = appTools.connect({ url: `ws://127.0.0.1:${port}/environments/env_1/channel`, token: channelToken })
    .register({ name: "highlight_row", description: "Highlight a row.", input: z.object({ row: z.number().int() }) }, ({ row }) => ({ highlighted: row }))
    .register({ name: "hang", description: "Wait until cancelled.", input: z.object({}) }, (_input, call) => {
      hangStarted = true;
      return new Promise((resolve) => call.signal.addEventListener("abort", () => { cancelSeen = true; resolve(null); }, { once: true }));
    });
  try {
    await app.ready();

    // Invoke → result.
    assert.deepEqual(await outcomeOf(handle, "inv_ok", "highlight_row", { row: 7 }, 5_000), { status: "ok", value: { highlighted: 7 } });

    // Schema violations stay in-band.
    const invalid = await outcomeOf(handle, "inv_bad", "highlight_row", { row: "seven" }, 5_000);
    assert.equal(invalid.status, "error");
    assert.equal(invalid.error.code, "invalid_input");

    // Deadline → timeout outcome, and the cancel reaches the app.
    assert.deepEqual(await outcomeOf(handle, "inv_slow", "hang", {}, 200), { status: "timeout" });
    await until(() => (cancelSeen ? true : undefined));

    // Brain-side cancel → cancelled outcome.
    cancelSeen = false;
    hangStarted = false;
    const pending = outcomeOf(handle, "inv_cancel", "hang", {}, 60_000);
    await until(() => (hangStarted ? true : undefined));
    await handle(command("cancel_1", { type: "cancel", target_operation_id: "inv_cancel" }, "att_1"));
    assert.deepEqual(await pending, { status: "cancelled" });
    await until(() => (cancelSeen ? true : undefined));
  } finally {
    app.close();
    server.close();
  }
});

test("fails fast while the app is disconnected and recovers when it reconnects", async () => {
  const handle = createEnvironmentHandler(makeChannelEnvironment("chanReconnect"));
  const { server, sockets, port } = await listen(handle);
  await openEnvironment(handle, { driver: "chanReconnect", channelToken });

  // No app yet: a typed error, not a hang.
  const unrouted = await outcomeOf(handle, "inv_none", "echo", {}, 1_000);
  assert.equal(unrouted.status, "error");
  assert.equal(unrouted.error.code, "app_disconnected");

  const app = appTools.connect({ url: `ws://127.0.0.1:${port}/environments/env_1/channel`, token: channelToken })
    .register({ name: "echo", description: "Echo.", input: z.object({}) }, () => "here");
  try {
    await app.ready();
    assert.deepEqual(await outcomeOf(handle, "inv_first", "echo", {}, 5_000), { status: "ok", value: "here" });

    // Drop the transport out from under the app; it reconnects with backoff.
    let sequence = 0;
    for (const socket of sockets) socket.destroy();
    const recovered = await until(async () => {
      const outcome = await outcomeOf(handle, `inv_retry_${sequence += 1}`, "echo", {}, 1_000);
      return outcome.status === "ok" ? outcome : undefined;
    });
    assert.deepEqual(recovered, { status: "ok", value: "here" });

    // A deliberate close is terminal: calls fail app-disconnected again.
    app.close();
    const closed = await until(async () => {
      const outcome = await outcomeOf(handle, `inv_gone_${sequence += 1}`, "echo", {}, 1_000);
      return outcome.status === "error" ? outcome : undefined;
    });
    assert.equal(closed.error.code, "app_disconnected");
  } finally {
    app.close();
    server.close();
  }
});

test("refuses a channel upgrade with a bad token", async () => {
  const handle = createEnvironmentHandler(makeChannelEnvironment("chanAuth"));
  const { server, port } = await listen(handle);
  await openEnvironment(handle, { driver: "chanAuth", channelToken });
  try {
    const socket = new WebSocket(`ws://127.0.0.1:${port}/environments/env_1/channel?token=wrong`);
    const outcome = await new Promise((resolve) => {
      socket.addEventListener("open", () => resolve("open"));
      socket.addEventListener("error", () => resolve("refused"));
    });
    assert.equal(outcome, "refused");
  } finally {
    server.close();
  }
});

test("routes invocations to a backend app over signed POSTs", async () => {
  const signingKey = "post-secret";
  const app = appTools({ signingKey })
    .register({ name: "create_invoice", description: "Create an invoice.", input: z.object({ customer_id: z.string() }) }, ({ customer_id }) => ({ invoice_id: `inv_${customer_id}` }))
    .register({ name: "hang", description: "Wait forever.", input: z.object({}) }, () => new Promise(() => {}));
  const fetchHandler = app.fetchHandler();
  const appServer = createServer(async (request, response) => {
    let body = "";
    for await (const chunk of request) body += chunk;
    const answered = await fetchHandler(new Request(`http://app.local${request.url}`, { method: request.method, headers: { "x-brain-signature": request.headers["x-brain-signature"] ?? "" }, body }));
    response.statusCode = answered.status;
    response.setHeader("content-type", "application/json");
    response.end(await answered.text());
  });
  const port = await new Promise((resolve) => appServer.listen(0, "127.0.0.1", () => resolve(appServer.address().port)));

  const definition = environment({ options: z.object({ url: z.string(), key: z.string() }) }, (author) => {
    const instance = author.open(async () => ({}));
    instance.run(async () => undefined);
    instance.close(async () => undefined);
    author.route.callbacks(({ options }) => ({ mode: "post", url: options.url, signingKey: options.key }));
    return {};
  });
  installExtensionIdentity(definition, "postEnv", undefined, "postEnv");
  const handle = createEnvironmentHandler(definition);
  await openEnvironment(handle, { driver: "postEnv", url: `http://127.0.0.1:${port}/tools`, key: signingKey });
  try {
    assert.deepEqual(await outcomeOf(handle, "inv_ok", "create_invoice", { customer_id: "c1" }, 5_000), { status: "ok", value: { invoice_id: "inv_c1" } });
    assert.deepEqual(await outcomeOf(handle, "inv_slow", "hang", {}, 200), { status: "timeout" });
  } finally {
    appServer.close();
  }

  // An unreachable app is a typed error, not a hang.
  const unreachable = environment({ options: z.object({ url: z.string(), key: z.string() }) }, (author) => {
    const instance = author.open(async () => ({}));
    instance.run(async () => undefined);
    instance.close(async () => undefined);
    author.route.callbacks(({ options }) => ({ mode: "post", url: options.url, signingKey: options.key }));
    return {};
  });
  installExtensionIdentity(unreachable, "postGone", undefined, "postGone");
  const gone = createEnvironmentHandler(unreachable);
  await openEnvironment(gone, { driver: "postGone", url: `http://127.0.0.1:${port}/tools`, key: signingKey });
  const outcome = await outcomeOf(gone, "inv_gone", "create_invoice", { customer_id: "c2" }, 2_000);
  assert.equal(outcome.status, "error");
  assert.equal(outcome.error.code, "app_unreachable");
});

test("keeps provisioned invocations on the environment's own run handler", async () => {
  const handle = createEnvironmentHandler(makeChannelEnvironment("chanMixed"));
  assert.equal((await handle(command("setup", { type: "setup", configuration: { driver: "chanMixed", channelToken } }))).receipt.type, "accepted");
  const provision = { manifest: { name: "local_tool", description: "Provisioned here.", input_schema: {}, requires: [], binding_names: [], hosting: "provisioned", payload: { kind: "esm", identity: "c".repeat(64) } }, payload_identity: "c".repeat(64) };
  assert.equal((await handle(command("attach", { type: "attach", grants: {}, provisions: [provision], bindings: {} }, "att_1"))).receipt.type, "accepted");
  const provisioned = await outcomeOf(handle, "inv_local", "local_tool", {}, 1_000);
  assert.deepEqual(provisioned, { status: "ok", value: "provisioned ran" });
  const routed = await outcomeOf(handle, "inv_remote", "remote_tool", {}, 1_000);
  assert.equal(routed.error.code, "app_disconnected");
});
