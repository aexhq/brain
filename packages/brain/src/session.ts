import type {
  ApiError,
  CreateSessionRequest,
  Event,
  MessageAccepted,
  OutputValidationIssue,
  Provider,
  Session as SessionData,
  SessionList as SessionListData,
  SessionState,
} from "./generated/session.js";
import * as z from "zod";

import {
  AbortError,
  OutputRefusalError,
  OutputSchemaError,
  OutputValidationError,
  SessionError,
  abortError,
  errorFromApi,
} from "./errors.js";
import { jcsSha256, randomIdempotencyKey } from "./json.js";
import type { EventOptions } from "./transport.js";
import { Transport } from "./transport.js";
import { compileTools } from "./tools.js";
import type { Tool } from "./tools.js";

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

export interface CreateSessionOptions {
  model: ModelOptions;
  /** Omitted or empty grants no tools. A non-empty list is the exact grant. */
  tools?: readonly Tool[];
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

export interface OutputOptions<Schema extends z.ZodType = z.ZodType> extends RequestOptions {
  output: Schema;
  /** Extra attempts after the first invalid candidate. Defaults to 1; maximum 2. */
  outputRetries?: 0 | 1 | 2;
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

  constructor(transport: Transport) {
    this.#transport = transport;
  }

  async create(options: CreateSessionOptions, request: RequestOptions = {}): Promise<Session> {
    const compiledTools = await compileTools(options.tools);
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
      tools: { items: compiledTools.items },
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
    return new Session(this.#transport, data);
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

  constructor(transport: Transport, data: SessionData) {
    this.#transport = transport;
    this.#data = data;
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

  send(input: SessionInput, options?: RequestOptions): Promise<string>;
  send<Schema extends z.ZodType>(
    input: SessionInput,
    options: OutputOptions<Schema>,
  ): Promise<z.output<Schema>>;
  async send(
    input: SessionInput,
    options: RequestOptions | OutputOptions = {},
  ): Promise<unknown> {
    const outputOptions = isOutputOptions(options) ? options : undefined;
    const compiled =
      outputOptions === undefined
        ? undefined
        : await compileOutputSchema(outputOptions.output, outputOptions.outputRetries);
    const accepted = await this.#transport.json<MessageAccepted>(
      "POST",
      `/v1/sessions/${encodeURIComponent(this.id)}/messages`,
      {
        body: {
          content: input,
          ...(options.metadata === undefined ? {} : { metadata: options.metadata }),
          ...(compiled === undefined
            ? {}
            : {
                output: {
                  schema: compiled.jsonSchema,
                  schema_hash: compiled.schemaHash,
                  ...(compiled.retries === undefined ? {} : { retries: compiled.retries }),
                },
              }),
        },
        headers: { "Idempotency-Key": options.idempotencyKey ?? randomIdempotencyKey() },
        signal: options.signal,
        retry: true,
      },
    );
    if (compiled !== undefined) {
      if (
        accepted.session_id !== this.id ||
        accepted.output_id === undefined ||
        accepted.schema_hash !== compiled.schemaHash
      ) {
        throw new SessionError("Brain returned an inconsistent typed-output admission");
      }
    }

    let answer = "";
    try {
      for await (const event of this.events({ after: Math.max(0, accepted.seq - 1), signal: options.signal })) {
        if (event.type === "assistant.message" && event.turn_id === accepted.turn_id && event.agent_id === "root") {
          answer = event.text;
        } else if (event.type === "turn.failed" && event.turn_id === accepted.turn_id) {
          throw errorFromApi(event.error, undefined, outputIssues(event.error));
        } else if (event.type === "turn.completed" && event.turn_id === accepted.turn_id) {
          this.markIdle();
          if (event.stop_reason === "cancelled") throw new AbortError();
          if (compiled !== undefined && outputOptions !== undefined) {
            if (event.result === undefined) {
              throw new OutputRefusalError(
                "The model ended the turn without submitting the requested structured output",
              );
            }
            if (
              event.result.name !== "brain_submit_output" ||
              event.result.metadata?.output_id !== accepted.output_id ||
              event.result.metadata?.schema_hash !== compiled.schemaHash
            ) {
              throw new SessionError("Brain returned a typed result for a different request");
            }
            const parsed = await outputOptions.output.safeParseAsync(event.result.value);
            if (!parsed.success) {
              throw new OutputValidationError(
                "The output passed the wire schema but failed the original Zod schema",
                parsed.error.issues.map((issue) => ({
                  path: jsonPointer(issue.path),
                  message: issue.message,
                  keyword: issue.code,
                })),
              );
            }
            return parsed.data;
          }
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
  }

  private markIdle(): void {
    this.#data = { ...this.#data, state: "idle" };
  }
}

function isOutputOptions(options: RequestOptions | OutputOptions): options is OutputOptions {
  return "output" in options;
}

async function compileOutputSchema(
  schema: z.ZodType,
  retries: 0 | 1 | 2 | undefined,
): Promise<{ jsonSchema: Record<string, unknown>; schemaHash: string; retries: 0 | 1 | 2 | undefined }> {
  let jsonSchema: Record<string, unknown>;
  try {
    assertPortableOutputSchema(schema);
    jsonSchema = z.toJSONSchema(schema, {
      target: "draft-2020-12",
      unrepresentable: "throw",
    }) as Record<string, unknown>;
  } catch (cause) {
    throw new OutputSchemaError(messageOf(cause, "The Zod schema cannot be represented as JSON Schema"), {
      cause,
    });
  }
  if (jsonSchema.type !== "object") {
    throw new OutputSchemaError("session.send() output requires a Zod object schema");
  }
  return { jsonSchema, schemaHash: await jcsSha256(jsonSchema), retries };
}

function outputIssues(error: ApiError): readonly OutputValidationIssue[] {
  const details = error.details;
  if (details === undefined || details === null || typeof details !== "object") return [];
  const issues = (details as { issues?: unknown }).issues;
  if (!Array.isArray(issues)) return [];
  return issues.flatMap((issue): OutputValidationIssue[] => {
    if (issue === null || typeof issue !== "object") return [];
    const value = issue as { path?: unknown; message?: unknown; keyword?: unknown };
    if (typeof value.path !== "string" || typeof value.message !== "string") return [];
    return [{
      path: value.path,
      message: value.message,
      ...(typeof value.keyword === "string" ? { keyword: value.keyword } : {}),
    }];
  });
}

function jsonPointer(path: readonly PropertyKey[]): string {
  if (path.length === 0) return "";
  return `/${path
    .map((part) => String(part).replaceAll("~", "~0").replaceAll("/", "~1"))
    .join("/")}`;
}

function messageOf(error: unknown, fallback: string): string {
  return error instanceof Error && error.message !== "" ? error.message : fallback;
}

/**
 * A server cannot execute user-defined Zod functions. Reject them before admission instead of
 * committing a value that only the calling process can later discover was invalid.
 */
function assertPortableOutputSchema(schema: z.ZodType): void {
  const seen = new WeakSet<object>();
  const visit = (value: unknown): void => {
    if (value === null || typeof value !== "object" || seen.has(value)) return;
    seen.add(value);
    const internal = value as { _zod?: { def?: unknown } };
    if (internal._zod?.def !== undefined) {
      const definition = internal._zod.def as { type?: unknown; check?: unknown };
      const unsupported =
        definition.type === "custom" ||
        definition.type === "transform" ||
        definition.type === "file" ||
        definition.check === "overwrite";
      if (unsupported) {
        throw new OutputSchemaError(
          `This Zod schema contains ${String(definition.type ?? definition.check)} behavior that cannot be enforced by the Brain output service`,
        );
      }
      visit(definition);
      return;
    }
    if (Array.isArray(value)) {
      for (const item of value) visit(item);
      return;
    }
    for (const child of Object.values(value)) visit(child);
  };
  visit(schema);
}
