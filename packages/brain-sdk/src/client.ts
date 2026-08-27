import { BrainError } from "./errors.js";
import type { AgentloopAdmission, CreateSessionRequest, EventPage, Session, SessionList } from "./types.js";

export interface BrainOptions {
  baseUrl: string;
  apiKey?: string;
  timeoutMs?: number;
  fetch?: typeof globalThis.fetch;
}

export class BrainClient {
  readonly baseUrl: string;
  private readonly apiKey?: string;
  private readonly timeoutMs: number;
  private readonly transport: typeof globalThis.fetch;

  constructor(options: BrainOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/u, "");
    if (this.baseUrl.length === 0) throw new TypeError("baseUrl is required");
    const url = new URL(this.baseUrl);
    if ((url.protocol !== "http:" && url.protocol !== "https:") || url.username !== "" || url.password !== "" || url.search !== "" || url.hash !== "") {
      throw new TypeError("baseUrl must be HTTP(S) without credentials, query, or fragment");
    }
    if (options.apiKey !== undefined && options.apiKey.trim() === "") throw new TypeError("apiKey cannot be empty");
    if (!Number.isSafeInteger(options.timeoutMs ?? 30_000) || (options.timeoutMs ?? 30_000) < 1) {
      throw new TypeError("timeoutMs must be a positive safe integer");
    }
    this.apiKey = options.apiKey;
    this.timeoutMs = options.timeoutMs ?? 30_000;
    this.transport = options.fetch ?? globalThis.fetch;
  }

  async admitAgentloop(packageBytes: Uint8Array, idempotencyKey: string): Promise<AgentloopAdmission> {
    return this.request("POST", "/v1/agentloops", packageBytes, idempotencyKey, "application/octet-stream");
  }

  async getAgentloop(digest: string): Promise<AgentloopAdmission> {
    return this.request("GET", `/v1/agentloops/${encodeURIComponent(digest)}`);
  }

  async listSessions(): Promise<SessionList> {
    return this.request("GET", "/v1/sessions");
  }

  async createSession(request: CreateSessionRequest, idempotencyKey: string): Promise<SessionHandle> {
    const session = await this.request<Session>("POST", "/v1/sessions", request, idempotencyKey);
    return new SessionHandle(this, session);
  }

  async getSession(sessionId: string): Promise<SessionHandle> {
    const session = await this.request<Session>("GET", `/v1/sessions/${encodeURIComponent(sessionId)}`);
    return new SessionHandle(this, session);
  }

  async sendMessage(sessionId: string, content: unknown, idempotencyKey: string): Promise<Session> {
    return this.request("POST", `/v1/sessions/${encodeURIComponent(sessionId)}/messages`, { content }, idempotencyKey);
  }

  async readEvents(sessionId: string, after = 0): Promise<EventPage> {
    return this.request("GET", `/v1/sessions/${encodeURIComponent(sessionId)}/events?after=${after}`);
  }

  async request<T>(method: string, path: string, body?: unknown, idempotencyKey?: string, contentType = "application/json"): Promise<T> {
    const headers = new Headers({ accept: "application/json" });
    if (body !== undefined) headers.set("content-type", contentType);
    if (this.apiKey !== undefined) headers.set("authorization", `Bearer ${this.apiKey}`);
    if (idempotencyKey !== undefined) headers.set("idempotency-key", idempotencyKey);
    const response = await this.transport(`${this.baseUrl}${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : body instanceof Uint8Array ? new Uint8Array(body).buffer : JSON.stringify(body),
      signal: AbortSignal.timeout(this.timeoutMs),
    });
    if (!response.ok) {
      const error = (await response.json().catch(() => ({}))) as Partial<BrainError>;
      throw new BrainError(response.status, error.code ?? "http_error", error.message ?? response.statusText, error.retryable ?? false, error.details);
    }
    return (response.status === 204 ? undefined : await response.json()) as T;
  }
}

export class SessionHandle {
  constructor(private readonly client: BrainClient, public state: Session) {}
  get id(): string { return this.state.session_id; }

  async send(content: unknown, idempotencyKey: string): Promise<Session> {
    return (this.state = await this.client.sendMessage(this.id, content, idempotencyKey));
  }

  events(after = 0): AsyncIterable<import("./types.js").SessionEvent> {
    const client = this.client;
    const sessionId = this.id;
    return {
      async *[Symbol.asyncIterator]() {
        let cursor = after;
        for (;;) {
          const page = await client.readEvents(sessionId, cursor);
          for (const event of page.events) yield event;
          if (page.next_cursor === cursor) return;
          cursor = page.next_cursor;
        }
      },
    };
  }

  async cancel(idempotencyKey: string): Promise<void> {
    await this.client.request("POST", `/v1/sessions/${encodeURIComponent(this.id)}/cancel`, undefined, idempotencyKey);
  }

  async end(idempotencyKey: string): Promise<Session> {
    return (this.state = await this.client.request("POST", `/v1/sessions/${encodeURIComponent(this.id)}/end`, undefined, idempotencyKey));
  }

  async delete(idempotencyKey: string): Promise<void> {
    await this.client.request("DELETE", `/v1/sessions/${encodeURIComponent(this.id)}`, undefined, idempotencyKey);
  }
}
