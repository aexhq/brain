import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { z } from "zod";
import { CapabilityError, clamp, createEnvironmentHandler, environment } from "../dist/index.js";
import { build, unusedRequiresWarnings } from "../dist/build.js";

const identityOf = (payload) => createHash("sha256").update(payload).digest("hex");
const artifactOf = (name, payload, { requires = [], binding_names = [] } = {}) => ({
  manifest: {
    name,
    description: "test tool",
    input_schema: { type: "object" },
    requires,
    binding_names,
    hosting: "provisioned",
    payload: { kind: "esm", identity: identityOf(payload) },
  },
  payload,
});
const toolPayload = (members) => `export default { kind: "brain.provisioned-tool/v1", ${members} };`;

let sequence = 0;
const command = (request, attachment_id) => {
  sequence += 1;
  const id = `op_${sequence}`;
  return {
    contract: "environment/v2",
    binding: {},
    operation: { operation_id: id, request_identity: id.padEnd(64, "a"), environment_id: "env_1", session_id: "ses_test", ...(attachment_id === undefined ? {} : { attachment_id }), request },
  };
};
const attachRequest = (provisions, overrides = {}) => ({
  type: "attach",
  grants: { exec: { timeout_ms_max: 1_000 }, fs: { root: "/workspace" } },
  provisions,
  bindings: { API_BASE: "https://api.internal" },
  ...overrides,
});
const invokeRequest = (tool, input, deadline_ms = 5_000) => ({ type: "invoke", call_id: `call_${sequence}`, tool, input, deadline_ms });

function hostingEnvironment(artifacts) {
  const definition = environment((author) => {
    const box = author.open(async () => ({}));
    box.run(async () => undefined);
    box.close(async () => undefined);
    box.provide.exec(({ grants }) => ({
      run: async (cmd, opts) => ({ exitCode: 0, stdout: JSON.stringify(clamp(opts, grants.exec)), stderr: cmd }),
    }));
    box.provide.fs(({ grants }) => ({
      read: async (path) => new TextEncoder().encode(clamp.path(grants.fs.root, path)),
      write: async (path) => void clamp.path(grants.fs.root, path),
      list: async () => [],
    }));
    box.host.esm({ artifacts });
    return {};
  });
  return createEnvironmentHandler(definition);
}

test("clamp bounds exec options against the grant", () => {
  assert.deepEqual(clamp({ timeoutMs: 999_999 }, { timeout_ms_max: 1_000 }), { timeoutMs: 1_000 });
  assert.deepEqual(clamp({ timeoutMs: 5 }, { timeout_ms_max: 1_000 }), { timeoutMs: 5 });
  assert.deepEqual(clamp(undefined, { timeout_ms_max: 1_000 }), { timeoutMs: 1_000 });
  assert.deepEqual(clamp({ cwd: "/x" }, undefined), { cwd: "/x" });
});

test("clamp.path confines resolved paths to the granted root", () => {
  assert.equal(clamp.path("/workspace", "a/b.txt"), "/workspace/a/b.txt");
  assert.equal(clamp.path("/workspace", "a/../b.txt"), "/workspace/b.txt");
  assert.equal(clamp.path("/workspace", "/workspace/a"), "/workspace/a");
  assert.equal(clamp.path("/workspace/", "."), "/workspace");
  for (const escape of ["../secret", "a/../../secret", "/etc/passwd", "/workspacefake/a"]) {
    assert.throws(() => clamp.path("/workspace", escape), (error) => error instanceof CapabilityError && error.capability === "fs" && error.code === "path_escape");
  }
});

test("setup and attach receipts report what the environment provides", async () => {
  const handle = hostingEnvironment([]);
  const setup = await handle(command({ type: "setup", configuration: {} }));
  assert.equal(setup.receipt.type, "accepted");
  assert.deepEqual(setup.receipt.provides, ["exec", "fs"]);
  const attached = await handle(command(attachRequest([]), "att_1"));
  assert.equal(attached.receipt.type, "accepted");
  assert.deepEqual(attached.receipt.provides, ["exec", "fs"]);
});

test("hosts a provisioned tool: happy path with clamped grants and injected bindings", async () => {
  const payload = toolPayload(`
    parseInput(input) { if (typeof input.command !== "string") throw new Error("command must be a string"); return input; },
    async run(input, context) {
      const result = await context.exec.run(input.command, { timeoutMs: 999_999_999 });
      return { exit: result.exitCode, opts: JSON.parse(result.stdout), base: context.bindings.API_BASE, callId: context.callId };
    }`);
  const artifact = artifactOf("echo", payload, { requires: ["exec"], binding_names: ["API_BASE"] });
  const handle = hostingEnvironment([artifact]);
  await handle(command({ type: "setup", configuration: {} }));
  const attached = await handle(command(attachRequest([{ manifest: artifact.manifest, payload_identity: artifact.manifest.payload.identity }]), "att_1"));
  assert.equal(attached.receipt.type, "accepted");
  const invoked = await handle(command(invokeRequest("echo", { command: "run it" }), "att_1"));
  assert.equal(invoked.receipt.type, "outcome");
  assert.equal(invoked.receipt.outcome.status, "ok");
  assert.deepEqual(invoked.receipt.outcome.value.opts, { timeoutMs: 1_000 }, "the provider clamps the requested timeout to the grant");
  assert.equal(invoked.receipt.outcome.value.base, "https://api.internal");
  assert.equal(invoked.receipt.outcome.value.exit, 0);

  const invalid = await handle(command(invokeRequest("echo", { command: 7 }), "att_1"));
  assert.equal(invalid.receipt.outcome.status, "error");
  assert.equal(invalid.receipt.outcome.error.code, "invalid_input");
});

test("maps thrown errors, deadlines, and fs escapes onto the outcome envelope", async () => {
  const throwing = artifactOf("boom", toolPayload(`
    parseInput(input) { return input; },
    async run() { throw Object.assign(new Error("kaboom"), { code: "boom_code" }); }`));
  const slow = artifactOf("slow", toolPayload(`
    parseInput(input) { return input; },
    async run(input, context) { await new Promise((_, reject) => context.signal.addEventListener("abort", () => reject(context.signal.reason))); }`));
  const reader = artifactOf("reader", toolPayload(`
    parseInput(input) { return input; },
    async run(input, context) { return new TextDecoder().decode(await context.fs.read(input.path)); }`), { requires: ["fs"] });
  const handle = hostingEnvironment([throwing, slow, reader]);
  await handle(command({ type: "setup", configuration: {} }));
  const provisions = [throwing, slow, reader].map((artifact) => ({ manifest: artifact.manifest, payload_identity: artifact.manifest.payload.identity }));
  assert.equal((await handle(command(attachRequest(provisions), "att_1"))).receipt.type, "accepted");

  const thrown = await handle(command(invokeRequest("boom", {}), "att_1"));
  assert.deepEqual(thrown.receipt.outcome, { status: "error", error: { code: "boom_code", message: "kaboom" } });

  const timedOut = await handle(command(invokeRequest("slow", {}, 25), "att_1"));
  assert.deepEqual(timedOut.receipt.outcome, { status: "timeout" }, "the caller-owned deadline is a distinguished outcome");

  const inside = await handle(command(invokeRequest("reader", { path: "notes/a.txt" }), "att_1"));
  assert.deepEqual(inside.receipt.outcome, { status: "ok", value: "/workspace/notes/a.txt" });
  const escape = await handle(command(invokeRequest("reader", { path: "../etc/passwd" }), "att_1"));
  assert.equal(escape.receipt.outcome.status, "error");
  assert.equal(escape.receipt.outcome.error.code, "path_escape", "a CapabilityError keeps its code through the outcome");
});

test("a broken or unsatisfiable provision fails the attach receipt, not the first call", async () => {
  const broken = artifactOf("broken", `throw new Error("broken at import");`);
  const wantsPage = artifactOf("pager", toolPayload(`parseInput(input) { return input; }, async run() { return null; }`), { requires: ["page"] });
  const needsToken = artifactOf("secretive", toolPayload(`parseInput(input) { return input; }, async run() { return null; }`), { binding_names: ["TOKEN"] });
  const handle = hostingEnvironment([broken, wantsPage, needsToken]);
  await handle(command({ type: "setup", configuration: {} }));

  const provisionOf = (artifact) => [{ manifest: artifact.manifest, payload_identity: artifact.manifest.payload.identity }];
  const brokenAttach = await handle(command(attachRequest(provisionOf(broken)), "att_1"));
  assert.equal(brokenAttach.receipt.type, "failure");

  const missingCapability = await handle(command(attachRequest(provisionOf(wantsPage)), "att_2"));
  assert.equal(missingCapability.receipt.type, "failure");
  assert.match(missingCapability.receipt.message, /requires page/u);

  const missingBinding = await handle(command(attachRequest(provisionOf(needsToken)), "att_3"));
  assert.equal(missingBinding.receipt.type, "failure");
  assert.match(missingBinding.receipt.message, /binding TOKEN/u);

  const unknownIdentity = await handle(command(attachRequest([{ manifest: broken.manifest, payload_identity: "e".repeat(64) }]), "att_4"));
  assert.equal(unknownIdentity.receipt.type, "failure");
  assert.match(unknownIdentity.receipt.message, /no ESM payload/u);

  // The failed attaches leave nothing usable behind.
  const invoked = await handle(command(invokeRequest("broken", {}), "att_1"));
  assert.equal(invoked.receipt.type, "failure");
});

test("environments without host.esm keep provisions on their own run handler", async () => {
  const bare = environment((author) => {
    const box = author.open(async () => ({}));
    box.run(async (request) => ({ echoed: request.input }));
    box.close(async () => undefined);
    return {};
  });
  const handle = createEnvironmentHandler(bare);
  await handle(command({ type: "setup", configuration: {} }));
  // No SDK hosting here, so the provision is tolerated and its invocations run
  // through the environment's own run handler — the legacy runtime path.
  const artifact = artifactOf("any", toolPayload(`parseInput(input) { return input; }, async run() { return null; }`));
  assert.equal((await handle(command(attachRequest([{ manifest: artifact.manifest, payload_identity: artifact.manifest.payload.identity }]), "att_1"))).receipt.type, "accepted");
  const provisioned = await handle(command(invokeRequest("any", { y: 2 }), "att_1"));
  assert.deepEqual(provisioned.receipt.outcome, { status: "ok", value: { echoed: { y: 2 } } });

  assert.equal((await handle(command(attachRequest([]), "att_2"))).receipt.type, "accepted");
  const invoked = await handle(command(invokeRequest("legacy", { x: 1 }), "att_2"));
  assert.deepEqual(invoked.receipt.outcome, { status: "ok", value: { echoed: { x: 1 } } });
});

test("unused-requires stays a warning keyed on handle appearance", () => {
  assert.deepEqual(unusedRequiresWarnings("t", ["exec"], "await context.exec.run(x)"), []);
  assert.deepEqual(unusedRequiresWarnings("t", ["fs"], "const { fs } = context;"), []);
  const warned = unusedRequiresWarnings("t", ["exec", "fs"], `tool({ requires: ["exec", "fs"] }); context.exec.run(x);`);
  assert.equal(warned.length, 1);
  assert.match(warned[0], /"fs"/u);
});

test("an undeclared capability does not type-check", async () => {
  const fixture = fileURLToPath(new URL("./fixtures/capability-typing.ts", import.meta.url));
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

test("brain build emits the tool artifact: golden manifest, content identity, unused warning", async () => {
  const out = await mkdtemp(join(tmpdir(), "brain-sdk-artifact-"));
  try {
    const entry = fileURLToPath(new URL("./fixtures/provisioned-entry.mjs", import.meta.url));
    const built = await build({ entry, out });
    const bash = built.find((extension) => extension.name === "bash");
    const idle = built.find((extension) => extension.name === "idle");
    assert.equal(bash.kind, "tool");
    assert.equal(bash.warnings, undefined, "a used capability does not warn");
    assert.equal(idle.warnings.length, 1);
    assert.match(idle.warnings[0], /"fs"/u);

    const artifact = JSON.parse(await readFile(join(out, bash.artifact), "utf8"));
    assert.deepEqual(artifact.manifest, {
      name: "bash",
      description: "Run a shell command in the session workspace.",
      input_schema: z.toJSONSchema(z.object({ command: z.string(), timeout_ms: z.number().int().optional() })),
      output_schema: z.toJSONSchema(z.object({ exit_code: z.number(), stdout: z.string(), stderr: z.string() })),
      requires: ["exec"],
      binding_names: ["API_BASE"],
      hosting: "provisioned",
      payload: { kind: "esm", identity: identityOf(artifact.payload) },
    });
    assert.equal(bash.identity, artifact.manifest.payload.identity);

    // The emitted artifact runs end to end on a hosting environment.
    const handle = hostingEnvironment([artifact]);
    await handle(command({ type: "setup", configuration: {} }));
    const attached = await handle(command(attachRequest([{ manifest: artifact.manifest, payload_identity: artifact.manifest.payload.identity }]), "att_1"));
    assert.equal(attached.receipt.type, "accepted");
    const invoked = await handle(command(invokeRequest("bash", { command: "echo hi" }), "att_1"));
    assert.equal(invoked.receipt.outcome.status, "ok");
    const value = invoked.receipt.outcome.value;
    assert.equal(value.exit_code, 0);
    assert.deepEqual(JSON.parse(value.stdout), { timeoutMs: 1_000 }, "grants clamp the tool's requested timeout");
    assert.equal(value.stderr, "echo hi # https://api.internal", "binding values reach the tool at runtime");
  } finally {
    await rm(out, { recursive: true, force: true });
  }
});
