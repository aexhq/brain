import assert from "node:assert/strict";
import { execFile, spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import { promisify } from "node:util";
import { responsesReply } from "./model-responses.mjs";
const requireSDK = createRequire(new URL("../../bin/sdk-consumer/package.json", import.meta.url));
const { Brain, BrainError, Model, ApiKey, CredentialReference, Tool, ManagedTool } = await import(pathToFileURL(requireSDK.resolve("@aexhq/brain")).href);

export { Brain, BrainError, Model, ApiKey, CredentialReference, Tool, ManagedTool, delay };
export const root = fileURLToPath(new URL("../../", import.meta.url));
export const tokens = {
  a: "journey-tenant-a-0000000000000000", b: "journey-tenant-b-0000000000000000",
  reader: "journey-reader-a-0000000000000000", user: "journey-user-a-000000000000000000",
  tool: "journey-tools-0000000000000000000", operator: "journey-operator-0000000000000000",
};
export const config = (tools = []) => ({ harness: "pi", config: {
  model: "openai/gpt-4.1-mini", reasoning_effort: "off", instructions: "Stable journey instructions.", tools,
} });
export const message = (text, key) => ({ type: "user.message", text, ...(key ? { key } : {}) });
export const call = (name, args = {}) => `CALL ${name} ${JSON.stringify(args)}`;
export const result = text => ({ content: [{ type: "text", text }] });

// All temporary state is owned by this suite, including the executables it builds.
export async function build() {
  const dir = await mkdtemp(path.join(os.tmpdir(), "brain-journeys-build-"));
  try {
    for (const [name, source] of [["brain", "./cmd/brain"], ["tools", "./examples/tools"]]) {
      const output = spawnSync("go", ["build", "-o", path.join(dir, name + (process.platform === "win32" ? ".exe" : "")), source], { cwd: root, encoding: "utf8", timeout: 120000 });
      assert.ifError(output.error);
      assert.equal(output.status, 0, output.stderr);
    }
    return dir;
  } catch (error) { await removeTemp(dir); throw error; }
}

export async function removeTemp(dir) {
  const relative = path.relative(os.tmpdir(), dir);
  assert.ok(relative.startsWith("brain-journeys-") && !relative.includes(path.sep), `not a suite temporary directory: ${dir}`);
  await rm(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
}

export async function until(read, accepts = Boolean, label = "condition", timeout = 20000) {
  const deadline = Date.now() + timeout;
  let value;
  do {
    value = await read();
    if (accepts(value)) return value;
    await delay(20);
  } while (Date.now() < deadline);
  assert.fail(`timed out waiting for ${label}; last value: ${JSON.stringify(value)}`);
}

export async function rejects(promise, status, code) {
  await assert.rejects(promise, error => {
    assert.ok(error instanceof BrainError);
    assert.equal(error.status, status);
    assert.equal(error.code, code);
    assert.match(error.requestId, /^[a-f0-9]{32}$/);
    return true;
  });
}

export async function history(api, id, limit = 2) {
  const events = [];
  let after = "";
  for (;;) {
    const page = await api.history(id, after, limit);
    if (!page.events.length) { assert.equal(page.next_cursor, after); return events; }
    assert.notEqual(page.next_cursor, after, "history cursor did not advance");
    events.push(...page.events);
    after = page.next_cursor;
  }
}

export async function state(api, id, expected = "idle") {
  return until(async () => {
    const value = await api.get(id);
    if (["failed", "interrupted", "closed"].includes(value.status) && value.status !== expected) {
      assert.fail(`unexpected session state ${value.status}: ${JSON.stringify(await history(api, id))}`);
    }
    return value;
  }, value => value.status === expected, `session ${expected}`);
}

// Wait for the HTTP subscription handshake before producing events. The SDK is
// still responsible for parsing the real SSE response and cancelling its reader.
async function observe(api, id, options = {}) {
  const controller = new AbortController();
  const ready = Promise.withResolvers();
  const events = [];
  const client = new Brain(api.url, tokens.a, async (...args) => {
    try { const response = await fetch(...args); ready.resolve(); return response; }
    catch (error) { ready.reject(error); throw error; }
  });
  let failure;
  const done = (async () => {
    try { for await (const event of client.observe(id, { ...options, signal: controller.signal })) events.push(event); }
    catch (error) { if (!controller.signal.aborted) failure = error; }
  })();
  const stop = async () => { controller.abort(); await done; if (failure) throw failure; };
  await ready.promise;
  return { events, done, stop, wait: (predicate, label = "stream event") => until(() => {
    if (failure) throw failure;
    return events.find(predicate);
  }, Boolean, label) };
}

function startProcess(command, args, env, options = {}) {
  const child = spawn(command, args, { cwd: root, env, windowsHide: true, detached: process.platform !== "win32", stdio: ["ignore", "pipe", "pipe"], ...options });
  let output = "", stdout = "";
  let ended;
  const completion = new Promise(resolve => {
    child.on("error", error => { ended = { error: error.message }; resolve(ended); });
    child.on("exit", (code, signal) => { ended = { code, signal }; resolve(ended); });
  });
  for (const stream of [child.stdout, child.stderr]) stream.on("data", chunk => { output = (output + chunk).slice(-65536); });
  child.stdout.on("data", chunk => { stdout = (stdout + chunk).slice(-65536); });
  return {
    child, completion, get output() { return output; }, get stdout() { return stdout; }, get ended() { return ended; },
    async stop() {
      if (ended) return;
      // Kill only this owned process tree, including its Pi child. This is also
      // the crash injection used on both Windows and Linux (no skipped paths).
      if (process.platform === "win32") {
        await promisify(execFile)("taskkill", ["/pid", String(child.pid), "/t", "/f"], { windowsHide: true, timeout: 10000 });
      } else {
        try { process.kill(-child.pid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
      }
      await until(() => ended, Boolean, "process exit", 10000);
    },
  };
}

export async function localHTTP(handler, errors = []) {
  const server = http.createServer((req, res) => {
    Promise.resolve().then(() => handler(req, res)).catch(error => {
      errors.push(error.stack); res.destroy();
    });
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    async close() { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); },
  };
}

export async function jsonBody(req) {
  let body = "";
  for await (const chunk of req) body += chunk;
  return JSON.parse(body);
}

export function modelReply(res, answer, usage) {
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  const chunk = (delta, finish_reason = null) => res.write(`data: ${JSON.stringify({
    id: "journey", object: "chat.completion.chunk", created: 1, model: "gpt-4.1-mini",
    choices: [{ index: 0, delta, finish_reason }],
    ...(finish_reason && usage ? { usage } : {}),
  })}\n\n`);
  chunk({ role: "assistant" });
  if (typeof answer === "string") chunk({ content: answer });
  else chunk({ tool_calls: [{ index: 0, id: "provider-call", type: "function", function: {
    name: answer.name, arguments: JSON.stringify(answer.args),
  } }] });
  chunk({}, typeof answer === "string" ? "stop" : "tool_calls");
  res.end("data: [DONE]\n\n");
}

export async function fixture(t, binaries, options = {}) {
  const dir = await mkdtemp(path.join(os.tmpdir(), "brain-journeys-state-"));
  const processes = [], servers = [], subscriptions = [], errors = [];
  let brain;
  t.after(async () => {
    try {
      const cleanup = [...subscriptions.toReversed().map(s => () => s.stop()), ...processes.toReversed().map(p => () => p.stop()), ...servers.toReversed().map(s => () => s.close())];
      for (const close of cleanup) {
        try { await close(); } catch (error) { errors.push(error.stack); }
      }
      assert.deepEqual(errors, [], "fixture service failed");
    } finally {
      if (!t.passed) t.diagnostic(processes.map(proc => proc.output).join("\n"));
      await removeTemp(dir);
    }
  });
  const binary = name => path.join(binaries, name + (process.platform === "win32" ? ".exe" : ""));
  const env = { ...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith("OTEL_"))),
    JOURNEY_A: tokens.a, JOURNEY_B: tokens.b, JOURNEY_READER: tokens.reader, JOURNEY_USER: tokens.user,
    JOURNEY_MODEL: "local-recording-provider-key", BRAIN_TOOLS_KEY: tokens.tool, BRAIN_APPROVAL_KEY: tokens.operator,
    OTEL_EXPORTER_OTLP_ENDPOINT: "", OTEL_TRACES_EXPORTER: "none", OTEL_METRICS_EXPORTER: "none",
  };
  const listen = async handler => { const server = await localHTTP(handler, errors); servers.push(server); return server; };
  const reserved = await localHTTP(() => {});
  const url = reserved.url;
  await reserved.close();
  const model = { requests: [], authorization: [], respond: undefined };
  const provider = await listen(async (req, res) => {
    assert.equal(req.url, options.api === "openai-responses" ? "/v1/responses" : "/v1/chat/completions");
    assert.ok((options.modelKeys ?? ["local-recording-provider-key"]).some(key => req.headers.authorization === `Bearer ${key}`), "unexpected provider credential");
    model.authorization.push(req.headers.authorization);
    const input = await jsonBody(req);
    model.requests.push(input);
    if (model.respond) { await model.respond(input, res, req); return; }
    const last = (input.messages ?? input.input).at(-1);
    const text = typeof last.content === "string" ? last.content : (last.content ?? []).filter(p => ["text", "input_text", "output_text"].includes(p.type)).map(p => p.text).join("");
    const match = last.role === "user" && /^CALL (\w+) (.+)$/s.exec(text);
    const answer = match ? { name: match[1], args: JSON.parse(match[2]) } : last.role === "tool" || last.type === "function_call_output" ? "tool handled" : `answer: ${text}`;
    (options.api === "openai-responses" ? responsesReply : modelReply)(res, answer);
  });
  const settings = {
    listen: new URL(url).host, public_url: url, data_dir: path.join(dir, "cache"),
    tokens: [
      { token_env: "JOURNEY_A", tenant: "a", manage: true }, { token_env: "JOURNEY_B", tenant: "b", manage: true },
      { token_env: "JOURNEY_READER", tenant: "a", read_only: true }, { token_env: "JOURNEY_USER", tenant: "a" },
    ],
    harnesses: { pi: { command: [process.execPath, path.join(root, "runners/pi/dist/main.js")] } },
    providers: { openai: { api_key_env: "JOURNEY_MODEL", base_url: provider.url + "/v1", api: options.api ?? "openai-completions" } },
    limits: { active: 2, loaded: 16, queue: 2, input_bytes: 4096, session_bytes: 16 << 20, subscribers: 4, subscriber_events: 128, subscriber_bytes: 1 << 20, ...options.limits },
    tools: [],
    ...(options.persistence ? { persistence: { provider: "filesystem", directory: path.join(dir, "archive"), interval_seconds: 3600, queue: 32, ...options.persistence } } : {}),
  };
  const api = new Brain(url, tokens.a);
  const f = {
    dir, url, env, settings, api, model, listen,
    b: new Brain(url, tokens.b), reader: new Brain(url, tokens.reader), user: new Brain(url, tokens.user),
    get logs() { return brain?.output ?? ""; },
    async observe(id, options) {
      const stream = await observe(api, id, options);
      subscriptions.push(stream);
      return stream;
    },
    async start() {
      await writeFile(path.join(dir, "brain.json"), JSON.stringify(settings));
      brain = startProcess(binary("brain"), ["-config", path.join(dir, "brain.json")], env);
      processes.push(brain);
      await until(async () => {
        assert.ok(!brain.ended, `Brain exited: ${brain.output}`);
        try { return (await fetch(url + "/health/ready", { signal: AbortSignal.timeout(500) })).status; }
        catch { return 0; }
      }, status => status === 200, "Brain listener");
    },
    async check(value = settings) {
      const file = path.join(dir, "check.json");
      await writeFile(file, JSON.stringify(value));
      const checked = spawnSync(binary("brain"), ["-config", file, "-check"], { cwd: root, env, encoding: "utf8", timeout: 5000 });
      assert.ifError(checked.error);
      return checked;
    },
    async crash() { await brain.stop(); },
    async tool(name, handler, extra = {}) {
      const server = await listen(handler);
      settings.tools.push({ name, description: `Journey ${name}`, input_schema: { type: "object" }, url: server.url, tenants: ["a"], token_env: "BRAIN_TOOLS_KEY", ...extra });
      return server;
    },
    async examples() {
      const reserved = await localHTTP(() => {});
      const toolURL = reserved.url;
      await reserved.close();
      const args = ["-listen", new URL(toolURL).host, "-brain", url, "-tenant", "a"];
      const printed = spawnSync(binary("tools"), [...args, "-print-config"], { env, encoding: "utf8" });
      assert.ifError(printed.error); assert.equal(printed.status, 0, printed.stderr);
      settings.tools.push(...JSON.parse(printed.stdout).tools);
      const proc = startProcess(binary("tools"), args, env);
      processes.push(proc);
      const operator = async (pathname = "/approvals", body, token = tokens.operator) => fetch(toolURL + pathname, {
        method: body === undefined ? "GET" : "POST", headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal: AbortSignal.timeout(5000),
      });
      await until(async () => {
        assert.ok(!proc.ended, proc.output);
        try { return (await operator()).status === 200; } catch { return false; }
      });
      return { operator, pending: async () => (await (await operator()).json()).approvals };
    },
    async python(script, extra = {}) {
      const executable = path.join(root, "bin/sdk-python", process.platform === "win32" ? "Scripts/python.exe" : "bin/python");
      const args = ["-I", script];
      const proc = startProcess(executable, args, { ...env, PYTHONPATH: "", JOURNEY_URL: url, ...extra });
      processes.push(proc);
      const ended = await until(() => proc.ended, Boolean, "Python SDK journey", 30000);
      assert.equal(ended.code, 0, proc.output);
      return JSON.parse(proc.stdout);
    },
  };
  return f;
}
