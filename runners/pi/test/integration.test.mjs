import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { cp, mkdtemp, rm } from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import readline from "node:readline";
import { setTimeout as delay } from "node:timers/promises";
import { test } from "node:test";

function client(t, toolResult) {
  const child = spawn(process.execPath, ["dist/main.js"], { stdio: ["pipe", "pipe", "pipe"] });
  let diagnostic = "";
  child.stderr.on("data", b => { diagnostic = (diagnostic + b).slice(-8192); });
  const waiting = new Map();
  let sequence = 0;
  const events = [];
  const lines = readline.createInterface({ input: child.stdout });
  lines.on("line", line => {
    const frame = JSON.parse(line);
    if (frame.event) { events.push(frame); return; }
    if (frame.method === "tool.call") {
      child.stdin.write(JSON.stringify({ id: frame.id, result: toolResult(frame.params) }) + "\n");
      return;
    }
    const pending = waiting.get(frame.id);
    assert.ok(pending, `unexpected response ${line}`);
    waiting.delete(frame.id); clearTimeout(pending.timer);
    if (frame.error) pending.reject(new Error(JSON.stringify(frame.error)));
    else pending.resolve(frame.result);
  });
  child.on("exit", code => {
    for (const p of waiting.values()) { clearTimeout(p.timer); p.reject(new Error(`runner exited ${code}: ${diagnostic}`)); }
    waiting.clear();
  });
  const close = async () => { if (child.exitCode !== null || child.signalCode) return; const ended = once(child, "exit"); child.kill(); await ended; };
  t.after(close);
  return {
    events, close,
    request(method, params) {
      return new Promise((resolve, reject) => {
        const id = String(++sequence);
        const timer = setTimeout(() => { reject(new Error(`timeout: ${method}; ${diagnostic}`)); child.kill(); }, 20000);
        waiting.set(id, { resolve, reject, timer });
        child.stdin.write(JSON.stringify({ id, method, params }) + "\n");
      });
    },
  };
}

async function provider(t) {
  const requests = [];
  const server = http.createServer(async (req, res) => {
    let body = ""; for await (const part of req) body += part;
    const data = JSON.parse(body); requests.push(data);
    const last = data.messages.at(-1);
    const tool = last.role === "user" && JSON.stringify(last.content).includes("call tool");
    res.writeHead(200, { "Content-Type": "text/event-stream" });
    const chunk = delta => res.write(`data: ${JSON.stringify({ id: "chatcmpl-test", object: "chat.completion.chunk", created: 1, model: "gpt-4.1-mini", choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
    chunk({ role: "assistant" });
    if (tool) chunk({ tool_calls: [{ index: 0, id: "call-test", type: "function", function: { name: "echo", arguments: '{"text":"hello"}' } }] });
    else chunk({ content: "completed" });
    res.write(`data: ${JSON.stringify({ id: "chatcmpl-test", object: "chat.completion.chunk", created: 1, model: "gpt-4.1-mini", choices: [{ index: 0, delta: {}, finish_reason: tool ? "tool_calls" : "stop" }], usage: { prompt_tokens: 20, completion_tokens: 5, total_tokens: 25 } })}\n\n`);
    res.end("data: [DONE]\n\n");
  });
  server.listen(0, "127.0.0.1"); await once(server, "listening");
  t.after(() => { server.closeAllConnections(); return new Promise(resolve => server.close(resolve)); });
  return { requests, url: `http://127.0.0.1:${server.address().port}/v1` };
}

const tool = { name: "echo", description: "Echo text", input_schema: { type: "object", properties: { text: { type: "string" } }, required: ["text"], additionalProperties: false } };
const result = () => ({ content: [{ type: "text", text: "external result" }] });
function options(dir, url) {
  return { session_id: "test", dir, config: { model: "openai/gpt-4.1-mini", reasoning_effort: "off", instructions: "Stable instructions.", tools: ["echo"] }, tools: [tool], provider: { api_key: "test-key", base_url: url, api: "openai-completions" } };
}

test("real Pi: remote tools only, passive restore, and stable context after relocation", { timeout: 60000 }, async t => {
  const root = await mkdtemp(path.join(os.tmpdir(), "brain-pi-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const api = await provider(t);
  const first = client(t, result);
  assert.deepEqual(await first.request("open", options(path.join(root, "first"), api.url)), { tools: ["echo"] });
  assert.equal(api.requests.length, 0, "opening a session invoked a model");
  await first.request("prompt", { text: "call tool" });
  assert.equal(api.requests.length, 2);
  assert.deepEqual(api.requests[0].tools.map(t => t.function.name), ["echo"]);
  assert.equal(api.requests[0].messages[0].content, "Stable instructions.");
  assert.ok(api.requests[1].messages.some(m => m.role === "tool" && m.content.includes("external result")));
  const checkpoint = await first.request("checkpoint");
  assert.equal(checkpoint.length, 1); assert.equal(checkpoint[0].append_only, true);
  assert.ok(checkpoint[0].size > 0);
  await first.close();
  await cp(path.join(root, "first"), path.join(root, "restored"), { recursive: true });
  const second = client(t, result);
  await second.request("open", options(path.join(root, "restored"), api.url));
  await delay(100);
  assert.equal(api.requests.length, 2, "restore invoked a model");
  await second.request("prompt", { text: "next input" });
  const restored = api.requests[2].messages;
  assert.deepEqual(restored.slice(0, api.requests[1].messages.length), api.requests[1].messages);
  assert.ok(JSON.stringify(restored.at(-1).content).includes("next input"));
  await second.close();
});

test("real Pi: tool failure is model-visible and execution continues", { timeout: 30000 }, async t => {
  const dir = await mkdtemp(path.join(os.tmpdir(), "brain-pi-error-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  const api = await provider(t);
  const runner = client(t, () => ({ is_error: true, content: [{ type: "text", text: "approval denied" }] }));
  await runner.request("open", options(dir, api.url));
  await runner.request("prompt", { text: "call tool" });
  assert.ok(api.requests[1].messages.some(m => m.role === "tool" && m.content.includes("approval denied")));
  assert.ok(runner.events.some(e => e.event === "assistant.message" && e.data.text === "completed"));
  await runner.close();
});

