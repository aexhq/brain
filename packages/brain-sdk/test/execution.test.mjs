import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { z } from "zod";
import { createEnvironmentHandler, environment, substituteScript } from "../dist/index.js";
import { build } from "../dist/build.js";

const identityOf = (payload) => createHash("sha256").update(payload).digest("hex");
const esmArtifact = (name, payload, { needs = [], binding_names = [] } = {}) => ({
  manifest: { name, description: "test tool", input_schema: { type: "object" }, needs, binding_names, program: { kind: "esm", identity: identityOf(payload) } },
  payload,
});
const shellManifest = (name, script, { needs = ["process"], binding_names = [] } = {}) => ({ name, description: "test tool", input_schema: { type: "object" }, needs, binding_names, program: { kind: "shell", identity: identityOf(script), script } });
const httpManifest = (name, request, { needs = [], binding_names = [] } = {}) => ({ name, description: "test tool", input_schema: { type: "object" }, needs, binding_names, program: { kind: "http", identity: identityOf(JSON.stringify(request)), request } });
const provisionOf = (manifest) => ({ manifest, payload_identity: manifest.program.identity });
const toolPayload = (members) => `export default { kind: "brain.provisioned-tool/v1", ${members} };`;

let sequence = 0;
const command = (request, attachment_id) => {
  sequence += 1;
  return {
    contract: "environment/v1",
    binding: {},
    operation: { sequence, environment_id: "env_1", session_id: "ses_test", ...(attachment_id === undefined ? {} : { attachment_id }), request },
  };
};
const attachRequest = (provisions, bindings = { API_BASE: "https://api.internal" }) => ({ type: "attach", provisions, bindings });
const invokeRequest = (tool, input, deadline_ms = 5_000) => ({ type: "invoke", call_id: `call_${sequence}`, tool, input, deadline_ms });

const RESOURCES = { fs: { root: "/workspace" }, process: { timeout_ms_max: 1_000 } };

/** An environment that launches all three program kinds: esm in-process, shell
 * through a scripted executor that reports what it received, http through an
 * executor that echoes the request. */
function hostingEnvironment(artifacts, { shell, http } = {}) {
  const definition = environment({ resources: RESOURCES }, (author) => {
    const box = author.open(async () => ({ name: "box" }));
    box.execute.esm({ artifacts });
    box.execute.shell(shell ?? (async (context, script) => ({ exit_code: 0, stdout: script, stderr: `${context.instance.name}:${context.callId}:${context.bindings.API_BASE ?? ""}` })));
    box.execute.http(http ?? (async (context, request) => ({ echoed: request, session: context.sessionId })));
    box.close(async () => undefined);
    return {};
  });
  return createEnvironmentHandler(definition);
}

test("substituteScript replaces input references and leaves the shell's own alone", () => {
  assert.equal(substituteScript("npm test -- $filter", { filter: "unit" }), "npm test -- unit");
  assert.equal(substituteScript("echo ${greeting} $HOME", { greeting: "hi" }), "echo hi $HOME");
  assert.equal(substituteScript("run $count $flag $missing", { count: 3, flag: null }), "run 3  $missing");
  assert.equal(substituteScript("send $body", { body: { a: 1 } }), 'send {"a":1}');
  assert.throws(() => substituteScript("$x", "not an object"), (error) => error.code === "invalid_input");
});

test("setup and attach receipts declare what the environment executes and offers", async () => {
  const handle = hostingEnvironment([]);
  const setup = await handle(command({ type: "setup", configuration: {} }));
  assert.equal(setup.receipt.type, "accepted");
  assert.deepEqual(setup.receipt.runtimes, ["esm", "shell", "http"]);
  assert.deepEqual(setup.receipt.resources, RESOURCES);
  const attached = await handle(command(attachRequest([]), "att_1"));
  assert.equal(attached.receipt.type, "accepted");
  assert.deepEqual(attached.receipt.runtimes, ["esm", "shell", "http"]);
  assert.deepEqual(attached.receipt.resources, RESOURCES);
});

test("declared resources must be resource names with object policy blocks", () => {
  assert.throws(() => environment({ resources: { Fs: {} } }, () => ({})), /not a resource name/u);
  assert.throws(() => environment({ resources: { fs: "root" } }, () => ({})), /must declare an object/u);
  assert.doesNotThrow(() => environment({ resources: { fs: { root: "/w" }, "bin:ffmpeg": {} } }, (author) => { const box = author.open(async () => ({})); box.close(async () => undefined); return {}; }));
});

test("hosts an esm program with injected bindings and no resource handles", async () => {
  const payload = toolPayload(`
    parseInput(input) { if (typeof input.text !== "string") throw new Error("text must be a string"); return input; },
    async run(input, context) {
      return { text: input.text, base: context.bindings.API_BASE, callId: context.callId, keys: Object.keys(context).sort() };
    }`);
  const artifact = esmArtifact("echo", payload, { needs: ["process"], binding_names: ["API_BASE"] });
  const handle = hostingEnvironment([artifact]);
  await handle(command({ type: "setup", configuration: {} }));
  const attached = await handle(command(attachRequest([provisionOf(artifact.manifest)]), "att_1"));
  assert.equal(attached.receipt.type, "accepted", JSON.stringify(attached.receipt));
  const invoked = await handle(command(invokeRequest("echo", { text: "run it" }), "att_1"));
  assert.equal(invoked.receipt.type, "outcome");
  assert.equal(invoked.receipt.outcome.status, "ok");
  assert.equal(invoked.receipt.outcome.value.text, "run it");
  assert.equal(invoked.receipt.outcome.value.base, "https://api.internal");
  assert.deepEqual(invoked.receipt.outcome.value.keys, ["bindings", "callId", "deadline", "progress", "requestId", "signal"], "the context is plumbing only; resources are reached through the platform");

  const invalid = await handle(command(invokeRequest("echo", { text: 7 }), "att_1"));
  assert.equal(invalid.receipt.outcome.status, "error");
  assert.equal(invalid.receipt.outcome.error.code, "invalid_input");
});

test("runs a shell program through the environment's executor with its input substituted", async () => {
  const bash = shellManifest("bash", "$command", { binding_names: ["API_BASE"] });
  const handle = hostingEnvironment([]);
  await handle(command({ type: "setup", configuration: {} }));
  assert.equal((await handle(command(attachRequest([provisionOf(bash)]), "att_1"))).receipt.type, "accepted");
  const invoked = await handle(command(invokeRequest("bash", { command: "echo hi" }), "att_1"));
  assert.deepEqual(invoked.receipt.outcome, { status: "ok", value: { exit_code: 0, stdout: "echo hi", stderr: `box:call_${sequence - 1}:https://api.internal` } });
});

test("runs an http program through the environment's executor with the input as body", async () => {
  const ping = httpManifest("ping", { method: "POST", url: "https://service.internal/ping", headers: { "x-source": "brain" } });
  const handle = hostingEnvironment([]);
  await handle(command({ type: "setup", configuration: {} }));
  assert.equal((await handle(command(attachRequest([provisionOf(ping)]), "att_1"))).receipt.type, "accepted");
  const invoked = await handle(command(invokeRequest("ping", { payload: "x" }), "att_1"));
  assert.deepEqual(invoked.receipt.outcome, { status: "ok", value: { echoed: { method: "POST", url: "https://service.internal/ping", headers: { "x-source": "brain" }, body: '{"payload":"x"}' }, session: "ses_test" } });
});

test("maps thrown errors and deadlines onto the outcome envelope for every kind", async () => {
  const throwing = esmArtifact("boom", toolPayload(`
    parseInput(input) { return input; },
    async run() { throw Object.assign(new Error("kaboom"), { code: "boom_code" }); }`));
  const slow = esmArtifact("slow", toolPayload(`
    parseInput(input) { return input; },
    async run(input, context) { await new Promise((_, reject) => context.signal.addEventListener("abort", () => reject(context.signal.reason))); }`));
  const failingScript = shellManifest("failing", "exit 1");
  const handle = hostingEnvironment([throwing, slow], {
    shell: async (context, script) => { if (script === "exit 1") throw Object.assign(new Error("exited 1"), { code: "exit_1" }); return { exit_code: 0 }; },
  });
  await handle(command({ type: "setup", configuration: {} }));
  const provisions = [throwing.manifest, slow.manifest, failingScript].map(provisionOf);
  assert.equal((await handle(command(attachRequest(provisions), "att_1"))).receipt.type, "accepted");

  const thrown = await handle(command(invokeRequest("boom", {}), "att_1"));
  assert.deepEqual(thrown.receipt.outcome, { status: "error", error: { code: "boom_code", message: "kaboom" } });
  const timedOut = await handle(command(invokeRequest("slow", {}, 25), "att_1"));
  assert.deepEqual(timedOut.receipt.outcome, { status: "timeout" }, "the caller-owned deadline is a distinguished outcome");
  const failed = await handle(command(invokeRequest("failing", {}), "att_1"));
  assert.deepEqual(failed.receipt.outcome, { status: "error", error: { code: "exit_1", message: "exited 1" } });
});

test("a broken or unsatisfiable provision fails the attach receipt, not the first call", async () => {
  const broken = esmArtifact("broken", `throw new Error("broken at import");`);
  const wantsDom = esmArtifact("pager", toolPayload(`parseInput(input) { return input; }, async run() { return null; }`), { needs: ["dom"] });
  const needsToken = esmArtifact("secretive", toolPayload(`parseInput(input) { return input; }, async run() { return null; }`), { binding_names: ["TOKEN"] });
  const handle = hostingEnvironment([broken, wantsDom, needsToken]);
  await handle(command({ type: "setup", configuration: {} }));

  const brokenAttach = await handle(command(attachRequest([provisionOf(broken.manifest)]), "att_1"));
  assert.equal(brokenAttach.receipt.type, "failure");

  const missingResource = await handle(command(attachRequest([provisionOf(wantsDom.manifest)]), "att_2"));
  assert.equal(missingResource.receipt.type, "failure");
  assert.match(missingResource.receipt.message, /needs dom/u);

  const missingBinding = await handle(command(attachRequest([provisionOf(needsToken.manifest)]), "att_3"));
  assert.equal(missingBinding.receipt.type, "failure");
  assert.match(missingBinding.receipt.message, /binding TOKEN/u);

  const unknownIdentity = await handle(command(attachRequest([{ manifest: broken.manifest, payload_identity: "e".repeat(64) }]), "att_4"));
  assert.equal(unknownIdentity.receipt.type, "failure");
  assert.match(unknownIdentity.receipt.message, /not its program/u);

  // The failed attaches leave nothing usable behind.
  const invoked = await handle(command(invokeRequest("broken", {}), "att_1"));
  assert.equal(invoked.receipt.type, "failure");
});

test("an environment launches only the program kinds it registered executors for", async () => {
  const esmOnly = environment({ resources: { process: {} } }, (author) => {
    const box = author.open(async () => ({}));
    box.execute.esm();
    box.close(async () => undefined);
    return {};
  });
  const handle = createEnvironmentHandler(esmOnly);
  const setup = await handle(command({ type: "setup", configuration: {} }));
  assert.deepEqual(setup.receipt.runtimes, ["esm"]);
  const shellAttach = await handle(command(attachRequest([provisionOf(shellManifest("bash", "$command"))], {}), "att_1"));
  assert.equal(shellAttach.receipt.type, "failure");
  assert.match(shellAttach.receipt.message, /shell program, which this environment does not execute/u);

  const nothing = environment((author) => {
    const box = author.open(async () => ({}));
    box.close(async () => undefined);
    return {};
  });
  const bare = createEnvironmentHandler(nothing);
  const bareSetup = await bare(command({ type: "setup", configuration: {} }));
  assert.deepEqual(bareSetup.receipt.runtimes, []);
  assert.deepEqual(bareSetup.receipt.resources, {});
  assert.equal((await bare(command(attachRequest([], {}), "att_1"))).receipt.type, "accepted", "an attach with nothing to provision is fine");
  const invoked = await bare(command(invokeRequest("legacy", { x: 1 }), "att_1"));
  assert.equal(invoked.receipt.type, "failure", "there is no fallback run handler any more");
});

test("the default http executor posts the input as JSON and surfaces the status as a code", async () => {
  const seen = [];
  const original = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    seen.push({ url, method: init.method, headers: init.headers, body: init.body });
    return url.endsWith("/ok") ? Response.json({ pong: true }) : new Response("nope", { status: 503 });
  };
  try {
    const service = environment((author) => {
      const box = author.open(async () => ({}));
      box.execute.http();
      box.close(async () => undefined);
      return {};
    });
    const handle = createEnvironmentHandler(service);
    await handle(command({ type: "setup", configuration: {} }));
    const ok = httpManifest("ok", { method: "POST", url: "https://service.internal/ok", headers: { authorization: "token" } });
    const down = httpManifest("down", { method: "PUT", url: "https://service.internal/down" });
    assert.equal((await handle(command(attachRequest([provisionOf(ok), provisionOf(down)], {}), "att_1"))).receipt.type, "accepted");
    const pong = await handle(command(invokeRequest("ok", { q: 1 }), "att_1"));
    assert.deepEqual(pong.receipt.outcome, { status: "ok", value: { pong: true } });
    assert.deepEqual(seen[0], { url: "https://service.internal/ok", method: "POST", headers: { "content-type": "application/json", authorization: "token" }, body: '{"q":1}' });
    const failed = await handle(command(invokeRequest("down", {}), "att_1"));
    assert.equal(failed.receipt.outcome.status, "error");
    assert.equal(failed.receipt.outcome.error.code, "http_503");
  } finally {
    globalThis.fetch = original;
  }
});

test("the built tool context type carries bindings and no resource handles", async () => {
  const fixture = fileURLToPath(new URL("./fixtures/execution-typing.ts", import.meta.url));
  const compiler = fileURLToPath(import.meta.resolve("typescript/bin/tsc"));
  const args = [compiler, fixture, "--noEmit", "--strict", "--skipLibCheck", "--target", "es2023", "--module", "nodenext", "--moduleResolution", "nodenext"];
  const { code, output } = await new Promise((resolve, reject) => {
    const child = spawn(process.execPath, args, { stdio: ["ignore", "pipe", "pipe"] });
    let output = "";
    child.stdout.on("data", (chunk) => { output += chunk; });
    child.stderr.on("data", (chunk) => { output += chunk; });
    child.once("error", reject);
    child.once("exit", (code) => resolve({ code, output }));
  });
  assert.equal(code, 0, `the typing fixture must compile with its @ts-expect-error assertions:\n${output}`);
});

test("brain build emits one artifact per program kind with its content identity", async () => {
  const out = await mkdtemp(join(tmpdir(), "brain-sdk-artifact-"));
  try {
    const entry = fileURLToPath(new URL("./fixtures/provisioned-entry.mjs", import.meta.url));
    const built = await build({ entry, out });
    const byName = Object.fromEntries(built.map((extension) => [extension.name, extension]));
    assert.deepEqual(built.map(({ name, kind, program }) => [name, kind, program]), [["bash", "tool", "shell"], ["echo", "tool", "esm"], ["ping", "tool", "http"]]);

    const echo = JSON.parse(await readFile(join(out, byName.echo.artifact), "utf8"));
    assert.deepEqual(echo.manifest, {
      name: "echo",
      description: "Echo the text beside the injected base URL.",
      input_schema: z.toJSONSchema(z.object({ text: z.string() })),
      output_schema: z.toJSONSchema(z.object({ echoed: z.string() })),
      needs: ["process"],
      binding_names: ["API_BASE"],
      program: { kind: "esm", identity: identityOf(echo.payload) },
    });
    const bash = JSON.parse(await readFile(join(out, byName.bash.artifact), "utf8"));
    assert.deepEqual(bash.manifest.program, { kind: "shell", identity: identityOf("$command"), script: "$command" });
    assert.equal(bash.payload, "$command");
    const ping = JSON.parse(await readFile(join(out, byName.ping.artifact), "utf8"));
    assert.deepEqual(ping.manifest.program, { kind: "http", identity: identityOf(ping.payload), request: { method: "POST", url: "https://service.internal/ping", headers: { "x-source": "brain" } } });
    assert.deepEqual(ping.manifest.needs, []);

    // The generated index installs each tool's program, so a placed tool carries it
    // to session create.
    const index = await readFile(join(out, "index.mjs"), "utf8");
    assert.match(index, /installExtensionIdentity\(bash, "bash", new URL\("\.\/bash\.tool\.json", import\.meta\.url\), undefined, \{"kind":"shell"/u);

    // The emitted artifacts run end to end on a hosting environment.
    const handle = hostingEnvironment([echo, bash, ping]);
    await handle(command({ type: "setup", configuration: {} }));
    const attached = await handle(command(attachRequest([echo, bash, ping].map((artifact) => provisionOf(artifact.manifest))), "att_1"));
    assert.equal(attached.receipt.type, "accepted", JSON.stringify(attached.receipt));
    const echoed = await handle(command(invokeRequest("echo", { text: "hello" }), "att_1"));
    assert.deepEqual(echoed.receipt.outcome, { status: "ok", value: { echoed: "hello # https://api.internal" } }, "binding values reach the program at runtime");
    const ran = await handle(command(invokeRequest("bash", { command: "echo hi" }), "att_1"));
    assert.equal(ran.receipt.outcome.status, "ok");
    assert.equal(ran.receipt.outcome.value.stdout, "echo hi", "the script reached the shell executor with its input substituted");
  } finally {
    await rm(out, { recursive: true, force: true });
  }
});
