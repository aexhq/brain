import { randomBytes } from "node:crypto";
import { mkdir, readdir, stat } from "node:fs/promises";
import path from "node:path";
import {
  createAgentSession, DefaultResourceLoader, ModelRuntime, SessionManager,
  SettingsManager, type AgentSession, type ToolDefinition,
} from "@earendil-works/pi-coding-agent";
import { InMemoryCredentialStore } from "@earendil-works/pi-ai";
import type { OpenOptions, Result, RunnerFrame as Frame } from "./generated.js";
import { observeRequests } from "./evidence.js";

const maxFrame = 4 << 20;

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


class NativeSession {
  session: AgentSession | undefined;
  nativeDir = "";
  busy = false;
  settled: (() => void) | undefined;
  lastError: string | undefined;
  aborted = false;
  brainError: Error | undefined;
  readonly pending = new Map<string, { resolve: (r: Result) => void; reject: (e: Error) => void }>();
  readonly cancelled = new Set<string>();
  readonly observations = new Map<string, { resolve: () => void; reject: (error: Error) => void }>();

  turn = "";
  constructor(readonly attachment: string) {}
  send(value: object) { send({ ...value, attachment: this.attachment, turn: this.turn }); }
  event(type: string, data: unknown) { this.send({ event:type, data }); }
  async callTool(name: string, callID: string, args: unknown, signal?: AbortSignal): Promise<Result> {
    if (this.pending.size >= 32) throw new Error("concurrent tool limit reached");
    signal?.throwIfAborted();
    const id = randomBytes(16).toString("hex");
    let abort: (() => void) | undefined;
    try {
      return await new Promise<Result>((resolve, reject) => {
        this.pending.set(id, { resolve, reject });
        abort = () => {
          this.pending.delete(id); this.cancelled.add(id);
          if (this.cancelled.size > 32) fatal(new Error("unacknowledged cancelled tools"));
          reject(new Error("tool interrupted"));
        };
        signal?.addEventListener("abort", abort, { once: true });
        this.send({ id, method: "tool.call", params: { id: callID, name, arguments: args } });
      });
    } finally {
      if (abort) signal?.removeEventListener("abort", abort);
      this.pending.delete(id);
    }
  }

  async open(options: OpenOptions) {
    if (this.session) throw new Error("session already open");
    this.nativeDir = options.dir;
    const cwd = path.join(this.nativeDir, "work");
    const agentDir = path.join(this.nativeDir, "agent");
    const sessionDir = path.join(this.nativeDir, "session");
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
        const result = await this.callTool(tool.name, callID, args, signal);
        if (result.is_error) {
          throw new Error(result.content.map(c => c.text ?? "").join("\n"));
        }
        return {
          content: result.content.map(c => {
            if (c.type === "text") return { type: "text" as const, text: c.text ?? "" };
            if (c.type === "image" && c.data && c.mime_type) return { type: "image" as const, data: c.data, mimeType: c.mime_type };
            throw new Error("unsupported tool content");
          }),
          details: result.structured_content ?? {},
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
    this.session = created.session;
    observeRequests(this.session, request => new Promise<void>((resolve, reject) => {
      const id = randomBytes(16).toString("hex");
      this.observations.set(id, { resolve, reject });
      this.send({ id, method: "model.request", params: request });
    }));
    if (created.modelFallbackMessage || this.session.model?.provider !== provider || this.session.model?.id !== modelID) {
      throw new Error("harness did not select the requested model");
    }
    if (options.config.reasoning_effort && this.session.thinkingLevel !== options.config.reasoning_effort) {
      throw new Error("requested reasoning effort is unsupported by the model");
    }
    const active = this.session.getActiveToolNames().sort();
    const selected = options.tools.map(t => t.name).sort();
    if (JSON.stringify(active) !== JSON.stringify(selected)) throw new Error("native tool isolation failed");
    this.session.subscribe(e => {
      if (e.type === "message_update" && e.assistantMessageEvent.type === "text_delta") {
        this.event("assistant.delta", { text: e.assistantMessageEvent.delta });
      } else if (e.type === "message_end" && e.message.role === "assistant") {
        const text = e.message.content.filter(c => c.type === "text").map(c => c.text).join("");
        this.event("assistant.message", { text, usage: e.message.usage, stop_reason: e.message.stopReason });
        this.lastError = e.message.stopReason === "error" ? e.message.errorMessage ?? "model provider failed" : undefined;
      } else if (e.type === "agent_settled") {
        this.aborted ||= e.aborted; this.settled?.();
      }
    });
    return { tools: active };
  }

  async dispatch(method: string, params: unknown): Promise<unknown> {
    if (method === "open") return this.open(params as OpenOptions);
    if (method === "detach") { if (this.busy || this.pending.size) throw new Error("cannot detach active session"); this.session?.dispose(); return {}; }
    if (!this.session) throw new Error("session is not open");
    if (method === "interrupt") { this.aborted = true; await this.session.abort(); return {}; }
    if (method === "steer") {
      if (!this.busy) throw new Error("steering requires active execution");
      await this.session.steer((params as { text: string }).text); return {};
    }
    if (method === "checkpoint") {
      if (this.busy || this.pending.size) throw new Error("checkpoint requires settled execution");
      const file = this.session.sessionManager.getSessionFile();
      if (!file) return [];
      let info;
      try { info = await stat(file); } catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ENOENT") return [];
        throw error;
      }
      return [{ path: path.relative(this.nativeDir, file).split(path.sep).join("/"), size: info.size, append_only: true }];
    }
    if (method === "prompt") {
      if (this.busy) throw new Error("session is busy");
      this.busy = true; this.lastError = undefined; this.brainError = undefined; this.aborted = false;
      const completion = new Promise<void>(resolve => { this.settled = resolve; });
      try {
        try { await this.session.prompt((params as { text: string }).text, { expandPromptTemplates: false }); }
        catch (error) { if (!this.aborted) { if (this.lastError) throw new ProviderFailure(this.lastError); throw error; } }
        await completion;
        if (this.brainError) throw this.brainError;
        if (this.aborted) return { interrupted: true };
        if (this.lastError) throw new ProviderFailure(this.lastError);
        return {};
      } finally { this.busy = false; this.settled = undefined; }
    }
    throw new Error("unsupported runner operation");
  }

  async receive(frame: Frame) {
    if (!frame.id) fatal(new Error("missing frame identity"));
    if (!frame.method) {
      const observation = this.observations.get(frame.id);
      if (observation) {
        if (frame.turn !== this.turn) fatal(new Error("stale model observation response"));
        this.observations.delete(frame.id);
        if (frame.error) {
          this.brainError = new Error("model observation failed"); observation.reject(this.brainError); void this.session?.abort();
        } else observation.resolve();
        return;
      }
      if (this.cancelled.delete(frame.id)) return;
      if (frame.turn !== this.turn) fatal(new Error("stale tool response"));
      const callback = this.pending.get(frame.id);
      if (!callback || (!frame.result && !frame.error)) fatal(new Error("unexpected tool response"));
      if (frame.error) {
        this.pending.delete(frame.id);
        this.brainError = new Error("tool dispatch failed");
        callback.reject(this.brainError);
        await this.session?.abort();
        return;
      }
      this.pending.delete(frame.id); callback.resolve(frame.result as Result); return;
    }
    try {
      if (frame.method === "prompt") { if (this.busy || !frame.turn) throw new Error("invalid active turn"); this.turn = frame.turn; }
      const result = await this.dispatch(frame.method, frame.params);
      send({ attachment: this.attachment, turn: frame.turn, id: frame.id, result });
      if (frame.method === "prompt") this.turn = "";
      if (frame.method === "detach") sessions.delete(this.attachment);
   }
    catch (error) {
      send({ attachment: this.attachment, turn: frame.turn, id: frame.id, error: {
        code: error instanceof ProviderFailure ? "provider_error" : "brain_error",
        message: error instanceof Error ? error.message : "runner operation failed",
      } });
    }
  }

}

class ProviderFailure extends Error {}

const sessions = new Map<string, NativeSession>();
async function receive(frame: Frame) {
  if (!frame.attachment) fatal(new Error("missing session attachment"));
  let state = sessions.get(frame.attachment);
  if (frame.method === "open") {
    if (state) fatal(new Error("duplicate session attachment"));
    state = new NativeSession(frame.attachment); sessions.set(frame.attachment,state);
  }
  if (!state) fatal(new Error("unknown session attachment"));
  await state.receive(frame);
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
process.stdin.on("end", () => { for (const state of sessions.values()) state.session?.dispose(); process.exit(0); });
process.stdin.on("error", fatal);
process.stdout.on("error", fatal);
process.on("unhandledRejection", fatal);

