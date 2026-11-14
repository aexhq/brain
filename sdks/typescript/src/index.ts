import createClient, { type Client } from "openapi-fetch";
import type { paths, Status } from "./generated.js";
import { BrowserExecutor, type BrowserTool, type BrowserTransport } from "./executor.js";
import { instrumentRequest } from './telemetry.js';
export { configureTelemetry, telemetry, instrumentRequest } from './telemetry.js';
import { lower, type SessionOptions } from "./authoring.js";
export { ApiKey, CredentialReference, Model, Tool, ManagedTool } from "./authoring.js";
export { Pi, Codex, OpenCode, PiWebAccess, PiSubagents, PiMcp } from './harnesses.js';
export type { Harness, HarnessOptions, PiOptions, PiPlugin, PiPluginOptions } from './harnesses.js';
export { AwsS3, Modal, E2B, Daytona } from './integrations.js';
export type { AwsS3Options, ModalOptions, E2BOptions, DaytonaOptions } from './integrations.js';
export { connectTab } from "./browser.js";
export type { SessionOptions, ManagedSelection } from "./authoring.js";
export type { BrowserTool, BrowserContext, BrowserTransport } from "./executor.js";
import { routes, type CreateSession, type Session, type Input, type Event, type Tool as ToolDescriptor, type SyncStatus } from "./generated.js";
export type * from "./generated.js";

export class BrainError extends Error {
  constructor(readonly status: number, readonly code: string, message: string, readonly requestId?: string) { super(message); }
}

export type SessionInput = (Omit<CreateSession, "id"> & { id?: string; browser_tools?: BrowserTool[]; browser_transport?: BrowserTransport }) | SessionOptions;

export class Brain {
  private executors = new Map<string, BrowserExecutor>();
  private readonly http: Client<paths, "application/json">;
  constructor(readonly url: string, private readonly token: string, private readonly requestFetch: typeof fetch = (...args) => globalThis.fetch(...args)) {
    this.http = createClient<paths, "application/json">({
      baseUrl: new URL("/", url).href, fetch: request => this.requestFetch(request),
      headers: { Authorization: `Bearer ${token}` }, redirect: "error",
    });
    this.http.use({ onResponse: async ({ response }) => { if (!response.ok) await this.fail(response); } });
  }
  private request<T>(operation: string, action: (options: { headers: Record<string, string>; signal: AbortSignal }) => Promise<{ data?: T }>, signal?: AbortSignal): Promise<T> {
    return instrumentRequest(operation, async headers => {
      const { data } = await action({ headers, signal: signal ?? AbortSignal.timeout(30000) });
      if (data === undefined) throw new Error("missing response body");
      return data;
    });
  }
  private async fail(response: Response): Promise<never> {
    const body = await response.json() as { error?: { code?: string; message?: string }; request_id?: string };
    throw new BrainError(response.status, body.error?.code ?? "http_error", body.error?.message ?? response.statusText, body.request_id);
  }
  async create(input: SessionInput, signal?: AbortSignal): Promise<Session> {
    const normalized: CreateSession & { browser_tools?: BrowserTool[]; browser_transport?: BrowserTransport } = "config" in input ? { ...input, id: input.id ?? crypto.randomUUID() } : (() => { const { wire, runtime } = lower(input); return { ...wire, browser_tools: runtime }; })();
    const { browser_tools, browser_transport, ...wire } = normalized;
    let executor: BrowserExecutor | undefined;
    try {
      if (browser_tools?.length) {
        executor = await BrowserExecutor.open(this.url, this.token, browser_tools, browser_transport ?? "sse", this.requestFetch);
        wire.config = { ...wire.config, bindings: [...(wire.config.bindings ?? []), { alias: "runtime", executor: executor.credential.id, runtime: browser_tools.map(({ run: _, ...descriptor }) => descriptor) }] };
      }
      const session = await this.request("create", options => this.http.POST("/sessions", { ...options, body: wire }), signal);
      if (executor) this.executors.set(session.id, executor);
      return session;
    } catch (error) { if (executor) await executor.close(); throw error; }
  }
  async rebind(id: string, tools: BrowserTool[], transport: BrowserTransport = "sse"): Promise<void> {
    await this.executors.get(id)?.close(); this.executors.delete(id);
    this.executors.set(id, await BrowserExecutor.open(this.url, this.token, tools, transport, this.requestFetch, id));
  }
  async dispose(): Promise<void> {
    const executors = [...this.executors.values()]; this.executors.clear();
    const results = await Promise.allSettled(executors.map(e => e.close()));
    const errors = results.filter((r): r is PromiseRejectedResult => r.status === "rejected");
    if (errors.length) throw new AggregateError(errors.map(e => e.reason), "executor shutdown failed");
  }
  async get(id: string, signal?: AbortSignal): Promise<Session> { return this.request("get", options => this.http.GET("/sessions/{id}", { ...options, params: { path: { id } } }), signal); }
  async list(after = "", limit = 100) { return this.request("list", options => this.http.GET("/sessions", { ...options, params: { query: { after, limit } } })); }
  async send(id: string, input: Input, signal?: AbortSignal): Promise<Event | Status> { return this.request("send", options => this.http.POST("/sessions/{id}/events", { ...options, params: { path: { id } }, body: input }), signal); }
  async interrupt(id: string): Promise<void> { await this.send(id, { type: "user.interrupt" }); }
  async resume(id: string): Promise<Session> { return this.request("resume", options => this.http.POST("/sessions/{id}/resume", { ...options, params: { path: { id } } })); }
  async close(id: string): Promise<void> { await this.request("close", options => this.http.POST("/sessions/{id}/close", { ...options, params: { path: { id } } })); const executor = this.executors.get(id); this.executors.delete(id); await executor?.close(); }
  async delete(id: string, signal?: AbortSignal): Promise<void> { await this.request("delete", options => this.http.DELETE("/sessions/{id}", { ...options, params: { path: { id } } }), signal); const executor = this.executors.get(id); this.executors.delete(id); await executor?.close(); }
  async flush(id: string, signal = AbortSignal.timeout(60000)): Promise<SyncStatus> { return this.request("flush", options => this.http.POST("/sessions/{id}/flush", { ...options, params: { path: { id } } }), signal); }
  async sync(id: string): Promise<SyncStatus> { return this.request("sync", options => this.http.GET("/sessions/{id}/sync", { ...options, params: { path: { id } } })); }
  async restore(id: string, checkpoint: string, signal = AbortSignal.timeout(60000)): Promise<Session> { return this.request("restore", options => this.http.POST("/sessions/{id}/restore", { ...options, params: { path: { id } }, body: { checkpoint } }), signal); }
  async catalog() { return this.request("catalog", options => this.http.GET("/catalog", options)); }
  async uploadTool(tool: ToolDescriptor, module: string, signal?: AbortSignal): Promise<{ reference: string }> {
    return instrumentRequest('uploadTool', async headers => {
      const response = await this.requestFetch(new URL('/tools/packages', this.url), { method: 'POST', headers: { ...headers, Authorization: `Bearer ${this.token}`, 'Content-Type': 'application/json' }, body: JSON.stringify({ tool, module }), signal: signal ?? AbortSignal.timeout(60000), redirect: 'error' });
      if (!response.ok) await this.fail(response);
      return response.json();
    });
  }
  async uploadAttachment(id: string, name: string, body: Blob, signal?: AbortSignal): Promise<import('./generated.js').Attachment> {
    const target = new URL(`/sessions/${encodeURIComponent(id)}/attachments`, this.url); target.searchParams.set('name', name);
    return instrumentRequest('uploadAttachment', async headers => {
      const response = await this.requestFetch(target, { method: 'POST', headers: { ...headers, Authorization: `Bearer ${this.token}`, 'Content-Type': body.type || 'application/octet-stream' }, body, signal: signal ?? AbortSignal.timeout(60000), redirect: 'error' });
      if (!response.ok) await this.fail(response);
      return response.json();
    });
  }
  async downloadAttachment(id: string, reference: string, signal?: AbortSignal): Promise<Response> {
    return instrumentRequest('downloadAttachment', async headers => {
      const { response } = await this.http.GET('/sessions/{id}/attachments', { params: { path: { id }, query: { reference } }, parseAs: 'stream', headers, signal: signal ?? AbortSignal.timeout(30000) });
      return response;
    });
  }
  async history(id: string, after = "", limit = 100, signal?: AbortSignal): Promise<import("./generated.js").EventList> {
    return this.request("events", options => this.http.GET("/sessions/{id}/events", { ...options, params: { path: { id }, query: { after, limit } } }), signal);
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
