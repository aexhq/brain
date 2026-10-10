import { BrainError } from "./errors.js";
import type { SessionStreamEvent } from "./types.js";

export interface HttpOptions {
  baseUrl: string;
  token?: string;
  timeoutMs?: number;
  fetch?: typeof globalThis.fetch;
}

/** Owns one credential's HTTP requests, streams and cancellation lifetime. */
export class HttpTransport {
  readonly baseUrl: string;
  readonly options: HttpOptions;
  private readonly token?: string;
  private readonly timeoutMs?: number;
  readonly transport: typeof globalThis.fetch;
  private readonly controller = new AbortController();
  readonly signal = this.controller.signal;
  private readonly requests = new Set<Promise<unknown>>();
  private readonly readers = new Set<ReadableStreamDefaultReader<Uint8Array>>();
  private closing?: Promise<void>;

  constructor(options: HttpOptions) {
    let end = options.baseUrl.length;
    while (end > 0 && options.baseUrl.charCodeAt(end - 1) === 47) end -= 1;
    this.baseUrl = options.baseUrl.slice(0, end);
    if (this.baseUrl.length === 0) throw new TypeError("baseUrl is required");
    const url = new URL(this.baseUrl);
    if ((url.protocol !== "http:" && url.protocol !== "https:") || url.username !== "" || url.password !== "" || url.search !== "" || url.hash !== "") {
      throw new TypeError("baseUrl must be HTTP(S) without credentials, query, or fragment");
    }
    if (options.token !== undefined && options.token.trim() === "") throw new TypeError("token cannot be empty");
    if (options.timeoutMs !== undefined && (!Number.isSafeInteger(options.timeoutMs) || options.timeoutMs < 1)) throw new TypeError("timeoutMs must be a positive safe integer");
    this.options = { baseUrl: this.baseUrl, token: options.token, timeoutMs: options.timeoutMs, fetch: options.fetch };
    this.token = options.token;
    this.timeoutMs = options.timeoutMs;
    this.transport = options.fetch ?? globalThis.fetch.bind(globalThis);
  }

  close(): Promise<void> {
    if (this.closing !== undefined) return this.closing;
    this.closing = Promise.resolve().then(async () => {
      const readers = [...this.readers].map(reader => reader.cancel().catch(() => {}));
      await Promise.allSettled([...readers, ...this.requests]);
      this.readers.clear();
    });
    this.controller.abort(new DOMException("Brain client is closed", "AbortError"));
    return this.closing;
  }

  async io<T>(operation: (signal: AbortSignal) => Promise<T>, signal?: AbortSignal): Promise<T> {
    this.controller.signal.throwIfAborted();
    const combined = signal === undefined ? this.controller.signal : AbortSignal.any([this.controller.signal, signal]);
    combined.throwIfAborted();
    const pending = operation(combined);
    this.requests.add(pending);
    try { return await pending; }
    finally { this.requests.delete(pending); }
  }

  async request<T>(method: string, path: string, body?: unknown, idempotencyKey?: string, contentType = "application/json", signal?: AbortSignal, extraHeaders?: HeadersInit): Promise<T> {
    const headers = new Headers(extraHeaders);
    headers.set("accept", "application/json");
    if (body !== undefined) headers.set("content-type", contentType);
    if (this.token !== undefined) headers.set("authorization", `Bearer ${this.token}`);
    if (idempotencyKey !== undefined) headers.set("idempotency-key", idempotencyKey);
    return this.io(async (signal) => {
      const response = await this.transport(`${this.baseUrl}${path}`, {
        method,
        headers,
        body: body === undefined ? undefined : body instanceof Uint8Array ? new Uint8Array(body).buffer : JSON.stringify(body),
        signal,
      });
      if (!response.ok) {
        const error = (await response.json().catch(() => ({}))) as Partial<BrainError>;
        signal.throwIfAborted();
        throw new BrainError(response.status, error.code ?? "http_error", error.message ?? response.statusText, error.retryable ?? false, error.details);
      }
      const value = response.status === 204 ? undefined : await response.json();
      signal.throwIfAborted();
      return value as T;
    }, this.timeoutMs === undefined ? signal : signal === undefined ? AbortSignal.timeout(this.timeoutMs) : AbortSignal.any([signal, AbortSignal.timeout(this.timeoutMs)]));
  }

  async *stream(path: string, signal?: AbortSignal, onOpen?: () => void): AsyncGenerator<SessionStreamEvent> {
    const headers = new Headers({ accept: "text/event-stream" });
    if (this.token !== undefined) headers.set("authorization", `Bearer ${this.token}`);
    const response = await this.io((signal) => this.transport(`${this.baseUrl}${path}`, { headers, signal }), signal);
    if (this.controller.signal.aborted) {
      await response.body?.cancel().catch(() => {});
      this.controller.signal.throwIfAborted();
    }
    if (!response.ok || response.body === null) {
      const error = (await response.json().catch(() => ({}))) as Partial<BrainError>;
      this.controller.signal.throwIfAborted();
      signal?.throwIfAborted();
      throw new BrainError(response.status, error.code ?? "http_error", error.message ?? response.statusText, error.retryable ?? false, error.details);
    }
    onOpen?.();
    const reader = response.body.getReader();
    this.readers.add(reader);
    const decoder = new TextDecoder();
    let buffer = "";
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) return;
        buffer += decoder.decode(value, { stream: true });
        let boundary;
        while ((boundary = buffer.indexOf("\n\n")) !== -1) {
          const frame = buffer.slice(0, boundary);
          buffer = buffer.slice(boundary + 2);
          let id: string | undefined;
          let type: string | undefined;
          const data: string[] = [];
          for (const raw of frame.split("\n")) {
            const line = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
            if (line.startsWith("id:")) id = line.slice(3).trim();
            else if (line.startsWith("event:")) type = line.slice(6).trim();
            else if (line.startsWith("data:")) data.push(line.slice(5).trimStart());
          }
          if (type === undefined) continue;
          const text = data.join("\n");
          let payload: unknown = text;
          try { payload = JSON.parse(text); } catch { /* keep non-JSON payloads */ }
          this.controller.signal.throwIfAborted();
          signal?.throwIfAborted();
          yield { ...(id === undefined || id === "" ? {} : { sequence: Number(id) }), type, data: payload };
        }
      }
    } finally {
      await reader.cancel().catch(() => {});
      this.readers.delete(reader);
    }
  }

}
