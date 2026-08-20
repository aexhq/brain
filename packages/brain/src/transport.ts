import type { ApiError, Event } from "./generated/session.js";

import { AbortError, BrainError, SessionError, abortError, errorFromApi } from "./errors.js";

export type Fetch = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

interface JsonRequestOptions {
  body?: unknown;
  headers?: Record<string, string>;
  signal?: AbortSignal | undefined;
  retry?: boolean;
}

export interface EventOptions {
  after?: number;
  follow?: boolean;
  signal?: AbortSignal | undefined;
}

interface ErrorEnvelope {
  error?: ApiError;
}

export class Transport {
  readonly baseUrl: string;
  readonly #apiKey: string;
  readonly #fetch: Fetch;

  constructor(apiKey: string, baseUrl: string, fetchImplementation: Fetch) {
    this.#apiKey = apiKey;
    let end = baseUrl.length;
    while (end > 0 && baseUrl.charCodeAt(end - 1) === 47) end -= 1;
    this.baseUrl = baseUrl.slice(0, end);
    this.#fetch = fetchImplementation;
  }

  attachedConnection(sessionId: string): { url: string; token: string } {
    const base = new URL(this.baseUrl);
    base.protocol = base.protocol === "https:" ? "wss:" : "ws:";
    base.pathname = `/v1/sessions/${encodeURIComponent(sessionId)}/attached`;
    base.search = "";
    base.hash = "";
    return { url: base.toString(), token: this.#apiKey };
  }

  async json<T>(
    method: "GET" | "POST" | "DELETE",
    path: string,
    options: JsonRequestOptions = {},
  ): Promise<T> {
    const attempts = options.retry === true ? 2 : 1;
    let lastError: unknown;
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      try {
        const response = await this.#fetch(`${this.baseUrl}${path}`, {
          method,
          headers: {
            Accept: "application/json",
            ...(options.body === undefined ? {} : { "Content-Type": "application/json" }),
            Authorization: `Bearer ${this.#apiKey}`,
            ...options.headers,
          },
          ...(options.body === undefined ? {} : { body: JSON.stringify(options.body) }),
          ...(options.signal === undefined ? {} : { signal: options.signal }),
        });
        if (!response.ok) throw await this.responseError(response);
        if (response.status === 204) return undefined as T;
        return (await response.json()) as T;
      } catch (error) {
        if (options.signal?.aborted === true || isAbort(error)) throw abortError(error);
        if (error instanceof BrainError) throw error;
        lastError = error;
      }
    }
    throw new SessionError("Could not reach the Brain server", { cause: lastError });
  }

  async *events(sessionId: string, options: EventOptions = {}): AsyncGenerator<Event> {
    let cursor = options.after ?? 0;
    const follow = options.follow ?? true;
    let consecutiveFailures = 0;

    while (true) {
      try {
        const query = new URLSearchParams({ after: String(cursor), follow: String(follow) });
        const response = await this.#fetch(
          `${this.baseUrl}/v1/sessions/${encodeURIComponent(sessionId)}/events?${query}`,
          {
            method: "GET",
            headers: {
              Accept: "text/event-stream",
              Authorization: `Bearer ${this.#apiKey}`,
              ...(cursor > 0 ? { "Last-Event-ID": String(cursor) } : {}),
            },
            ...(options.signal === undefined ? {} : { signal: options.signal }),
          },
        );
        if (!response.ok) throw await this.responseError(response);
        if (response.body === null) throw new SessionError("The Brain event stream had no body");

        let received = false;
        for await (const event of parseEventStream(response.body)) {
          if (event.seq <= cursor) continue;
          cursor = event.seq;
          received = true;
          consecutiveFailures = 0;
          yield event;
        }
        if (!follow) return;
        if (!received) consecutiveFailures += 1;
      } catch (error) {
        if (options.signal?.aborted === true || isAbort(error)) throw abortError(error);
        if (error instanceof BrainError && error.status !== undefined && error.status < 500) throw error;
        consecutiveFailures += 1;
        if (consecutiveFailures > 5) {
          if (error instanceof BrainError) throw error;
          throw new SessionError("The Brain event stream disconnected", { cause: error });
        }
      }

      await delay(Math.min(2_000, 100 * 2 ** (consecutiveFailures - 1)), options.signal);
    }
  }

  private async responseError(response: Response): Promise<BrainError> {
    let envelope: ErrorEnvelope | undefined;
    try {
      envelope = (await response.json()) as ErrorEnvelope;
    } catch {
      // Fall through to a status-based error.
    }
    if (envelope?.error !== undefined) return errorFromApi(envelope.error, response.status);
    return new SessionError(`Brain API request failed with HTTP ${response.status}`, {
      status: response.status,
      requestId: response.headers.get("x-request-id") ?? undefined,
    });
  }
}

interface SseFrame {
  id?: string;
  event?: string;
  data: string[];
}

async function* parseEventStream(stream: ReadableStream<Uint8Array>): AsyncGenerator<Event> {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let frame: SseFrame = { data: [] };

  const dispatch = (): Event | undefined => {
    if (frame.data.length === 0) {
      frame = { data: [] };
      return undefined;
    }
    const data = frame.data.join("\n");
    frame = { data: [] };
    try {
      return JSON.parse(data) as Event;
    } catch (cause) {
      throw new SessionError("Brain sent an invalid event", { cause });
    }
  };

  const line = (raw: string): Event | undefined => {
    const value = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
    if (value === "") return dispatch();
    if (value.startsWith(":")) return undefined;
    const separator = value.indexOf(":");
    const field = separator === -1 ? value : value.slice(0, separator);
    let content = separator === -1 ? "" : value.slice(separator + 1);
    if (content.startsWith(" ")) content = content.slice(1);
    if (field === "data") frame.data.push(content);
    else if (field === "id") frame.id = content;
    else if (field === "event") frame.event = content;
    return undefined;
  };

  try {
    while (true) {
      const { done, value } = await reader.read();
      buffer += decoder.decode(value, { stream: !done });
      let newline = buffer.indexOf("\n");
      while (newline !== -1) {
        const event = line(buffer.slice(0, newline));
        buffer = buffer.slice(newline + 1);
        if (event !== undefined) yield event;
        newline = buffer.indexOf("\n");
      }
      if (done) break;
    }
    if (buffer !== "") {
      const event = line(buffer);
      if (event !== undefined) yield event;
    }
    const final = dispatch();
    if (final !== undefined) yield final;
  } finally {
    reader.releaseLock();
  }
}

function isAbort(error: unknown): boolean {
  return error instanceof AbortError || (error instanceof DOMException && error.name === "AbortError");
}

function delay(milliseconds: number, signal?: AbortSignal): Promise<void> {
  if (signal?.aborted === true) return Promise.reject(abortError(signal.reason));
  return new Promise((resolve, reject) => {
    const cleanup = (): void => signal?.removeEventListener("abort", onAbort);
    const timer = setTimeout(() => {
      cleanup();
      resolve();
    }, milliseconds);
    const onAbort = (): void => {
      clearTimeout(timer);
      cleanup();
      reject(abortError(signal?.reason));
    };
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}
