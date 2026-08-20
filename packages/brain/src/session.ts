import type {
  CreateSessionRequest,
  Event,
  MessageAccepted,
  Provider,
  Session as SessionData,
  SessionList as SessionListData,
  SessionState,
} from "./generated/session.js";

import {
  AbortError,
  SessionError,
  abortError,
  errorFromApi,
} from "./errors.js";
import { randomIdempotencyKey } from "./json.js";
import type { EventOptions } from "./transport.js";
import { Transport } from "./transport.js";
import { compileTools } from "./tools.js";
import type { Tool } from "./tools.js";
import { AttachedWorker, type WebSocketFactory } from "./attached.js";

export type SessionInput = string;

export interface ModelOptions {
  provider: Provider;
  name: string;
  apiKey: string;
  baseUrl?: string;
  maxOutputTokens?: number;
  temperature?: number;
  reasoningEffort?: "low" | "medium" | "high";
}

export interface McpServerOptions {
  name: string;
  url: string;
  headers?: Record<string, string>;
  protocol?: "auto" | "2026-07" | "legacy";
  allowedTools?: readonly string[];
}

export interface CreateSessionOptions {
  model: ModelOptions;
  /** Omitted or empty grants no tools. A non-empty list is the exact grant. */
  tools?: readonly Tool[];
  /** Optional remote MCP servers. Discovery happens once and is sealed at session creation. */
  mcp?: readonly McpServerOptions[];
  systemPrompt?: string;
  hand?: {
    enabled?: boolean;
    env?: Record<string, string>;
  };
  metadata?: Record<string, string>;
}

export interface RequestOptions {
  signal?: AbortSignal;
  idempotencyKey?: string;
  metadata?: Record<string, string>;
}

export interface ListSessionsOptions {
  limit?: number;
  cursor?: string;
  state?: SessionState;
  signal?: AbortSignal;
}

export interface SessionList {
  data: Session[];
  hasMore: boolean;
  nextCursor?: string;
}

export interface ModelSummary {
  provider: Provider;
  name: string;
  baseUrl?: string;
}

export interface SessionSummary {
  id: string;
  state: SessionState;
  model: ModelSummary;
  createdAt: string;
  updatedAt: string;
  metadata: Readonly<Record<string, string | undefined>>;
}

export class Sessions {
  readonly #transport: Transport;
  readonly #webSocketFactory: WebSocketFactory | undefined;

  constructor(transport: Transport, webSocketFactory?: WebSocketFactory) {
    this.#transport = transport;
    this.#webSocketFactory = webSocketFactory;
  }

  async create(options: CreateSessionOptions, request: RequestOptions = {}): Promise<Session> {
    const compiledTools = await compileTools(options.tools);
    if (compiledTools.attached.size > 0 && this.#webSocketFactory === undefined) {
      throw new TypeError("This runtime does not provide WebSocket; pass a webSocketFactory to Brain");
    }
    const body = {
      model: {
        provider: options.model.provider,
        name: options.model.name,
        api_key: options.model.apiKey,
        ...(options.model.baseUrl === undefined ? {} : { base_url: options.model.baseUrl }),
        ...(options.model.maxOutputTokens === undefined
          ? {}
          : { max_output_tokens: options.model.maxOutputTokens }),
        ...(options.model.temperature === undefined ? {} : { temperature: options.model.temperature }),
        ...(options.model.reasoningEffort === undefined
          ? {}
          : { reasoning_effort: options.model.reasoningEffort }),
      },
      tools: {
        items: compiledTools.items,
        ...(options.mcp === undefined
          ? {}
          : {
              mcp: options.mcp.map((server) => ({
                name: server.name,
                url: server.url,
                ...(server.headers === undefined ? {} : { headers: server.headers }),
                ...(server.protocol === undefined ? {} : { protocol: server.protocol }),
                ...(server.allowedTools === undefined
                  ? {}
                  : { allowed_tools: [...server.allowedTools] }),
              })),
            }),
      },
      ...(compiledTools.bundles.length === 0 ? {} : { tool_bundles: compiledTools.bundles }),
      ...(options.hand === undefined
        ? {}
        : {
            hand: {
              ...(options.hand.enabled === undefined ? {} : { enabled: options.hand.enabled }),
              ...(options.hand.env === undefined ? {} : { env: options.hand.env }),
            },
          }),
      ...(options.systemPrompt === undefined ? {} : { system_prompt: options.systemPrompt }),
      ...(options.metadata === undefined ? {} : { metadata: options.metadata }),
    } as CreateSessionRequest;
    const data = await this.#transport.json<SessionData>("POST", "/v1/sessions", {
      body,
      headers: { "Idempotency-Key": request.idempotencyKey ?? randomIdempotencyKey() },
      signal: request.signal,
      retry: true,
    });
    let worker: AttachedWorker | undefined;
    if (compiledTools.attached.size > 0) {
      const connection = this.#transport.attachedConnection(data.id);
      worker = new AttachedWorker(
        connection.url,
        connection.token,
        compiledTools.attached,
        this.#webSocketFactory!,
      );
      try {
        await worker.ready;
      } catch (cause) {
        throw new SessionError(`Session ${data.id} was created but its attached Tool worker could not connect`, {
          cause,
        });
      }
    }
    return new Session(this.#transport, data, worker);
  }

  async get(id: string, options: Pick<RequestOptions, "signal"> = {}): Promise<Session> {
    const data = await this.#transport.json<SessionData>(
      "GET",
      `/v1/sessions/${encodeURIComponent(id)}`,
      { signal: options.signal },
    );
    return new Session(this.#transport, data);
  }

  async list(options: ListSessionsOptions = {}): Promise<SessionList> {
    const query = new URLSearchParams();
    if (options.limit !== undefined) query.set("limit", String(options.limit));
    if (options.cursor !== undefined) query.set("cursor", options.cursor);
    if (options.state !== undefined) query.set("state", options.state);
    const suffix = query.size === 0 ? "" : `?${query}`;
    const list = await this.#transport.json<SessionListData>("GET", `/v1/sessions${suffix}`, {
      signal: options.signal,
    });
    return {
      data: list.data.map((data) => new Session(this.#transport, data)),
      hasMore: list.has_more,
      ...(list.next_cursor === undefined ? {} : { nextCursor: list.next_cursor }),
    };
  }
}

export class Session implements SessionSummary {
  readonly #transport: Transport;
  #data: SessionData;
  readonly #attachedWorker: AttachedWorker | undefined;

  constructor(transport: Transport, data: SessionData, attachedWorker?: AttachedWorker) {
    this.#transport = transport;
    this.#data = data;
    this.#attachedWorker = attachedWorker;
  }

  get id(): string {
    return this.#data.id;
  }

  get state(): SessionState {
    return this.#data.state;
  }

  get model(): ModelSummary {
    return {
      provider: this.#data.model.provider,
      name: this.#data.model.name,
      ...(this.#data.model.base_url === undefined ? {} : { baseUrl: this.#data.model.base_url }),
    };
  }

  get createdAt(): string {
    return this.#data.created_at;
  }

  get updatedAt(): string {
    return this.#data.updated_at;
  }

  get metadata(): Readonly<Record<string, string | undefined>> {
    return this.#data.metadata;
  }

  async refresh(options: Pick<RequestOptions, "signal"> = {}): Promise<this> {
    this.#data = await this.#transport.json<SessionData>(
      "GET",
      `/v1/sessions/${encodeURIComponent(this.id)}`,
      { signal: options.signal },
    );
    return this;
  }

  async send(input: SessionInput, options: RequestOptions = {}): Promise<string> {
    const accepted = await this.#transport.json<MessageAccepted>(
      "POST",
      `/v1/sessions/${encodeURIComponent(this.id)}/messages`,
      {
        body: {
          content: input,
          ...(options.metadata === undefined ? {} : { metadata: options.metadata }),
        },
        headers: { "Idempotency-Key": options.idempotencyKey ?? randomIdempotencyKey() },
        signal: options.signal,
        retry: true,
      },
    );
    let answer = "";
    try {
      for await (const event of this.events({ after: Math.max(0, accepted.seq - 1), signal: options.signal })) {
        if (event.type === "assistant.message" && event.turn_id === accepted.turn_id && event.agent_id === "root") {
          answer = event.text;
        } else if (event.type === "turn.failed" && event.turn_id === accepted.turn_id) {
          throw errorFromApi(event.error);
        } else if (event.type === "turn.completed" && event.turn_id === accepted.turn_id) {
          this.markIdle();
          if (event.stop_reason === "cancelled") throw new AbortError();
          return answer;
        }
      }
    } catch (error) {
      if (options.signal?.aborted === true) {
        await this.cancel().catch(() => undefined);
        throw abortError(error);
      }
      throw error;
    }
    throw new SessionError("The Brain event stream ended before the session finished its work");
  }

  events(options: EventOptions = {}): AsyncGenerator<Event> {
    return this.#transport.events(this.id, options);
  }

  async cancel(options: Pick<RequestOptions, "signal"> = {}): Promise<this> {
    this.#data = await this.#transport.json<SessionData>(
      "POST",
      `/v1/sessions/${encodeURIComponent(this.id)}/cancel`,
      { signal: options.signal },
    );
    return this;
  }

  async delete(options: Pick<RequestOptions, "signal"> = {}): Promise<void> {
    await this.#transport.json<void>("DELETE", `/v1/sessions/${encodeURIComponent(this.id)}`, {
      signal: options.signal,
    });
    this.#data = { ...this.#data, state: "deleted" };
    this.#attachedWorker?.close();
  }

  private markIdle(): void {
    this.#data = { ...this.#data, state: "idle" };
  }
}
