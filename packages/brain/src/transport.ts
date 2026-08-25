import type { ApiError, Event } from "./generated/session.js";

import {
  MAX_CREATE_SESSION_REQUEST_BYTES,
  MAX_CUSTOMER_OBSERVATION_BYTES,
  MAX_CUSTOMER_WS_FRAME_BYTES,
  MAX_MESSAGE_REQUEST_BYTES,
  MAX_PUBLIC_EVENT_BYTES,
} from "./limits.js";

import { AbortError, BrainError, SessionError, abortError, errorFromApi } from "./errors.js";

export type Fetch = (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>;

export interface JsonRequestOptions {
  body?: unknown;
  headers?: Record<string, string>;
  signal?: AbortSignal | undefined;
  retry?: boolean;
}

/** Structural HTTP/event port used by neutral resource controllers and downstream SDKs. */
export interface SessionTransport {
  json<T>(
    method: "GET" | "POST" | "DELETE",
    path: string,
    options?: JsonRequestOptions,
  ): Promise<T>;
  events(sessionId: string, options?: EventOptions): AsyncGenerator<Event>;
}

export interface EventOptions {
  after?: number;
  follow?: boolean;
  signal?: AbortSignal | undefined;
}

interface ErrorEnvelope {
  error?: ApiError;
}

const MAX_ORDINARY_JSON_BYTES = 2 * 1024 * 1024;
const MAX_ERROR_RESPONSE_BYTES = 64 * 1024;
const MAX_SSE_FRAME_OVERHEAD_BYTES = 4 * 1024;
const MAX_SSE_WIRE_FRAME_BYTES = MAX_PUBLIC_EVENT_BYTES + MAX_SSE_FRAME_OVERHEAD_BYTES;

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

  async customerEnvironmentGrant(clientId: string, signal?: AbortSignal): Promise<{
    url: string;
    protocol: string;
    expiresAt: string;
    observationUrl: string;
    observationToken: string;
  }> {
    const grant = await this.json<{
      url: string;
      protocol: string;
      expires_at: string;
      observation_url: string;
      observation_token: string;
    }>(
      "POST",
      "/v1/customer-environment/grants",
      { body: { client_id: clientId }, signal },
    );
    return {
      url: grant.url,
      protocol: grant.protocol,
      expiresAt: grant.expires_at,
      observationUrl: grant.observation_url,
      observationToken: grant.observation_token,
    };
  }

  async customerEnvironmentObserve(
    url: string,
    token: string,
    observation: unknown,
    signal?: AbortSignal,
  ): Promise<void> {
    const body = encodeJsonOnce(observation, MAX_CUSTOMER_OBSERVATION_BYTES, "Customer Environment observation");
    const response = await this.#fetch(url, {
      method: "POST",
      headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
      body,
      ...(signal === undefined ? {} : { signal }),
    });
    if (!response.ok) {
      const preview = await readResponseText(response, 4096).catch(() => "<response too large or unreadable>");
      throw new SessionError(`Customer Environment observation ingress returned HTTP ${response.status}: ${preview}`);
    }
  }

  async json<T>(
    method: "GET" | "POST" | "DELETE",
    path: string,
    options: JsonRequestOptions = {},
  ): Promise<T> {
    const attempts = options.retry === true ? 2 : 1;
    const requestBody = options.body === undefined
      ? undefined
      : encodeJsonOnce(options.body, requestLimit(method, path), "Brain API request");
    let lastError: unknown;
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      try {
        const response = await this.#fetch(`${this.baseUrl}${path}`, {
          method,
          headers: {
            Accept: "application/json",
            ...(requestBody === undefined ? {} : { "Content-Type": "application/json" }),
            Authorization: `Bearer ${this.#apiKey}`,
            ...options.headers,
          },
          ...(requestBody === undefined ? {} : { body: requestBody }),
          ...(options.signal === undefined ? {} : { signal: options.signal }),
        });
        if (!response.ok) throw await this.responseError(response);
        if (response.status === 204) return undefined as T;
        const text = await readResponseText(response, MAX_ORDINARY_JSON_BYTES);
        try {
          return JSON.parse(text) as T;
        } catch (cause) {
          throw new SessionError("Brain sent an invalid JSON response", { cause });
        }
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
          const durableSeq = eventCursor(event);
          if (durableSeq !== undefined) {
            if (durableSeq <= cursor) continue;
            cursor = durableSeq;
          }
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
      envelope = JSON.parse(await readResponseText(response, MAX_ERROR_RESPONSE_BYTES)) as ErrorEnvelope;
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

export async function* parseEventStream(stream: ReadableStream<Uint8Array>): AsyncGenerator<Event> {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let frame: SseFrame = { data: [] };
  let frameBytes = 0;
  let dataBytes = 0;

  const dispatch = (): Event | undefined => {
    if (frame.data.length === 0) {
      frame = { data: [] };
      frameBytes = 0;
      dataBytes = 0;
      return undefined;
    }
    const data = frame.data.join("\n");
    frame = { data: [] };
    frameBytes = 0;
    dataBytes = 0;
    try {
      return JSON.parse(data) as Event;
    } catch (cause) {
      throw new SessionError("Brain sent an invalid event", { cause });
    }
  };

  const line = (raw: string): Event | undefined => {
    frameBytes += new TextEncoder().encode(raw).byteLength + 1;
    if (frameBytes > MAX_SSE_WIRE_FRAME_BYTES) {
      throw new SessionError(`Brain event frame exceeds ${MAX_SSE_WIRE_FRAME_BYTES} wire bytes`);
    }
    const value = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
    if (value === "") return dispatch();
    if (value.startsWith(":")) return undefined;
    const separator = value.indexOf(":");
    const field = separator === -1 ? value : value.slice(0, separator);
    let content = separator === -1 ? "" : value.slice(separator + 1);
    if (content.startsWith(" ")) content = content.slice(1);
    if (field === "data") {
      dataBytes += new TextEncoder().encode(content).byteLength + (frame.data.length === 0 ? 0 : 1);
      if (dataBytes > MAX_PUBLIC_EVENT_BYTES) {
        throw new SessionError(`Brain event payload exceeds ${MAX_PUBLIC_EVENT_BYTES} bytes`);
      }
      frame.data.push(content);
    } else if (field === "id") frame.id = content;
    else if (field === "event") frame.event = content;
    return undefined;
  };

  try {
    while (true) {
      const { done, value } = await reader.read();
      buffer += decoder.decode(value, { stream: !done });
      if (new TextEncoder().encode(buffer).byteLength > MAX_SSE_WIRE_FRAME_BYTES) {
        throw new SessionError(`Brain event line exceeds ${MAX_SSE_WIRE_FRAME_BYTES} bytes`);
      }
      let newline = buffer.indexOf("\n");
      while (newline !== -1) {
        const event = line(buffer.slice(0, newline));
        buffer = buffer.slice(newline + 1);
        if (event !== undefined) yield event;
        newline = buffer.indexOf("\n");
      }
      if (done) break;
    }
    if (buffer !== "" || frameBytes !== 0 || frame.data.length !== 0) {
      throw new SessionError("Brain event stream ended with a truncated frame");
    }
  } finally {
    await reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
}

function eventCursor(event: Event): number | undefined {
  if (
    event.type === "assistant.delta" ||
    event.type === "tool.output" ||
    event.type === "replay.complete"
  ) {
    return undefined;
  }
  const seq = (event as { seq?: unknown }).seq;
  if (typeof seq !== "number" || !Number.isSafeInteger(seq) || seq < 1) {
    throw new SessionError("Brain sent a durable event without a valid sequence");
  }
  return seq;
}

function requestLimit(method: "GET" | "POST" | "DELETE", path: string): number {
  if (method === "POST" && path === "/v1/sessions") return MAX_CREATE_SESSION_REQUEST_BYTES;
  if (method === "POST" && /\/v1\/sessions\/[^/]+\/messages(?:\?|$)/u.test(path)) {
    return MAX_MESSAGE_REQUEST_BYTES;
  }
  if (
    method === "POST"
    && /\/v1\/sessions\/[^/]+\/children(?:\/[^/]+\/(?:messages|follow-up))?(?:\?|$)/u.test(path)
  ) {
    return MAX_MESSAGE_REQUEST_BYTES;
  }
  if (path.includes("/customer-environment/gateway")) return MAX_CUSTOMER_WS_FRAME_BYTES;
  return MAX_ORDINARY_JSON_BYTES;
}

function encodeJsonOnce(value: unknown, maxBytes: number, label: string): string {
  let encoded: string | undefined;
  try {
    encoded = JSON.stringify(value);
  } catch (cause) {
    throw new TypeError(`${label} is not JSON-serializable`, { cause });
  }
  if (encoded === undefined) throw new TypeError(`${label} must be a JSON value`);
  const bytes = new TextEncoder().encode(encoded).byteLength;
  if (bytes > maxBytes) throw new TypeError(`${label} exceeds ${maxBytes} bytes`);
  return encoded;
}

async function readResponseText(response: Response, maxBytes: number): Promise<string> {
  if (response.body === null) return "";
  if (response.headers.get("content-length") !== null) {
    const length = Number(response.headers.get("content-length"));
    if (Number.isFinite(length) && length > maxBytes) {
      await response.body.cancel().catch(() => undefined);
      throw new SessionError(`Brain response exceeds ${maxBytes} bytes`);
    }
  }
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let bytes = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      bytes += value.byteLength;
      if (bytes > maxBytes) throw new SessionError(`Brain response exceeds ${maxBytes} bytes`);
      chunks.push(value);
    }
  } finally {
    await reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
  const merged = new Uint8Array(bytes);
  let offset = 0;
  for (const chunk of chunks) {
    merged.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return new TextDecoder().decode(merged);
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
