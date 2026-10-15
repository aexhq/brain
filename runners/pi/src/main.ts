import { randomBytes } from "node:crypto";
import { mkdir, readdir, stat } from "node:fs/promises";
import path from "node:path";
import {
  createAgentSession, DefaultResourceLoader, ModelRuntime, SessionManager,
  SettingsManager, type AgentSession, type ToolDefinition,
} from "@earendil-works/pi-coding-agent";
import { InMemoryCredentialStore } from "@earendil-works/pi-ai";
import type { OpenOptions, Result, RunnerFrame as Frame } from "./generated.js";

const maxFrame = 4 << 20;
let session: AgentSession | undefined;
let nativeDir = "";
let busy = false;
let settled: (() => void) | undefined;
let lastError: string | undefined;
let aborted = false;
const pending = new Map<string, { resolve: (r: Result) => void; reject: (e: Error) => void }>();
const cancelled = new Set<string>();

function fatal(error: unknown): never {
  // Do not print request bodies, credentials, or provider payloads.
  process.stderr.write(`Brain Pi runner stopped: ${error instanceof Error ? error.name : "protocol failure"}\n`);
  process.exit(1);
}
function send(value: unknown) {
  const line = JSON.stringify(value) + "\n";
  if (Buffer.byteLength(line) > maxFrame || process.stdout.writableLength > 2 * maxFrame) {
    fatal(new Error("runner output limit exceeded"));
  }
  process.stdout.write(line);
}
function event(type: string, data: unknown) { send({ event: type, data }); }

async function callTool(name: string, callID: string, args: unknown, signal?: AbortSignal): Promise<Result> {
  if (pending.size >= 32) throw new Error("concurrent tool limit reached");
  signal?.throwIfAborted();
  const id = randomBytes(16).toString("hex");
  let abort: (() => void) | undefined;
  try {
    return await new Promise<Result>((resolve, reject) => {
      pending.set(id, { resolve, reject });
      abort = () => {
        pending.delete(id); cancelled.add(id);
        if (cancelled.size > 32) fatal(new Error("unacknowledged cancelled tools"));
        reject(new Error("tool interrupted"));
      };
      signal?.addEventListener("abort", abort, { once: true });
      send({ id, method: "tool.call", params: { id: callID, name, arguments: args } });
    });
  } finally {
    if (abort) signal?.removeEventListener("abort", abort);
    pending.delete(id);
  }
}

async function open(options: OpenOptions) {
  if (session) throw new Error("session already open");
  nativeDir = options.dir;
  const cwd = path.join(nativeDir, "work");
  const agentDir = path.join(nativeDir, "agent");
  const sessionDir = path.join(nativeDir, "session");
  await Promise.all([cwd, agentDir, sessionDir].map(p => mkdir(p, { recursive: true })));
  const settings = SettingsManager.inMemory({
    cacheWarming: "off", extensions: [], skills: [], prompts: [], themes: [],
  });
  const instructions = options.config.instructions ?? "You are a helpful assistant. Use the available tools when needed.";
  const resources = new DefaultResourceLoader({
    cwd, agentDir, settingsManager: settings,
    noExtensions: true, noSkills: true, noPromptTemplates: true, noThemes: true,
    noContextFiles: true,
    systemPrompt: instructions,
    // Pi otherwise appends its local cwd even with a custom prompt. A private
    // runner path has no meaning for remote tools and changes on relocation.
    extensionFactories: [pi => {
      pi.on("before_agent_start", () => ({ systemPrompt: instructions }));
    }],
  });
  await resources.reload();
  const runtime = await ModelRuntime.create({
    credentials: new InMemoryCredentialStore(), modelsPath: null,
    modelsStorePath: path.join(agentDir, "models-cache.json"),
    allowModelNetwork: false, refreshOnCreate: false,
  });
  const separator = options.config.model.indexOf("/");
  if (separator < 1) throw new Error("model must be provider/model");
  const provider = options.config.model.slice(0, separator);
  const modelID = options.config.model.slice(separator + 1);
  const initialModel = runtime.getModel(provider, modelID);
  if (!initialModel) throw new Error(`unknown model: ${options.config.model}`);
  if (options.provider.base_url || options.provider.api) {
    runtime.registerProvider(provider, {
      ...(options.provider.base_url ? { baseUrl: options.provider.base_url } : {}),
      ...(options.provider.api ? { api: options.provider.api } : {}),
      models: [{
        ...initialModel,
        ...(options.provider.base_url ? { baseUrl: options.provider.base_url } : {}),
        ...(options.provider.api ? { api: options.provider.api } : {}),
      }],
    });
  }
  if (options.provider.api_key) await runtime.setRuntimeApiKey(provider, options.provider.api_key);
  const model = runtime.getModel(provider, modelID);
  if (!model) throw new Error(`unknown model: ${options.config.model}`);
  const files = (await readdir(sessionDir)).filter(f => f.endsWith(".jsonl"));
  if (files.length > 1) throw new Error("ambiguous native session files");
  const manager = files[0]
    ? SessionManager.open(path.join(sessionDir, files[0]), sessionDir, cwd)
    : SessionManager.create(cwd, sessionDir);
  const customTools: ToolDefinition[] = options.tools.map(tool => ({
    name: tool.name, label: tool.name, description: tool.description,
    parameters: tool.input_schema as ToolDefinition["parameters"],
    execute: async (callID, args, signal) => {
      const result = await callTool(tool.name, callID, args, signal);
      if (result.is_error) {
        throw new Error(result.content.map(c => c.text ?? "").join("\n"));
      }
      return {
        content: result.content.map(c => {
          if (c.type === "text") return { type: "text" as const, text: c.text ?? "" };
          if (c.type === "image" && c.data && c.mime_type) return { type: "image" as const, data: c.data, mimeType: c.mime_type };
          throw new Error("unsupported tool content");
        }),
        details: {},
      };
    },
  }));
  type CreateOptions = NonNullable<Parameters<typeof createAgentSession>[0]>;
  const created = await createAgentSession({
    cwd, agentDir, modelRuntime: runtime, model,
    thinkingLevel: (options.config.reasoning_effort ?? "off") as CreateOptions["thinkingLevel"],
    sessionManager: manager, settingsManager: settings, resourceLoader: resources,
    noTools: "builtin", tools: options.tools.map(t => t.name), customTools,
  });
  session = created.session;
  if (created.modelFallbackMessage || session.model?.provider !== provider || session.model?.id !== modelID) {
    throw new Error("harness did not select the requested model");
  }
  if (options.config.reasoning_effort && session.thinkingLevel !== options.config.reasoning_effort) {
    throw new Error("requested reasoning effort is unsupported by the model");
  }
  const active = session.getActiveToolNames().sort();
  const selected = options.tools.map(t => t.name).sort();
  if (JSON.stringify(active) !== JSON.stringify(selected)) throw new Error("native tool isolation failed");
  session.subscribe(e => {
    if (e.type === "message_update" && e.assistantMessageEvent.type === "text_delta") {
      event("assistant.delta", { text: e.assistantMessageEvent.delta });
    } else if (e.type === "message_end" && e.message.role === "assistant") {
      const text = e.message.content.filter(c => c.type === "text").map(c => c.text).join("");
      event("assistant.message", { text, usage: e.message.usage, stop_reason: e.message.stopReason });
      lastError = e.message.stopReason === "error" ? e.message.errorMessage ?? "model provider failed" : undefined;
    } else if (e.type === "agent_settled") {
      aborted ||= e.aborted; settled?.();
    }
  });
  return { tools: active };
}

async function dispatch(method: string, params: unknown): Promise<unknown> {
  if (method === "open") return open(params as OpenOptions);
  if (!session) throw new Error("session is not open");
  if (method === "interrupt") { aborted = true; await session.abort(); return {}; }
  if (method === "steer") {
    if (!busy) throw new Error("steering requires active execution");
    await session.steer((params as { text: string }).text); return {};
  }
  if (method === "checkpoint") {
    if (busy || pending.size) throw new Error("checkpoint requires settled execution");
    const file = session.sessionManager.getSessionFile();
    if (!file) return [];
    let info;
    try { info = await stat(file); } catch (error) {
      if ((error as NodeJS.ErrnoException).code === "ENOENT") return [];
      throw error;
    }
    return [{ path: path.relative(nativeDir, file).split(path.sep).join("/"), size: info.size, append_only: true }];
  }
  if (method === "prompt") {
    if (busy) throw new Error("session is busy");
    busy = true; lastError = undefined; aborted = false;
    const completion = new Promise<void>(resolve => { settled = resolve; });
    try {
      try { await session.prompt((params as { text: string }).text, { expandPromptTemplates: false }); }
      catch (error) { if (!aborted) { if (lastError) throw new ProviderFailure(lastError); throw error; } }
      await completion;
      if (aborted) return { interrupted: true };
      if (lastError) throw new ProviderFailure(lastError);
      return {};
    } finally { busy = false; settled = undefined; }
  }
  throw new Error("unsupported runner operation");
}

class ProviderFailure extends Error {}

async function receive(frame: Frame) {
  if (!frame.id) fatal(new Error("missing frame identity"));
  if (!frame.method) {
    if (cancelled.delete(frame.id)) return;
    const callback = pending.get(frame.id);
    if (!callback || !frame.result || frame.error) fatal(new Error("unexpected tool response"));
    pending.delete(frame.id); callback.resolve(frame.result as Result); return;
  }
  try { send({ id: frame.id, result: await dispatch(frame.method, frame.params) }); }
  catch (error) {
    send({ id: frame.id, error: {
      code: error instanceof ProviderFailure ? "provider_error" : "brain_error",
      message: error instanceof Error ? error.message : "runner operation failed",
    } });
  }
}

let buffer = Buffer.alloc(0);
process.stdin.on("data", (chunk: Buffer) => {
  buffer = Buffer.concat([buffer, chunk]);
  let end: number;
  while ((end = buffer.indexOf(10)) >= 0) {
    if (end > maxFrame) fatal(new Error("runner frame too large"));
    const line = buffer.subarray(0, end); buffer = buffer.subarray(end + 1);
    try { void receive(JSON.parse(line.toString("utf8")) as Frame).catch(fatal); } catch (error) { fatal(error); }
  }
  if (buffer.length > maxFrame) fatal(new Error("runner frame too large"));
});
process.stdin.on("end", () => { session?.dispose(); process.exit(0); });
process.stdin.on("error", fatal);
process.stdout.on("error", fatal);
process.on("unhandledRejection", fatal);

