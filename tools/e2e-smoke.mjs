// usage: e2e-smoke.mjs [<release manifest.json> [<smoke evidence path>]]
//
// Drives the composition the product ships end to end through an installed @aexhq/brain archive
// and the binaries this tree builds. With a release manifest it installs exactly those registry
// versions and records the evidence promotion requires; without one it installs what this tree
// would publish. Nothing here reaches a model provider, a cloud service or any host but loopback.

import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { createServer as createSocketServer } from "node:net";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

const root = path.resolve(import.meta.dirname, "..");
const npmCli = [
  process.env.npm_execpath,
  path.join(path.dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js"),
  path.resolve(path.dirname(process.execPath), "../lib/node_modules/npm/bin/npm-cli.js"),
].find((candidate) => candidate !== undefined && existsSync(candidate));
if (npmCli === undefined) throw new Error("could not locate npm-cli.js for the active Node runtime");

// The one hosted engine capability this gate proves Brain admits. Deployment policy still seals
// the identifier: a request schema that accepts composition-owned names must not widen it.
const HOSTED_CAPABILITY = "aex.output";
const UNSEALED_CAPABILITY = "aex.smoke.unsealed";
const ANSWER = "brain-e2e-smoke-answer";
const ENVIRONMENT_ANSWER = "brain-e2e-smoke-environment-answer";
const REFUSAL = "the relay refused this release";
const OPERATOR_TOKEN = "brain-e2e-smoke-operator";
const EXECUTOR_TOKEN = "brain-e2e-smoke-executor";
const DISPATCH_TOKEN = "brain-e2e-smoke-dispatch";
// The immutable ESM the bound Environment executes. Its byte count reaches the Environment only if
// the sealed bundle travelled by content-addressed reference and its layer resolved.
const BUNDLE = new TextEncoder().encode(
  "export default async function invoke(input) { return input; }\n",
);

const workspace = mkdtempSync(path.join(os.tmpdir(), "brain-e2e-smoke-"));
const consumer = path.join(workspace, "consumer");

const run = (args, cwd = root, stdio = "pipe") =>
  execFileSync(process.execPath, [npmCli, ...args], { cwd, encoding: "utf8", stdio }).trim();

const binary = (name) => {
  const file = process.platform === "win32" ? `${name}.exe` : name;
  const found = ["debug", "release"]
    .map((profile) => path.join(root, "target", profile, file))
    .find((candidate) => existsSync(candidate));
  if (found === undefined) {
    throw new Error(
      `build ${name} first: cargo build -p brain-server --bin brain --bin brain-component-host`,
    );
  }
  return found;
};

const component = (kind) => {
  const file = path.join(root, "crates/brain-component-host/guest/dist", `${kind}.component.wasm`);
  if (!existsSync(file)) throw new Error("run npm run build:components before the e2e smoke");
  return pathToFileURL(file);
};

/** Exactly the versions the registry serves, or the archives this working tree would publish. */
const resolveRelease = (manifestPath) => {
  if (manifestPath === undefined) {
    const packed = JSON.parse(
      run(["pack", "--json", "--workspace", "@aexhq/brain", "--pack-destination", workspace]),
    );
    assert.equal(packed.length, 1, "npm pack returned an unexpected result for @aexhq/brain");
    const [item] = packed;
    return [{
      name: item.name,
      version: item.version,
      integrity: item.integrity,
      spec: `file:${path.join(workspace, item.filename)}`,
    }];
  }
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  assert.equal(manifest.schema, 1, "release manifest has an unsupported shape");
  return manifest.packages.map(({ name, version, integrity }) => {
    const published = JSON.parse(run(["view", `${name}@${version}`, "dist.integrity", "--json"]));
    assert.equal(
      published,
      integrity,
      `${name}@${version} on the registry is not the archive this release staged`,
    );
    return { name, version, integrity, spec: `${name}@${version}` };
  });
};

/** Import the installed package the way an empty consumer would, through its own resolution. */
const installRelease = async (release) => {
  mkdirSync(consumer, { recursive: true });
  writeFileSync(
    path.join(consumer, "package.json"),
    `${JSON.stringify({ private: true, type: "module" }, null, 2)}\n`,
  );
  run(["install", "--no-audit", "--no-fund", ...release.map(({ spec }) => spec)], consumer);
  const entry = path.join(consumer, "entry.mjs");
  writeFileSync(
    entry,
    `export * as brain from "@aexhq/brain";\n` +
      `export { officialTool } from "@aexhq/brain/internal";\n` +
      `export { z } from "zod";\n`,
  );
  return await import(pathToFileURL(entry));
};

const freePort = async () =>
  await new Promise((resolve, reject) => {
    const probe = createSocketServer();
    probe.on("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const { port } = probe.address();
      probe.close(() => resolve(port));
    });
  });

const startEndpoint = async (token, calls, respond) => {
  const server = createServer((request, response) => {
    let body = "";
    request.on("data", (chunk) => { body += chunk; });
    request.on("end", () => {
      const call = JSON.parse(body);
      calls.push({
        authorized: request.headers.authorization === `Bearer ${token}`,
        ...call,
      });
      const [status, payload] = respond(call);
      response.writeHead(status, { "content-type": "application/json" });
      response.end(JSON.stringify(payload));
    });
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { server, url: `http://127.0.0.1:${server.address().port}/` };
};

/**
 * The composition-owned same-host sidecars. Hosted Brain reaches Control's engine capabilities and
 * the Environment driver exactly this way, so neither needs a model, cloud or outbound dependency.
 */
const startSidecars = async (capabilityCalls, dispatchCalls) => ({
  capability: await startEndpoint(EXECUTOR_TOKEN, capabilityCalls, (call) => [200, {
    outcome: "completed",
    content: JSON.stringify(call.input),
    is_error: false,
    disposition: "continue",
    result: call.input,
  }]),
  // A release the Environment declares permanently impossible: the refusal shape that kept a live
  // plane's sessions ending forever, and whose reason three layers used to destroy.
  dispatch: await startEndpoint(DISPATCH_TOKEN, dispatchCalls, (call) =>
    call.action === "release" ? [400, { error: REFUSAL }] : [200, { dispatched: "ok" }]),
});

const startBrain = async (capabilityUrl, dispatchUrl) => {
  const port = await freePort();
  const server = spawn(binary("brain"), {
    cwd: root,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env,
      BRAIN_MODE: "local",
      BRAIN_DATA_DIR: path.join(workspace, "data"),
      BRAIN_LISTEN: `127.0.0.1:${port}`,
      BRAIN_API_TOKEN: OPERATOR_TOKEN,
      BRAIN_COMPONENT_HOST_BIN: binary("brain-component-host"),
      // One worker per host: a single sequential session never needs two, and a second worker
      // would compile and instantiate the same component again on a small CI runner.
      BRAIN_COMPONENT_WORKERS: "1",
      // The deadline exists for a network provider. Here the model is a Wasm component whose
      // first activation carries engine work, which alone exceeds the 30-second default on a
      // two-vCPU runner; the job timeout is what bounds a genuinely wedged turn.
      BRAIN_PROVIDER_HEADER_TIMEOUT_MS: "300000",
      BRAIN_EXTERNAL_TOOL_EXECUTOR_URL: capabilityUrl,
      BRAIN_EXTERNAL_TOOL_EXECUTOR_TOKEN: EXECUTOR_TOKEN,
      BRAIN_EXTERNAL_TOOL_POLICIES_JSON: JSON.stringify([{
        capability: HOSTED_CAPABILITY,
        scope: "root",
        completion: "continue",
        effect: "opaque",
        max_input_bytes: 65_536,
      }]),
      BRAIN_ENVIRONMENT_DISPATCH_URL: dispatchUrl,
      BRAIN_ENVIRONMENT_DISPATCH_TOKEN: DISPATCH_TOKEN,
    },
  });
  let log = "";
  const record = (chunk) => { log = `${log}${chunk}`.slice(-32_768); };
  server.stdout.on("data", record);
  server.stderr.on("data", record);
  let exited;
  server.on("exit", (code, signal) => { exited = `brain exited with ${code ?? signal}`; });

  const base = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + 120_000;
  for (;;) {
    if (exited !== undefined) throw new Error(`${exited}\n${log}`);
    const ready = await fetch(`${base}/v1/sessions`, {
      headers: { authorization: `Bearer ${OPERATOR_TOKEN}` },
    }).then((response) => response.ok, () => false);
    if (ready) return { server, base, log: () => log };
    assert.ok(Date.now() < deadline, `brain did not accept requests in time\n${log}`);
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
};

const kernel = ({ brain }, modelConfig) => ({
  model: {
    component: brain.component("model", component("model"), modelConfig, {
      metadata: { name: "smoke" },
    }),
    name: "smoke-model",
    apiKey: "sk-brain-e2e-smoke",
  },
  agentloop: brain.component("agentloop", component("agentloop"), { fixture: "sequential" }),
});

/** Model, Agentloop and one Brain-owned engine capability, the shape a hosted create carries. */
const hostedComposition = (installed, capability, modelConfig) => ({
  ...kernel(installed, modelConfig),
  tools: [installed.officialTool({
    name: "output",
    description: "Submit the structured answer.",
    input: installed.z.object({ text: installed.z.string() }),
    output: installed.z.object({ text: installed.z.string() }),
    capability,
  })],
});

/** All four component worlds: the Tool runs its sealed bundle inside the bound Environment. */
const shippedComposition = (installed) => {
  const definition = {
    name: "environment_echo",
    description: "Run the sealed bundle in the bound Environment.",
    input_schema: { type: "object" },
    output_schema: { type: "object", required: ["providerOperationId", "value"] },
  };
  return {
    ...kernel(installed, {
      toolName: definition.name,
      toolInput: { message: "run the bundle" },
      finalText: ENVIRONMENT_ANSWER,
    }),
    tools: [installed.brain.component(
      "tool",
      component("tool"),
      {
        definition: {
          ...definition,
          contract_digest: createHash("sha256").update(JSON.stringify(definition)).digest("hex"),
        },
        useEnvironment: true,
      },
      { grants: ["environment"], bundle: BUNDLE },
    )],
    environments: {
      workspace: installed.brain.component("environment", component("environment"), {
        dispatch: true,
      }),
    },
  };
};

const replay = async (session) => {
  const events = [];
  for await (const event of session.events({ after: 0, follow: false })) events.push(event);
  return events;
};

const settle = async (session, state, log) => {
  const deadline = Date.now() + 60_000;
  for (;;) {
    if ((await session.refresh()).state === state) return;
    assert.ok(Date.now() < deadline, `the session never reached ${state}\n${log()}`);
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
};

let sidecars;
let brainServer;
try {
  const release = resolveRelease(process.argv[2]);
  const installed = await installRelease(release);
  const { brain } = installed;

  const capabilityCalls = [];
  const dispatchCalls = [];
  sidecars = await startSidecars(capabilityCalls, dispatchCalls);
  brainServer = await startBrain(sidecars.capability.url, sidecars.dispatch.url);
  const client = new brain.Brain({ token: OPERATOR_TOKEN, baseUrl: brainServer.base });
  try {
    const hosted = await client.sessions.create(hostedComposition(installed, HOSTED_CAPABILITY, {
      toolName: "output",
      toolInput: { text: ANSWER },
      finalText: ANSWER,
    }));
    assert.equal(await hosted.send("submit the answer"), ANSWER);
    assert.deepEqual(capabilityCalls.map(({ authorized, name, input, context }) => ({
      authorized,
      capability: context["brain.capability"],
      name,
      input,
    })), [{ authorized: true, capability: HOSTED_CAPABILITY, name: "output", input: { text: ANSWER } }]);
    const hostedEvents = await replay(hosted);
    const hostedResult = hostedEvents.find((event) => event.type === "tool.result");
    assert.equal(hostedResult?.name, "output");
    assert.equal(hostedResult?.outcome, "completed");
    const hostedCompleted = hostedEvents.find((event) => event.type === "turn.completed");
    assert.equal(hostedCompleted?.stop_reason, "end_turn");
    assert.equal(hostedCompleted?.tool_calls, 1);

    // Admitting composition-owned capability names must leave sealed deployment policy the only
    // authority: an unsealed name is a structured refusal, never the raw HTTP 422 that a
    // request-schema rejection produces before Brain can compare it with policy.
    const refused = await client.sessions
      .create(hostedComposition(installed, UNSEALED_CAPABILITY, { finalText: ANSWER }))
      .then(() => undefined, (error) => error);
    assert.ok(refused instanceof brain.BrainError, "an unsealed capability was admitted");
    assert.equal(refused.status, 400);
    assert.equal(refused.code, "invalid_request");

    // The composition the product actually ships. Only the legacy Environment declaration was
    // driven end to end before, so route, world-identity and release handling drift reached a
    // deployment instead of this gate.
    const shipped = await client.sessions.create(shippedComposition(installed));
    assert.equal(await shipped.send("run the environment Tool"), ENVIRONMENT_ANSWER);
    const shippedEvents = await replay(shipped);
    const shippedResult = shippedEvents.find((event) => event.type === "tool.result");
    assert.equal(shippedResult?.name, "environment_echo");
    assert.equal(shippedResult?.outcome, "completed");
    const value = JSON.parse(shippedResult.output_preview);
    assert.equal(value.value, "environment-ok");
    assert.match(value.providerOperationId, new RegExp(`^ok:.+:${BUNDLE.byteLength}$`));
    assert.equal(
      shippedEvents.find((event) => event.type === "turn.completed")?.stop_reason,
      "end_turn",
    );
    const submits = dispatchCalls.filter((call) => call.action === "submit");
    assert.equal(submits.length, 1);
    assert.equal(submits[0].authorized, true);

    // The resource route the SDK addresses a bound Environment by. Drift here is a 404 that only
    // a client meets, and every in-repo test builds its own request instead. A component
    // Environment is bound by its Tool rather than materialized, which is the state it reports.
    assert.equal((await shipped.environment("workspace").status()).state, "never_materialized");

    // A release the Environment declares permanent cannot be cleared by repeating it, and an end
    // that waits on one is never retired. The end must finish, the release must be attempted, and
    // the endpoint's own reason must survive every layer that used to drop it.
    await shipped.end();
    await settle(shipped, "ended", brainServer.log);
    const releases = dispatchCalls.filter((call) => call.action === "release");
    assert.equal(releases.length, 1, "a refused release must be recorded once, never repeated");
    assert.equal(releases[0].authorized, true);
    assert.equal(releases[0].request.released, true);
    assert.match(brainServer.log(), new RegExp(REFUSAL));
  } finally {
    client.close();
  }

  const evidence = process.argv[3];
  if (evidence !== undefined) {
    mkdirSync(path.dirname(path.resolve(evidence)), { recursive: true });
    writeFileSync(
      path.resolve(evidence),
      `${JSON.stringify({
        schema: 1,
        source: process.env.EXPECTED_COMMIT ?? process.env.GITHUB_SHA ?? "local",
        packages: release.map(({ name, version, integrity }) => ({ name, version, integrity })),
      }, null, 2)}\n`,
    );
  }
  process.stdout.write(
    `Brain e2e smoke passed against ${release.map(({ spec }) => spec).join(", ")}\n`,
  );
} catch (error) {
  if (brainServer !== undefined) process.stderr.write(`${brainServer.log()}\n`);
  throw error;
} finally {
  brainServer?.server.kill();
  sidecars?.capability.server.close();
  sidecars?.dispatch.server.close();
  rmSync(workspace, { recursive: true, force: true });
}
