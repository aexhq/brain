import { RuntimeExecutor, type RuntimeTool, type RuntimeTransport } from "./runtime.js";
import { lower, type SessionOptions } from "./authoring.js";
export { ApiKey, CredentialReference, Model, Tool, ManagedTool } from "./authoring.js";
export { connectTab } from "./browser.js";
export type { SessionOptions, ManagedSelection } from "./authoring.js";
export type { RuntimeTool, RuntimeContext, RuntimeTransport } from "./runtime.js";
import { routes, type CreateSession, type Session, type Input, type Event, type Tool as ToolDescriptor, type SyncStatus } from "./generated.js";
export type * from "./generated.js";

export class BrainError extends Error {
  constructor(readonly status: number, readonly code: string, message: string, readonly requestId?: string) { super(message); }
}

export type SessionInput = (CreateSession & { runtime_tools?: RuntimeTool[]; runtime_transport?: RuntimeTransport }) | SessionOptions;

export class Brain {
  private executors = new Map<string, RuntimeExecutor>();
  constructor(readonly url: string, private readonly token: string, private readonly requestFetch: typeof fetch = (...args) => globalThis.fetch(...args)) {}
  private async request(operation: keyof typeof routes, id?: string, body?: unknown, query?: URLSearchParams, signal?: AbortSignal): Promise<Response> {
    const route = routes[operation];
    const target = new URL(route.path.replace("{id}", encodeURIComponent(id ?? "")), this.url);
    if (query) target.search = query.toString();
    const response = await this.requestFetch(target, {
      method: route.method, headers: { Authorization: `Bearer ${this.token}`, "Content-Type": "application/json" },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal: signal ?? AbortSignal.timeout(30000), redirect: "error",
    });
    if (!response.ok) await this.fail(response);
    return response;
  }
  private async fail(response: Response): Promise<never> {
    const body = await response.json() as { error?: { code?: string; message?: string }; request_id?: string };
    throw new BrainError(response.status, body.error?.code ?? "http_error", body.error?.message ?? response.statusText, body.request_id);
  }
  async create(input: SessionInput, signal?: AbortSignal): Promise<Session> {
    const normalized: CreateSession & { runtime_tools?: RuntimeTool[]; runtime_transport?: RuntimeTransport } = "config" in input ? input : (() => { const { wire, runtime } = lower(input); return { ...wire, runtime_tools: runtime }; })();
    const { runtime_tools, runtime_transport, ...wire } = normalized;
    let executor: RuntimeExecutor | undefined;
    try {
      if (runtime_tools?.length) {
        executor = await RuntimeExecutor.open(this.url, this.token, runtime_tools, runtime_transport ?? "sse", this.requestFetch);
        wire.config = { ...wire.config, bindings: [...(wire.config.bindings ?? []), { alias: "runtime", executor: executor.credential.id, runtime: runtime_tools.map(({ run: _, ...descriptor }) => descriptor) }] };
      }
      const session = await (await this.request("create", undefined, wire, undefined, signal)).json() as Session;
      if (executor) this.executors.set(session.id, executor);
      return session;
    } catch (error) { if (executor) await executor.close(); throw error; }
  }
  async rebind(id: string, tools: RuntimeTool[], transport: RuntimeTransport = "sse"): Promise<void> {
    await this.executors.get(id)?.close(); this.executors.delete(id);
    this.executors.set(id, await RuntimeExecutor.open(this.url, this.token, tools, transport, this.requestFetch, id));
  }
  async dispose(): Promise<void> {
    const executors = [...this.executors.values()]; this.executors.clear();
    const results = await Promise.allSettled(executors.map(e => e.close()));
    const errors = results.filter((r): r is PromiseRejectedResult => r.status === "rejected");
    if (errors.length) throw new AggregateError(errors.map(e => e.reason), "executor shutdown failed");
  }
  async get(id: string, signal?: AbortSignal): Promise<Session> { return (await this.request("get", id, undefined, undefined, signal)).json(); }
  async list(after = "", limit = 100): Promise<{ sessions: Session[] }> { return (await this.request("list", undefined, undefined, new URLSearchParams({ after, limit: String(limit) }))).json(); }
  async send(id: string, input: Input, signal?: AbortSignal): Promise<Event> { return (await this.request("send", id, input, undefined, signal)).json(); }
  async interrupt(id: string): Promise<void> { await this.request("send", id, { type: "user.interrupt" }); }
  async resume(id: string): Promise<Session> { return (await this.request("resume", id)).json(); }
  async close(id: string): Promise<void> { await this.request("close", id); const executor = this.executors.get(id); this.executors.delete(id); await executor?.close(); }
  async flush(id: string, signal = AbortSignal.timeout(60000)): Promise<SyncStatus> { return (await this.request("flush", id, undefined, undefined, signal)).json(); }
  async sync(id: string): Promise<SyncStatus> { return (await this.request("sync", id)).json(); }
  async restore(id: string, checkpoint: string, signal = AbortSignal.timeout(60000)): Promise<Session> { return (await this.request("restore", id, { checkpoint }, undefined, signal)).json(); }
  async catalog(): Promise<{ tools: ToolDescriptor[] }> { return (await this.request("catalog")).json(); }
  async history(id: string, after = "", limit = 100): Promise<{ events: Event[]; next_cursor: string }> {
    return (await this.request("events", id, undefined, new URLSearchParams({ after, limit: String(limit) }))).json();
  }
  async *observe(id: string, options: { after?: string; signal?: AbortSignal } = {}): AsyncGenerator<Event> {
    const target = new URL(routes.events.path.replace("{id}", encodeURIComponent(id)), this.url);
    if (options.after) target.searchParams.set("after", options.after);
    const response = await this.requestFetch(target, { headers: { Authorization: `Bearer ${this.token}`, Accept: "text/event-stream" }, signal: options.signal, redirect: "error" });
    if (!response.ok) await this.fail(response);
    if (!response.body) throw new Error("missing event stream");
    const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    let buffer = "";
    let data: string[] = [];
    let size = 0;
    try {
      while (true) {
        const next = await reader.read();
        if (next.done) { if (buffer || data.length) throw new Error("incomplete event stream"); return; }
        buffer += next.value;
        if (buffer.length + size > 4 << 20) throw new Error("event exceeds client limit");
        let end: number;
        while ((end = buffer.indexOf("\n")) >= 0) {
          const line = buffer.slice(0, end).replace(/\r$/, ""); buffer = buffer.slice(end + 1);
          if (line === "") {
            if (data.length) yield JSON.parse(data.join("\n")) as Event;
            data = []; size = 0;
          } else if (line.startsWith("data:")) {
            const value = line.slice(5).replace(/^ /, ""); data.push(value); size += value.length;
          }
        }
      }
    } finally { await reader.cancel(); reader.releaseLock(); }
  }
}
