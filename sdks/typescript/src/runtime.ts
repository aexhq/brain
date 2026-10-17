import type { Command, ExecutorCredential, ExecutorRegistration, Reply, Tool, ToolScope } from "./generated.js";

export interface RuntimeContext {
  signal: AbortSignal;
  scope: ToolScope;
  progress(value: unknown): Promise<void>;
}
export interface RuntimeTool extends Tool {
  run(arguments_: unknown, context: RuntimeContext): unknown | Promise<unknown>;
}
export type RuntimeTransport = "sse" | "rpc";

export async function* streamJSON<T>(body: ReadableStream<Uint8Array<ArrayBuffer>>): AsyncGenerator<T> {
  const reader = body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = "", data: string[] = [], size = 0;
  try {
    while (true) {
      const next = await reader.read();
      if (next.done) { if (buffer || data.length) throw new Error("incomplete command stream"); return; }
      buffer += next.value;
      if (buffer.length + size > 4 << 20) throw new Error("command exceeds client limit");
      let end: number;
      while ((end = buffer.indexOf("\n")) >= 0) {
        const line = buffer.slice(0, end).replace(/\r$/, ""); buffer = buffer.slice(end + 1);
        if (!line) { if (data.length) yield JSON.parse(data.join("\n")) as T; data = []; size = 0; }
        else if (line.startsWith("data:")) { const value = line.slice(5).replace(/^ /, ""); data.push(value); size += value.length; }
      }
    }
  } finally { await reader.cancel(); reader.releaseLock(); }
}

export class RuntimeExecutor {
  readonly abort = new AbortController();
  private socket?: WebSocket;
  private calls = new Map<string, AbortController>();
  private connection = "";
  private ended = false;
  private task?: Promise<void>;
  private constructor(readonly url: string, readonly credential: ExecutorCredential, readonly tools: RuntimeTool[], private fetcher: typeof fetch) {}

  static async open(url: string, token: string, tools: RuntimeTool[], transport: RuntimeTransport, fetcher: typeof fetch, session?: string): Promise<RuntimeExecutor> {
    if (transport !== "sse" && transport !== "rpc") throw new Error("unsupported runtime transport");
    const input: ExecutorRegistration = { binding: "runtime", tools: tools.map(({ run: _, ...descriptor }) => descriptor), ...(session ? { session } : {}) };
    const response = await fetcher(new URL("/executors", url), { method: "POST", headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" }, body: JSON.stringify(input), redirect: "error", signal: AbortSignal.timeout(30000) });
    if (!response.ok) throw new Error(`executor registration failed (${response.status})`);
    const executor = new RuntimeExecutor(url, await response.json() as ExecutorCredential, tools, fetcher);
    try { await executor.connect(transport); return executor; }
    catch (error) { await executor.close(); throw error; }
  }

  private async request(path: string, body?: unknown): Promise<Response> {
    const response = await this.fetcher(new URL(`/executors/${encodeURIComponent(this.credential.id)}/${path}`, this.url), {
      method: body === undefined && path === "commands" ? "GET" : "POST",
      headers: { Authorization: `Bearer ${this.credential.token}`, "Content-Type": "application/json" },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }), redirect: "error",
      signal: path === "commands" ? this.abort.signal : AbortSignal.timeout(30000),
    });
    // A session closed elsewhere has already revoked this executor token.
    if (!response.ok && !(path === "close" && [401, 403].includes(response.status))) throw new Error(`executor ${path} failed (${response.status})`);
    return response;
  }
  private async connect(transport: RuntimeTransport): Promise<void> {
    let ready!: () => void, failed!: (error: unknown) => void;
    const opening = new Promise<void>((resolve, reject) => { ready = resolve; failed = reject; });
    const timer = setTimeout(() => failed(new Error("executor readiness timeout")), 30000);
    const receive = (command: Command) => {
      if (command.type === "ready") { this.connection = command.connection; ready(); return; }
      if (command.connection !== this.connection) throw new Error("stale executor command");
      if (command.type === "ping") return;
      if (command.type === "cancel") { this.calls.get(command.call_id ?? "")?.abort(); return; }
      if (command.type !== "call" || !command.call_id || !command.tool || !command.scope || this.calls.has(command.call_id) || this.calls.size >= 32) throw new Error("invalid runtime call");
      void this.invoke(command).catch(error => { failed(error); this.stop(); });
    };
    if (transport === "sse") {
      this.task = (async () => {
        const response = await this.request("commands");
        if (!response.body) throw new Error("missing command stream");
        for await (const command of streamJSON<Command>(response.body)) receive(command);
        throw new Error("executor disconnected");
      })().catch(error => { failed(error); this.stop(); });
    } else if (transport === "rpc") {
      const endpoint = new URL(`/executors/${encodeURIComponent(this.credential.id)}/rpc`, this.url);
      endpoint.protocol = endpoint.protocol === "https:" ? "wss:" : "ws:";
      this.socket = new WebSocket(endpoint, ["brain-rpc", `brain-auth.${this.credential.token}`]);
      this.socket.addEventListener("message", e => { try { if (typeof e.data !== "string" || e.data.length > 4 << 20) throw new Error("invalid RPC frame"); receive(JSON.parse(e.data) as Command); } catch (error) { failed(error); this.stop(); } });
      this.socket.addEventListener("error", () => { failed(new Error("executor RPC failed")); this.stop(); });
      this.socket.addEventListener("close", () => { failed(new Error("executor disconnected")); this.stop(); });
    } else throw new Error("unsupported runtime transport");
    try { await opening; } finally { clearTimeout(timer); }
  }
  private async reply(reply: Reply): Promise<void> {
    if (this.ended) throw new Error("executor is disconnected");
    if (this.socket) {
      const data = JSON.stringify(reply);
      if (data.length > 2 << 20 || this.socket.bufferedAmount > 4 << 20 || this.socket.readyState !== WebSocket.OPEN) throw new Error("executor RPC output unavailable");
      this.socket.send(data);
    } else await this.request("responses", reply);
  }
  private async invoke(command: Command): Promise<void> {
    const id = command.call_id!, controller = new AbortController();
    const tool = this.tools.find(t => t.name === command.tool);
    if (!tool) throw new Error("unbound runtime tool");
    this.calls.set(id, controller);
    try {
      let result;
      try {
        const value = await tool.run(command.arguments, { signal: controller.signal, scope: command.scope!, progress: async value => { if (!controller.signal.aborted) await this.reply({ connection: this.connection, call_id: id, progress: value }); } });
        const payload = value === undefined ? null : value;
        result = { content: [{ type: "text", text: JSON.stringify(payload) }], structured_content: payload };
      } catch (error) { result = { content: [{ type: "text", text: error instanceof Error ? error.message : "runtime tool failed" }], is_error: true }; }
      if (!controller.signal.aborted) await this.reply({ connection: this.connection, call_id: id, result });
    } finally { this.calls.delete(id); }
  }
  private stop(): void {
    this.ended = true; this.abort.abort(); this.socket?.close();
    for (const controller of this.calls.values()) controller.abort();
  }
  async close(): Promise<void> {
    this.stop();
    await this.request("close");
    await this.task;
  }
}
