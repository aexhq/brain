import { createHash, randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";

import { BrainError } from "./errors.js";
import type {
  AgentloopAdmission,
  BoundTool,
  CreateSessionOptions,
  OperationOptions,
  SessionEvent,
  SessionState,
  WireCreateSessionRequest,
  WireEnvironmentRequirement,
  WireEventPage,
  WireSession,
  WireSessionList,
  WireToolBinding,
  WireToolDefinition,
} from "./types.js";

export interface BrainOptions {
  baseUrl: string;
  token?: string;
  timeoutMs?: number;
  fetch?: typeof globalThis.fetch;
}

export class BrainClient {
  readonly baseUrl: string;
  private readonly token?: string;
  private readonly timeoutMs: number;
  private readonly transport: typeof globalThis.fetch;
  private readonly admitted = new WeakMap<object, Promise<string>>();

  constructor(options: BrainOptions) {
    let end = options.baseUrl.length;
    while (end > 0 && options.baseUrl.charCodeAt(end - 1) === 47) end -= 1;
    this.baseUrl = options.baseUrl.slice(0, end);
    if (this.baseUrl.length === 0) throw new TypeError("baseUrl is required");
    const url = new URL(this.baseUrl);
    if ((url.protocol !== "http:" && url.protocol !== "https:") || url.username !== "" || url.password !== "" || url.search !== "" || url.hash !== "") {
      throw new TypeError("baseUrl must be HTTP(S) without credentials, query, or fragment");
    }
    if (options.token !== undefined && options.token.trim() === "") throw new TypeError("token cannot be empty");
    if (!Number.isSafeInteger(options.timeoutMs ?? 30_000) || (options.timeoutMs ?? 30_000) < 1) {
      throw new TypeError("timeoutMs must be a positive safe integer");
    }
    this.token = options.token;
    this.timeoutMs = options.timeoutMs ?? 30_000;
    this.transport = options.fetch ?? globalThis.fetch;
  }

  async createSession(options: CreateSessionOptions, operation: OperationOptions = {}): Promise<SessionHandle> {
    validateSessionOptions(options);
    const digest = await this.admit(options.agentLoop);
    const request = compileSession(options, digest);
    const session = await this.request<WireSession>("POST", "/v1/sessions", request, keyOf(operation));
    return new SessionHandle(this, toSessionState(session));
  }

  async getSession(sessionId: string): Promise<SessionHandle> {
    const session = await this.request<WireSession>("GET", `/v1/sessions/${encodeURIComponent(sessionId)}`);
    return new SessionHandle(this, toSessionState(session));
  }

  async listSessions(): Promise<SessionState[]> {
    const response = await this.request<WireSessionList>("GET", "/v1/sessions");
    return response.sessions.map(toSessionState);
  }

  async sendMessage(sessionId: string, content: unknown, operation: OperationOptions = {}): Promise<SessionState> {
    const session = await this.request<WireSession>("POST", `/v1/sessions/${encodeURIComponent(sessionId)}/messages`, { content }, keyOf(operation));
    return toSessionState(session);
  }

  async readEvents(sessionId: string, after = 0): Promise<{ events: SessionEvent[]; nextCursor: number }> {
    const page = await this.request<WireEventPage>("GET", `/v1/sessions/${encodeURIComponent(sessionId)}/events?after=${after}`);
    return {
      events: page.events.map((event) => ({
        id: event.event_id,
        sequence: event.sequence,
        recordedAt: new Date(event.recorded_at_ms),
        type: event.event_type,
        data: event.data,
      })),
      nextCursor: page.next_cursor,
    };
  }

  async request<T>(method: string, path: string, body?: unknown, idempotencyKey?: string, contentType = "application/json"): Promise<T> {
    const headers = new Headers({ accept: "application/json" });
    if (body !== undefined) headers.set("content-type", contentType);
    if (this.token !== undefined) headers.set("authorization", `Bearer ${this.token}`);
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

  private admit(agentLoop: CreateSessionOptions["agentLoop"]): Promise<string> {
    const cached = this.admitted.get(agentLoop);
    if (cached !== undefined) return cached;
    const admission = this.loadAndAdmit(agentLoop.package);
    this.admitted.set(agentLoop, admission);
    return admission;
  }

  private async loadAndAdmit(artifact: URL | Uint8Array): Promise<string> {
    const bytes = artifact instanceof Uint8Array
      ? artifact
      : artifact.protocol === "file:"
        ? new Uint8Array(await readFile(artifact))
        : new Uint8Array(await (await this.transport(artifact)).arrayBuffer());
    if (bytes.byteLength === 0) throw new TypeError("AgentLoop package cannot be empty");
    const idempotencyKey = `agent-loop-${createHash("sha256").update(bytes).digest("hex")}`;
    const admission = await this.request<AgentloopAdmission>("POST", "/v1/agentloops", bytes, idempotencyKey, "application/octet-stream");
    if (admission.status !== "admitted") throw new BrainError(400, "agent_loop_rejected", admission.error?.message ?? "AgentLoop was rejected", false, admission.error?.details);
    return admission.digest;
  }
}

export class SessionHandle {
  constructor(private readonly client: BrainClient, public state: SessionState) {}
  get id(): string { return this.state.id; }

  async send(content: unknown, operation: OperationOptions = {}): Promise<SessionState> {
    return (this.state = await this.client.sendMessage(this.id, content, operation));
  }

  events(after = 0): AsyncIterable<SessionEvent> {
    const client = this.client;
    const sessionId = this.id;
    return {
      async *[Symbol.asyncIterator]() {
        let cursor = after;
        for (;;) {
          const page = await client.readEvents(sessionId, cursor);
          for (const event of page.events) yield event;
          if (page.nextCursor === cursor) return;
          cursor = page.nextCursor;
        }
      },
    };
  }

  async cancel(operation: OperationOptions = {}): Promise<void> {
    await this.client.request("POST", `/v1/sessions/${encodeURIComponent(this.id)}/cancel`, undefined, keyOf(operation));
  }

  async end(operation: OperationOptions = {}): Promise<SessionState> {
    const session = await this.client.request<WireSession>("POST", `/v1/sessions/${encodeURIComponent(this.id)}/end`, undefined, keyOf(operation));
    return (this.state = toSessionState(session));
  }

  async delete(operation: OperationOptions = {}): Promise<void> {
    await this.client.request("DELETE", `/v1/sessions/${encodeURIComponent(this.id)}`, undefined, keyOf(operation));
  }
}

function compileSession(options: CreateSessionOptions, agentloopDigest: string): WireCreateSessionRequest {
  const environments = new Map<object, WireEnvironmentRequirement>();
  const definitions: WireToolDefinition[] = [];
  const bindings: WireToolBinding[] = [];
  const names = new Set<string>();
  for (const bound of options.tools ?? []) {
    validateBoundTool(bound);
    if (names.has(bound.tool.definition.name)) throw new TypeError(`Tool name ${bound.tool.definition.name} is duplicated`);
    names.add(bound.tool.definition.name);
    let requirement = environments.get(bound.environment);
    if (requirement === undefined) {
      const lifecycle = bound.environment.lifecycle.type ?? "session";
      const environmentId = lifecycle === "session"
        ? `env_${environments.size + 1}`
        : requiredEnvironmentId(bound.environment);
      const created: WireEnvironmentRequirement = {
        environment_id: environmentId,
        configuration: structuredClone(bound.environment.configuration),
        lifecycle_policy: lifecycle,
      };
      environments.set(bound.environment, created);
      requirement = created;
    }
    const definition = bound.tool.definition;
    definitions.push({
      name: definition.name,
      description: definition.description,
      input_schema: structuredClone(definition.inputSchema),
      ...(definition.outputSchema === undefined ? {} : { output_schema: structuredClone(definition.outputSchema) }),
    });
    bindings.push({
      name: definition.name,
      environment_id: requirement.environment_id,
      remote_tool_id: bound.tool.remoteToolId,
      grant: structuredClone(bound.grant),
    });
  }
  return {
    agentloop_digest: agentloopDigest,
    model: { provider: options.model.provider, name: options.model.name, api_key: options.model.apiKey },
    presentation: {
      system: options.system ?? "",
      tools: definitions,
      ...(options.responseFormat === undefined ? {} : { response_format: structuredClone(options.responseFormat) }),
    },
    environments: [...environments.values()],
    tool_bindings: bindings,
  };
}

function requiredEnvironmentId(environment: BoundTool["environment"]): string {
  if (environment.lifecycle.type === "shared" || environment.lifecycle.type === "external") {
    return environment.lifecycle.id;
  }
  throw new TypeError("a shared or external Environment requires a stable id");
}

function validateSessionOptions(options: CreateSessionOptions): void {
  if (options === null || typeof options !== "object") throw new TypeError("session options are required");
  if (options.agentLoop?.kind !== "agent-loop") throw new TypeError("agentLoop is required");
  if (options.model?.provider !== "vercel-ai-gateway") throw new TypeError("unsupported model provider");
  if (!/^[^/\s]+\/[^/\s][^\s]*$/u.test(options.model.name)) throw new TypeError("model name must include its provider namespace");
  if (typeof options.model.apiKey !== "string" || options.model.apiKey.trim() === "") throw new TypeError("model apiKey is required");
  if (options.tools !== undefined && !Array.isArray(options.tools)) throw new TypeError("tools must be an array");
  if (options.system !== undefined && (typeof options.system !== "string" || options.system.length > 131_072)) throw new TypeError("system exceeds its contract bound");
}

function validateBoundTool(bound: BoundTool): void {
  if (bound?.kind !== "bound-tool" || bound.tool?.kind !== "tool" || bound.environment?.kind !== "environment") {
    throw new TypeError("tools must be configured with runIn");
  }
  if (bound.tool.environmentCapability !== bound.environment.capability) {
    throw new TypeError(`Tool ${bound.tool.definition.name} cannot run in a ${bound.environment.capability} Environment`);
  }
}

function keyOf(options: OperationOptions): string {
  if (options.idempotencyKey !== undefined && options.idempotencyKey.trim() === "") throw new TypeError("idempotencyKey cannot be empty");
  return options.idempotencyKey ?? randomUUID();
}

function toSessionState(session: WireSession): SessionState {
  return Object.freeze({
    id: session.session_id,
    journalId: session.journal_id,
    status: session.status,
    throughSequence: session.through_sequence,
    presentationDigest: session.presentation_digest,
  });
}
