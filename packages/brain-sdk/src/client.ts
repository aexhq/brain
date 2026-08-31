import { createHash, randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";

import { assertEnvironmentBindable, bindEnvironment, endEnvironment, inspectAgentloop, inspectBoundTool, inspectEnvironment } from "./extensions.js";
import { BrainError } from "./errors.js";
import type {
  AgentloopAdmission, BoundTool as WireBoundTool, CreateSessionRequest, EnvironmentRequirement,
  EventPage, Session as WireSession, SessionList,
} from "./generated/session.js";
import type {
  Agentloop, BoundTool, CreateSessionOptions, Environment, OperationOptions, SessionEvent, SessionState,
} from "./types.js";

export interface BrainOptions {
  baseUrl: string;
  token?: string;
  timeoutMs?: number;
  fetch?: typeof globalThis.fetch;
}

export class BrainClient {
  readonly baseUrl: string;
  readonly sessions: Sessions;
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
    if ((url.protocol !== "http:" && url.protocol !== "https:") || url.username !== "" || url.password !== "" || url.search !== "" || url.hash !== "") throw new TypeError("baseUrl must be HTTP(S) without credentials, query, or fragment");
    if (options.token !== undefined && options.token.trim() === "") throw new TypeError("token cannot be empty");
    if (!Number.isSafeInteger(options.timeoutMs ?? 30_000) || (options.timeoutMs ?? 30_000) < 1) throw new TypeError("timeoutMs must be a positive safe integer");
    this.token = options.token;
    this.timeoutMs = options.timeoutMs ?? 30_000;
    this.transport = options.fetch ?? globalThis.fetch;
    this.sessions = new Sessions(this);
    Object.freeze(this.sessions);
  }

  async callEnvironment(sessionId: string, environmentId: string, name: string, input: unknown, operation: OperationOptions = {}): Promise<unknown> {
    const response = await this.request<{ output: unknown }>("POST", `/v1/sessions/${encodeURIComponent(sessionId)}/environments/${encodeURIComponent(environmentId)}/calls/${encodeURIComponent(name)}`, { input }, keyOf(operation));
    return response.output;
  }

  async request<T>(method: string, path: string, body?: unknown, idempotencyKey?: string, contentType = "application/json"): Promise<T> {
    const headers = new Headers({ accept: "application/json" });
    if (body !== undefined) headers.set("content-type", contentType);
    if (this.token !== undefined) headers.set("authorization", `Bearer ${this.token}`);
    if (idempotencyKey !== undefined) headers.set("idempotency-key", idempotencyKey);
    const response = await this.transport(`${this.baseUrl}${path}`, {
      method, headers,
      body: body === undefined ? undefined : body instanceof Uint8Array ? new Uint8Array(body).buffer : JSON.stringify(body),
      signal: AbortSignal.timeout(this.timeoutMs),
    });
    if (!response.ok) {
      const error = (await response.json().catch(() => ({}))) as Partial<BrainError>;
      throw new BrainError(response.status, error.code ?? "http_error", error.message ?? response.statusText, error.retryable ?? false, error.details);
    }
    return (response.status === 204 ? undefined : await response.json()) as T;
  }

  async admit(extension: Agentloop): Promise<string> {
    const cached = this.admitted.get(extension);
    if (cached !== undefined) return cached;
    const admission = this.loadAndAdmit(inspectAgentloop(extension).artifact);
    this.admitted.set(extension, admission);
    // A rejection is not an admission: forget it, so a transient fetch or server
    // failure does not poison this agentloop for the life of the client.
    admission.catch(() => this.admitted.delete(extension));
    return admission;
  }

  private async loadAndAdmit(artifact: URL | Uint8Array): Promise<string> {
    const bytes = artifact instanceof Uint8Array ? artifact : artifact.protocol === "file:"
      ? new Uint8Array(await readFile(artifact))
      : new Uint8Array(await (await this.transport(artifact)).arrayBuffer());
    if (bytes.byteLength === 0) throw new TypeError("Agentloop package cannot be empty");
    const idempotencyKey = `agentloop-${createHash("sha256").update(bytes).digest("hex")}`;
    const admission = await this.request<AgentloopAdmission>("POST", "/v1/agentloops", bytes, idempotencyKey, "application/octet-stream");
    if (admission.status !== "admitted") throw new BrainError(400, "agentloop_rejected", admission.error?.message ?? "Agentloop was rejected", false, admission.error?.details);
    return admission.identity;
  }
}

export class Sessions {
  constructor(private readonly client: BrainClient) {}

  async create(options: CreateSessionOptions, operation: OperationOptions = {}): Promise<SessionHandle> {
    validateSessionOptions(options);
    const identity = await this.client.admit(options.agentloop);
    const compiled = compileSession(options, identity);
    for (const environment of compiled.environments.keys()) assertEnvironmentBindable(environment);
    const session = await this.client.request<WireSession>("POST", "/v1/sessions", compiled.request, keyOf(operation));
    for (const [environment, environmentId] of compiled.environments) bindEnvironment(environment, this.client, session.session_id, environmentId);
    return new SessionHandle(this.client, toSessionState(session), [...compiled.environments.keys()]);
  }

  async get(sessionId: string): Promise<SessionHandle> {
    const session = await this.client.request<WireSession>("GET", `/v1/sessions/${encodeURIComponent(sessionId)}`);
    return new SessionHandle(this.client, toSessionState(session), []);
  }

  async list(): Promise<SessionState[]> {
    const response = await this.client.request<SessionList>("GET", "/v1/sessions");
    return response.sessions.map(toSessionState);
  }
}

export class SessionHandle {
  constructor(private readonly client: BrainClient, public state: SessionState, private readonly environments: readonly Environment[]) {}
  get id(): string { return this.state.id; }

  async send(content: unknown, operation: OperationOptions = {}): Promise<SessionState> {
    const session = await this.client.request<WireSession>("POST", `/v1/sessions/${encodeURIComponent(this.id)}/messages`, { content }, keyOf(operation));
    return (this.state = toSessionState(session));
  }

  events(after = 0): AsyncIterable<SessionEvent> {
    const client = this.client;
    const sessionId = this.id;
    return { async *[Symbol.asyncIterator]() {
      let cursor = after;
      for (;;) {
        const page = await client.request<EventPage>("GET", `/v1/sessions/${encodeURIComponent(sessionId)}/events?after=${cursor}`);
        for (const event of page.events) yield { id: event.event_id, sequence: event.sequence, recordedAt: new Date(event.recorded_at_ms), type: event.event_type, data: event.data };
        if (page.next_cursor === cursor) return;
        cursor = page.next_cursor;
      }
    } };
  }

  async cancel(operation: OperationOptions = {}): Promise<void> {
    await this.client.request("POST", `/v1/sessions/${encodeURIComponent(this.id)}/cancel`, undefined, keyOf(operation));
  }

  async end(operation: OperationOptions = {}): Promise<SessionState> {
    const session = await this.client.request<WireSession>("POST", `/v1/sessions/${encodeURIComponent(this.id)}/end`, undefined, keyOf(operation));
    this.endEnvironments();
    return (this.state = toSessionState(session));
  }

  async delete(operation: OperationOptions = {}): Promise<void> {
    await this.client.request("DELETE", `/v1/sessions/${encodeURIComponent(this.id)}`, undefined, keyOf(operation));
    this.endEnvironments();
  }

  private endEnvironments(): void { for (const environment of this.environments) endEnvironment(environment); }
}

function compileSession(options: CreateSessionOptions, agentloopIdentity: string): { readonly request: CreateSessionRequest; readonly environments: ReadonlyMap<Environment, string> } {
  const environments = new Map<Environment, string>();
  const requirements: EnvironmentRequirement[] = [];
  const tools: WireBoundTool[] = [];
  const names = new Set<string>();
  for (const selected of options.tools ?? []) {
    const bound = inspectBoundTool(selected as BoundTool);
    if (names.has(bound.definition.name)) throw new TypeError(`Tool name ${bound.definition.name} is duplicated`);
    names.add(bound.definition.name);
    let environmentId = environments.get(bound.environment);
    if (environmentId === undefined) {
      environmentId = `env_${environments.size + 1}`;
      environments.set(bound.environment, environmentId);
      requirements.push({ environment_id: environmentId, configuration: structuredClone(inspectEnvironment(bound.environment).configuration), lifecycle_policy: "session" });
    }
    tools.push({
      name: bound.definition.name,
      description: bound.definition.description,
      input_schema: structuredClone(bound.definition.inputSchema),
      ...(bound.definition.outputSchema === undefined ? {} : { output_schema: structuredClone(bound.definition.outputSchema) }),
      environment_id: environmentId,
      remote_tool_id: bound.implementationName,
      configuration: structuredClone(bound.configuration),
      grant: {},
    });
  }
  const agentloop = inspectAgentloop(options.agentloop);
  return { request: {
    agentloop: { identity: agentloopIdentity, configuration: structuredClone(agentloop.configuration) },
    model: { provider: options.model.provider, name: options.model.name, api_key: options.model.apiKey },
    system: options.system ?? "",
    tools,
    ...(options.responseFormat === undefined ? {} : { response_format: structuredClone(options.responseFormat) }),
    environments: requirements,
    // The event id is left behind deliberately: it names an event in the session it came
    // from, and this is a different session, so Brain mints them again.
    ...(options.history === undefined ? {} : { history: options.history.map((event) => ({ sequence: event.sequence, recorded_at_ms: event.recordedAt.getTime(), event_type: event.type, data: event.data })) }),
  }, environments };
}

function validateSessionOptions(options: CreateSessionOptions): void {
  if (options === null || typeof options !== "object") throw new TypeError("session options are required");
  inspectAgentloop(options.agentloop);
  // Shape only, mirroring the contract's Identifier: which providers exist is
  // the server deployment's registry, so an unknown one fails there, not here.
  const provider = options.model?.provider;
  if (typeof provider !== "string" || !/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(provider)) throw new TypeError("model provider is invalid");
  if (provider === "vercel-ai-gateway" && !/^[^/\s]+\/[^/\s][^\s]*$/u.test(options.model.name)) throw new TypeError("model name must include its provider namespace");
  if (typeof options.model.name !== "string" || options.model.name.length === 0 || options.model.name.length > 256 || /\s/u.test(options.model.name)) throw new TypeError("model name is invalid");
  if (typeof options.model.apiKey !== "string" || options.model.apiKey.trim() === "") throw new TypeError("model apiKey is required");
  if (options.tools !== undefined && !Array.isArray(options.tools)) throw new TypeError("tools must be an array");
  if (options.system !== undefined && (typeof options.system !== "string" || options.system.length > 131_072)) throw new TypeError("system exceeds its contract bound");
}

function keyOf(options: OperationOptions): string {
  if (options.idempotencyKey !== undefined && options.idempotencyKey.trim() === "") throw new TypeError("idempotencyKey cannot be empty");
  return options.idempotencyKey ?? randomUUID();
}
function toSessionState(session: WireSession): SessionState {
  return Object.freeze({ id: session.session_id, journalId: session.journal_id, status: session.status, lastSequence: session.last_sequence, configHash: session.config_hash });
}
