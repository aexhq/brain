import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { mkdir, readFile, writeFile, appendFile, stat, copyFile, open } from "node:fs/promises";
import path from "node:path";
import { RunnerSession, serve, lines, fatal, identity, unknownUsage } from "../../common/bridge.mjs";

const require = createRequire(import.meta.url);
const binary = path.join(path.dirname(require.resolve("@openai/codex/package.json")), "bin/codex.js");
let native, context, boot, sequence = 0;
const waiting = new Map(), threads = new Map();
function rpc(method, params) {
  if (!native || native.exitCode !== null) return Promise.reject(new Error("native server unavailable"));
  const id = ++sequence;
  return new Promise((resolve, reject) => { waiting.set(id, { resolve, reject }); native.stdin.write(JSON.stringify({ id, method, params }) + "\n"); });
}
function launch(options) {
  if (boot) {
    if (context !== createHash("sha256").update(JSON.stringify(options.provider)).digest("hex")) throw new Error("incompatible native credential context");
    return boot;
  }
  context = createHash("sha256").update(JSON.stringify(options.provider)).digest("hex");
  boot = launchNative(options);
  return boot;
}
async function launchNative(options) {
  if (options.provider.api && options.provider.api !== "openai-responses") throw Object.assign(new Error(), { code: "unsupported", safeMessage: "Codex requires the Responses transport" });
  const home = path.join(process.cwd(), "codex"); await mkdir(home, { recursive: true });
  const base = options.provider.base_url || "https://api.openai.com/v1";
  await writeFile(path.join(home, "config.toml"), `model_provider="brain"\nweb_search="disabled"\n[model_providers.brain]\nname="Brain"\nbase_url=${JSON.stringify(base)}\nwire_api="responses"\nenv_key="BRAIN_MODEL_KEY"\nrequires_openai_auth=false\nsupports_websockets=false\n[features]\nshell_tool=false\nimage_generation=false\napps=false\nplugins=false\nmemories=false\nmulti_agent=false\ngoals=false\ncollaboration_modes=false\n[tools.experimental_request_user_input]\nenabled=false\n`);
  native = spawn(process.execPath, [binary, "app-server"], { cwd: process.cwd(), env: { ...process.env, CODEX_HOME: home, BRAIN_MODEL_KEY: options.provider.api_key }, stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
  native.stderr.on("data", () => {}); // Diagnostics can contain provider payloads.
  native.on("error", fatal); native.on("exit", fatal);
  lines(native.stdout, async message => {
    if (message.id !== undefined && !message.method) {
      const pending = waiting.get(message.id); if (!pending) fatal(); waiting.delete(message.id);
      if (message.error) pending.reject(new Error("native RPC rejected")); else pending.resolve(message.result);
      return;
    }
    const session = threads.get(message.params?.threadId);
    if (message.id !== undefined) {
      let result;
      if (message.method === "item/tool/call" && session) {
        try {
          const tool = message.params.tool;
          if (!session.tools.has(tool)) throw new Error("tool is not enabled");
          const value = await session.tool({ id: message.params.callId, name: tool, arguments: message.params.arguments });
          result = { success: !value.is_error, contentItems: value.content.map(c => c.type === "image" ? { type: "inputImage", imageUrl: `data:${c.mime_type};base64,${c.data}` } : { type: "inputText", text: c.text ?? "" }) };
        } catch { result = { success: false, contentItems: [{ type: "inputText", text: "tool dispatch failed" }] }; void session.interrupt(); }
        native.stdin.write(JSON.stringify({ id: message.id, result }) + "\n");
      } else native.stdin.write(JSON.stringify({ id: message.id, error: { code: -32601, message: "native action is not enabled" } }) + "\n");
      return;
    }
    if (!session) return;
    if (message.method === "item/agentMessage/delta") session.event("assistant.delta", { text: message.params.delta });
    else if (message.method === "item/completed" && message.params.item.type === "agentMessage") session.event("assistant.message", { text: message.params.item.text });
    else if (message.method === "thread/tokenUsage/updated") { session.tokens = message.params.tokenUsage.total; session.tokenObservations = (session.tokenObservations ?? 0) + 1; }
    else if (message.method === "turn/started") session.nativeTurn = message.params.turn.id;
    else if (message.method === "turn/completed") session.completed?.(message.params.turn);
  });
  await rpc("initialize", { clientInfo: { name: "brain", version: "0.163.0" }, capabilities: { experimentalApi: true } });
  native.stdin.write(JSON.stringify({ method: "initialized" }) + "\n");
}

const counters = ["inputTokens", "outputTokens", "cachedInputTokens", "cacheWriteInputTokens", "reasoningOutputTokens"];
class CodexSession extends RunnerSession {
  async open(options) {
    this.options = options; this.tools = new Set(options.tools.map(tool => tool.name));
    await launch(options);
    await mkdir(path.join(options.dir, "work"), { recursive: true });
    const saved = path.join(options.dir, "adapter.jsonl");
    let state;
    try { state = JSON.parse((await readFile(saved, "utf8")).trim().split("\n").at(-1)); }
    catch (error) { if (error.code !== "ENOENT") throw error; }
    const common = { model: options.config.model.slice(options.config.model.indexOf("/") + 1), modelProvider: "brain", cwd: path.join(options.dir, "work"), baseInstructions: options.config.instructions || "You are a helpful assistant. Use the available tools when needed.", developerInstructions: "", approvalPolicy: "never", sandbox: "read-only", config: { "tools.experimental_request_user_input.enabled": false, ...(options.config.reasoning_effort ? { model_reasoning_effort: options.config.reasoning_effort === "off" ? "none" : options.config.reasoning_effort } : {}) } };
    let response;
    if (state) {
      const imported = path.join(process.cwd(), "codex", "imports", options.session_id + ".jsonl"); await mkdir(path.dirname(imported), { recursive: true });
      await copyFile(path.join(options.dir, "rollout.jsonl"), imported);
      response = await rpc("thread/resume", { ...common, threadId: state.thread, path: imported, excludeTurns: true });
      this.tokens = state.tokens;
    } else {
      // Pin the native JSONL history mode: paginated threads depend on a host-local index.
      response = await rpc("thread/start", { ...common, historyMode: "legacy", environments: [], allowProviderModelFallback: false, dynamicTools: options.tools.map(tool => ({ type: "function", name: tool.name, description: tool.description, inputSchema: tool.input_schema, deferLoading: false })) });
      this.tokens = Object.fromEntries(counters.map(key => [key, 0]));
    }
    this.thread = response.thread.id; this.source = response.thread.path;
    if (!this.source) throw new Error("native rollout is unavailable");
    threads.set(this.thread, this);
    return { tools: [...this.tools] };
  }
  async prompt(text) {
    this.interrupted = false;
    this.interruption = undefined;
    const before = this.tokens;
    const beforeObservations = this.tokenObservations ?? 0;
    const request = { request_id: identity(), purpose: "primary", boundary: "native_turn", unmeasured: ["generations", "compaction", "retries"], state: "started", harness: "", provider: "", model: "", attempts: "unobserved", usage: unknownUsage() };
    await this.observe(request);
    const done = new Promise(resolve => { this.completed = resolve; });
    try {
      const result = await rpc("turn/start", { threadId: this.thread, environments: [], input: [{ type: "text", text }] });
      this.nativeTurn = result.turn.id;
      if (this.interrupted) await this.interrupt();
      const turn = await done;
      request.state = turn.status === "completed" ? "completed" : turn.status === "interrupted" ? "cancelled" : "failed";
      if ((this.tokenObservations ?? 0) > beforeObservations && before && this.tokens && counters.every(key => Number.isSafeInteger(this.tokens[key] ?? 0) && (this.tokens[key] ?? 0) >= (before[key] ?? 0))) {
        const delta = key => (this.tokens[key] ?? 0) - (before[key] ?? 0);
        const fields = { inputTokens: "input_tokens", outputTokens: "output_tokens", cachedInputTokens: "cache_read_tokens", cacheWriteInputTokens: "cache_write_tokens", reasoningOutputTokens: "reasoning_tokens" };
        const quantities = Object.fromEntries(counters.filter(key => delta(key) > 0).map(key => [fields[key], delta(key)]));
        if (Object.keys(quantities).length) request.usage = { availability: "partial", semantics: "snapshot", ...quantities, input_cache: "included", output_reasoning: "included" };
      }
      await this.observe(request);
      if (turn.status === "failed") throw Object.assign(new Error(), { code: "provider_error", safeMessage: "Codex model execution failed" });
      return { interrupted: turn.status === "interrupted" };
    } finally { this.completed = undefined; this.nativeTurn = undefined; }
  }
  async interrupt() {
    this.interrupted = true;
    if (this.nativeTurn && !this.interruption) this.interruption = rpc("turn/interrupt", { threadId: this.thread, turnId: this.nativeTurn });
    if (this.interruption) await this.interruption;
    return {};
  }
  async checkpoint() {
    // turn/completed and thread/read flush native state before the append export.
    await rpc("thread/read", { threadId: this.thread, includeTurns: false });
    const destination = path.join(this.options.dir, "rollout.jsonl");
    let copied = 0;
    try { copied = (await stat(destination)).size; } catch (error) { if (error.code !== "ENOENT") throw error; }
    const source = await open(this.source, "r");
    try {
      const size = (await source.stat()).size;
      if (size < copied) throw new Error("native rollout was rewritten");
      const output = await open(destination, "a");
      try {
        const buffer = Buffer.alloc(64 << 10);
        for (let offset = copied; offset < size;) {
          const { bytesRead } = await source.read(buffer, 0, Math.min(buffer.length, size - offset), offset);
          if (!bytesRead) throw new Error("native rollout changed during export");
          const { bytesWritten } = await output.write(buffer.subarray(0, bytesRead));
          if (bytesWritten !== bytesRead) throw new Error("short native checkpoint write");
          offset += bytesRead;
        }
      } finally { await output.close(); }
    } finally { await source.close(); }
    await appendFile(path.join(this.options.dir, "adapter.jsonl"), JSON.stringify({ thread: this.thread, tokens: this.tokens }) + "\n");
    return Promise.all(["rollout.jsonl", "adapter.jsonl"].map(async file => ({ path: file, size: (await stat(path.join(this.options.dir, file))).size, append_only: true })));
  }
  async detach() {
    await rpc("thread/archive", { threadId: this.thread });
    const result = await rpc("thread/read", { threadId: this.thread, includeTurns: false });
    if (result.thread.status.type !== "notLoaded") throw new Error("native thread remained resident");
    threads.delete(this.thread);
    return {};
  }
}
serve(attachment => new CodexSession(attachment));
