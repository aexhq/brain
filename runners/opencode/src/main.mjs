import { mkdir, readFile, appendFile, stat } from "node:fs/promises";
import path from "node:path";
import { OpenCode } from "@opencode/sdk";
import { Model, Provider } from "@opencode/plugin";
import { SessionRestart } from "@opencode/core/session/execution/restart";
import { Effect, Layer } from "effect";
import { RunnerSession, serve, identity, unknownUsage } from "../../common/bridge.mjs";

let boot;
const attachments = new Map();
const deny = { action: "*", resource: "*", effect: "deny" };
const usage = value => {
  if (!value) return unknownUsage();
  const fields = { input_tokens: value.input_tokens, output_tokens: value.output_tokens, cache_read_tokens: value.input_tokens_details?.cached_tokens, reasoning_tokens: value.output_tokens_details?.reasoning_tokens };
  const quantities = Object.fromEntries(Object.entries(fields).filter(([, count]) => count !== undefined));
  return { availability: value.input_tokens !== undefined && value.output_tokens !== undefined ? "reported" : "partial", semantics: "snapshot", ...quantities, input_cache: "included", output_reasoning: "included" };
};
function launch() {
  return boot ??= (async () => {
    const home = path.join(process.cwd(), "opencode");
    await mkdir(path.join(home, "work"), { recursive: true });
    return OpenCode.create({
      database: { path: path.join(home, "native.sqlite") },
      config: { directory: path.join(home, "config"), project: false, claude: false, content: "{}" },
      models: { fetch: false, snapshot: false }, fs: { filewatcher: false, fff: false },
      log: { emit() {} },
      instances: {
        key: session => session.metadata.brain,
        configure: key => ({ plugins: [attachments.get(key).plugin()] }),
      },
    }, { overrides: [SessionRestart.node.replace(Layer.succeed(SessionRestart.Service, { resumeSuspendedSessions: Effect.void }))] });
  })();
}

class OpenCodeSession extends RunnerSession {
  snapshots = new Map();
  plugin() {
    const owner = this, options = this.options;
    return { id: "brain", async setup(ctx) {
      const providerID = Provider.ID.make("brain"), modelID = Model.ID.make(options.config.model.split("/").slice(1).join("/"));
      await ctx.provider.transform(editor => {
        for (const entry of editor.list()) editor.remove(entry.provider.id);
        editor.add({ info: { ...Provider.Info.empty(providerID), activation: "enabled", package: "@opencode/ai/providers/openai", settings: { apiKey: "session-scoped", baseURL: options.provider.base_url || "https://api.openai.com/v1", transport: "http", compaction: { type: "summary" } } }, models: [Model.Info.default(providerID, modelID)] });
      });
      await ctx.agent.transform(editor => {
        for (const agent of editor.list()) {
          if (agent.id !== "build") editor.remove(agent.id);
          else editor.update(agent.id, value => { value.system = options.config.instructions ?? ""; value.permissions = [deny]; });
        }
        editor.default("build");
      });
      await ctx.tool.transform(editor => {
        for (const tool of editor.list()) editor.remove(tool.id);
        for (const tool of options.tools) editor.add({ name: tool.name, description: tool.description, input: tool.input_schema, options: { codemode: false }, async execute(input, context) {
          const result = await owner.tool({ id: context.id, name: tool.name, arguments: input });
          if (result.is_error) throw new Error(result.content.map(part => part.text ?? "").join("\n"));
          return { content: result.content.map(part => part.type === "image" ? { type: "file", uri: `data:${part.mime_type};base64,${part.data}`, mime: part.mime_type } : { type: "text", text: part.text ?? "" }) };
        } });
      });
      await ctx.session.hook("title", event => { event.result = "Brain session"; });
      await ctx.session.hook("context", event => { event.system = [{ type: "text", text: options.config.instructions ?? "" }]; });
      await ctx.session.hook("model.request", async event => {
        if (event.sessionID !== owner.nativeID || !owner.turn) throw new Error("unsolicited native inference");
        await owner.finish();
        owner.requestEvidence = { request_id: identity(), purpose: event.kind === "compaction" ? "compaction" : event.kind === "primary" ? "primary" : "other", boundary: "request", state: "started", harness: "", provider: "", model: "", attempts: "observed", usage: unknownUsage() };
        await owner.observe(owner.requestEvidence);
        event.headers.Authorization = `Bearer ${options.provider.api_key}`;
      });
      await ctx.session.hook("http.request", async event => {
        if (owner.attempt && owner.attempt.state === "started") await owner.finishAttempt("unknown");
        owner.attempt = { ...owner.requestEvidence, attempt_id: identity(), state: "started", usage: unknownUsage() };
        await owner.observe(owner.attempt);
        // The hook is the final native dispatch boundary; no credentials enter native state.
        event.request.headers.set("Authorization", `Bearer ${options.provider.api_key}`);
      });
      await ctx.session.hook("http.response", async event => {
        const response = event.response, attempt = owner.attempt;
        const providerID = response.headers.get("x-request-id");
        if (providerID) attempt.provider_request_id = providerID;
        if (!response.ok) { await owner.finishAttempt("failed"); return; }
        if (response.headers.get("content-type")?.includes("application/json")) {
          const data = await response.json();
          attempt.usage = usage(data.usage);
          await owner.finishAttempt(data.status === "completed" ? "completed" : "unknown");
          event.response = new Response(JSON.stringify(data), { status: response.status, headers: response.headers });
          return;
        }
        const reader = response.body.getReader(), decoder = new TextDecoder();
        let buffer = "", completed = false;
        const body = new ReadableStream({
          async pull(controller) {
            try {
              const chunk = await reader.read();
              if (chunk.done) { await owner.finishAttempt(completed ? "completed" : "unknown"); controller.close(); return; }
              buffer += decoder.decode(chunk.value, { stream: true });
              let end;
              while ((end = buffer.indexOf("\n")) >= 0) {
                const line = buffer.slice(0, end).trim(); buffer = buffer.slice(end + 1);
                if (line.startsWith("data:") && line.slice(5).trim() !== "[DONE]") {
                  const data = JSON.parse(line.slice(5));
                  if (data.type === "response.completed") { completed = true; attempt.usage = usage(data.response?.usage); }
                  if (data.type === "response.output_text.delta") owner.event("assistant.delta", { text: data.delta });
                }
              }
              // A malformed provider stream cannot grow the parser without bound.
              if (buffer.length > 4 << 20) throw new Error("provider frame too large");
              controller.enqueue(chunk.value);
            } catch (error) { await owner.finishAttempt(owner.cancelled ? "cancelled" : "failed"); controller.error(error); }
          },
          async cancel(reason) { await reader.cancel(reason); await owner.finishAttempt(completed ? "completed" : owner.cancelled ? "cancelled" : "unknown"); },
        });
        event.response = new Response(body, { status: response.status, headers: response.headers });
      });
      // This adapter selects the native HTTP route explicitly. No WebSocket fallback is enabled.
      await ctx.session.hook("experimental.ws.handshake", () => { throw new Error("WebSocket transport is not enabled"); });
    } };
  }
  async finishAttempt(state) {
    if (this.attempt?.state !== "started") return;
    this.attempt.state = state;
    await this.observe(this.attempt);
  }
  async finish(state) {
    if (!this.requestEvidence) return;
    await this.finishAttempt(state === "cancelled" ? "cancelled" : "unknown");
    this.requestEvidence.state = state ?? this.attempt?.state ?? "unknown";
    this.requestEvidence.usage = this.attempt?.usage ?? unknownUsage();
    await this.observe(this.requestEvidence);
    this.requestEvidence = undefined; this.attempt = undefined;
  }
  async open(options) {
    this.options = options; attachments.set(options.session_id, this);
    this.host = await launch();
    this.location = { directory: path.join(process.cwd(), "opencode", "work") };
    let info;
    try {
      for (const line of (await readFile(path.join(options.dir, "session.jsonl"), "utf8")).trim().split("\n")) {
        const entry = JSON.parse(line);
        if (entry.info) info = entry.info;
        if (entry.message) this.snapshots.set(entry.message.id, JSON.stringify(entry.message));
      }
    } catch (error) { if (error.code !== "ENOENT") throw error; }
    if (info) {
      info.metadata = { brain: options.session_id };
      const imported = await this.host.sessions.import({ info, messages: [...this.snapshots.values()].map(JSON.parse), location: this.location });
      this.nativeID = imported.id;
    } else {
      const created = await this.host.sessions.create({ title: "Brain session", agent: "build", model: { providerID: "brain", id: options.config.model.split("/").slice(1).join("/") }, location: this.location, metadata: { brain: options.session_id }, permissions: [deny, ...options.tools.map(tool => ({ action: tool.name, resource: "*", effect: "allow" })), { action: "execute", resource: "*", effect: "deny" }] });
      this.nativeID = created.id;
    }
    return { tools: options.tools.map(tool => tool.name) };
  }
  async prompt(text) {
    this.cancelled = false;
    await this.host.sessions.prompt({ sessionID: this.nativeID, text });
    await this.host.sessions.wait({ sessionID: this.nativeID });
    const info = await this.host.sessions.get({ sessionID: this.nativeID });
    await this.finish(info.outcome === "interrupted" ? "cancelled" : undefined);
    for (const message of await this.host.sessions.context({ sessionID: this.nativeID })) {
      if (message.type === "assistant" && !this.snapshots.has(message.id)) {
        const text = message.content.filter(part => part.type === "text").map(part => part.text).join("");
        if (text) this.event("assistant.message", { text });
      }
    }
    if (info.outcome === "failed") throw Object.assign(new Error(), { code: "provider_error", safeMessage: "OpenCode model execution failed" });
    return { interrupted: info.outcome === "interrupted" };
  }
  async interrupt() { this.cancelled = true; await this.host.sessions.interrupt({ sessionID: this.nativeID, resume: false }); return {}; }
  async checkpoint() {
    const exported = await this.host.sessions.export({ sessionID: this.nativeID, sanitize: false });
    const entries = [];
    for (const message of exported.messages) {
      const encoded = JSON.stringify(message);
      if (this.snapshots.get(message.id) !== encoded) { entries.push(JSON.stringify({ message })); this.snapshots.set(message.id, encoded); }
    }
    entries.push(JSON.stringify({ info: exported.info }));
    const file = path.join(this.options.dir, "session.jsonl");
    await appendFile(file, entries.join("\n") + "\n");
    return [{ path: "session.jsonl", size: (await stat(file)).size, append_only: true }];
  }
  async detach() { await this.host.sessions.remove({ sessionID: this.nativeID }); attachments.delete(this.options.session_id); return {}; }
}
serve(attachment => new OpenCodeSession(attachment));
