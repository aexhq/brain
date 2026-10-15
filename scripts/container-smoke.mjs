// Exercises the packaged server and real Pi runner against a local recording provider.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { once } from "node:events";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";

const image = process.argv[2] ?? "brain:local-dev";
const name = `brain-smoke-${randomBytes(8).toString("hex")}`;
const token = randomBytes(32).toString("hex");
const directory = await mkdtemp(path.join(os.tmpdir(), "brain-container-"));
let calls = 0;
const provider = http.createServer(async (req, res) => {
  let bytes = 0;
  for await (const part of req) { bytes += part.length; if (bytes > 65536) { res.writeHead(413).end(); return; } }
  calls++;
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  const chunk = (delta, finish_reason) => `data: ${JSON.stringify({ id: "smoke", object: "chat.completion.chunk", choices: [{ index: 0, delta, finish_reason }] })}\n\n`;
  res.end(chunk({ role: "assistant", content: "container smoke passed" }, null) + chunk({}, "stop") + "data: [DONE]\n\n");
});
const docker = args => execFileSync("docker", args, {
  encoding: "utf8", timeout: 60000,
  env: { ...process.env, BRAIN_API_KEY: token, OPENAI_API_KEY: "local-recording-key" },
}).trim();
let started = false;
try {
  provider.listen(0, "0.0.0.0"); await once(provider, "listening");
  const config = JSON.parse(await readFile("examples/local.json", "utf8"));
  config.listen = "0.0.0.0:8080";
  config.data_dir = "/data";
  config.providers.openai.base_url = `http://host.docker.internal:${provider.address().port}/v1`;
  config.providers.openai.api = "openai-completions";
  const file = path.join(directory, "brain.json");
  await writeFile(file, JSON.stringify(config));
  docker(["run", "--detach", "--name", name, "--publish", "127.0.0.1::8080", "--add-host", "host.docker.internal:host-gateway", "--mount", `type=bind,source=${file},target=/config/brain.json,readonly`, "--env", "BRAIN_API_KEY", "--env", "OPENAI_API_KEY", image]);
  started = true;
  const url = `http://${docker(["port", name, "8080/tcp"]).split("\n")[0]}`;
  const deadline = Date.now() + 30000;
  while (true) {
    try { if ((await fetch(`${url}/health/ready`, { signal: AbortSignal.timeout(1000) })).ok) break; } catch {}
    if (Date.now() > deadline) throw new Error("container did not become ready");
    await delay(100);
  }
  const request = async (method, route, input) => {
    const response = await fetch(url + route, { method, headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" }, body: input === undefined ? undefined : JSON.stringify(input), signal: AbortSignal.timeout(3000) });
    const value = await response.json(); assert.ok(response.ok, JSON.stringify(value)); return value;
  };
  const session = await request("POST", "/sessions", { harness: "pi", config: { model: "openai/gpt-4.1-mini", reasoning_effort: "off", tools: [] } });
  await request("POST", `/sessions/${session.id}/events`, { type: "user.message", text: "local package smoke" });
  while (true) {
    const state = await request("GET", `/sessions/${session.id}`);
    if (state.status === "idle") break;
    if (state.status !== "running" || Date.now() > deadline) throw new Error(`container execution failed: ${JSON.stringify(await request("GET", `/sessions/${session.id}/events`))}`);
    await delay(100);
  }
  const history = await request("GET", `/sessions/${session.id}/events`);
  assert.ok(history.events.some(e => e.type === "assistant.message" && e.data.text === "container smoke passed"));
  assert.equal(calls, 1);
  assert.equal(docker(["exec", name, "id", "-u"]), "1000");
  console.log("Container smoke passed: non-root server, SQLite, real Pi, and recorded model response.");
} catch (error) {
  if (started) console.error(docker(["logs", name]));
  throw error;
} finally {
  if (started) docker(["rm", "--force", name]);
  provider.closeAllConnections(); await new Promise(resolve => provider.close(resolve));
  if (path.dirname(path.resolve(directory)) !== path.resolve(os.tmpdir())) throw new Error("unexpected smoke-test directory");
  await rm(directory, { recursive: true, force: true });
}
