import { AppToolRegistry } from "./app.js";
import { ResidentHostPump } from "./client-pump.js";
import {
  inspectAgentloop, inspectComponent, inspectEnvironment, inspectPlacedTool, inspectResidentTool,
} from "./extensions.js";
import { BrainError } from "./errors.js";
import type {
  AgentloopAdmission, BoundTool as WireBoundTool, CreateSessionRequest, EventPage,
  HostRegistration, SessionEnvironment, SessionSummary as WireSession, SessionList,
} from "./generated/session.js";
import type {
  AgentloopBinding, Component, CreateSessionOptions, Environment, OperationOptions, SessionEvent,
  SessionState, SessionStreamEvent, ToolBinding, UserInput,
} from "./types.js";

interface ToolAdmission {
  readonly identity: string;
  readonly status: "admitted" | "rejected";
  readonly error?: { readonly message: string; readonly details?: unknown };
}

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
  private readonly timeoutMs?: number;
  private readonly transport: typeof globalThis.fetch;
  private readonly agentloops = new WeakMap<object, Promise<string>>();
  private readonly tools = new WeakMap<object, Promise<string>>();
  private resident?: Promise<{
    readonly hostId: string;
    readonly pump: ResidentHostPump;
    unregister(sessionId: string): void;
  }>;

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
    if (options.timeoutMs !== undefined && (!Number.isSafeInteger(options.timeoutMs) || options.timeoutMs < 1)) throw new TypeError("timeoutMs must be a positive safe integer");
    this.token = options.token;
    this.timeoutMs = options.timeoutMs;
    this.transport = options.fetch ?? globalThis.fetch;
    this.sessions = new Sessions(this);
    Object.freeze(this.sessions);
  }

  withToken(token: string): BrainClient {
    return new BrainClient({ baseUrl: this.baseUrl, token, timeoutMs: this.timeoutMs, fetch: this.transport });
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
      ...(this.timeoutMs === undefined ? {} : { signal: AbortSignal.timeout(this.timeoutMs) }),
    });
    if (!response.ok) {
      const error = (await response.json().catch(() => ({}))) as Partial<BrainError>;
      throw new BrainError(response.status, error.code ?? "http_error", error.message ?? response.statusText, error.retryable ?? false, error.details);
    }
    return (response.status === 204 ? undefined : await response.json()) as T;
  }

  async *stream(sessionId: string, after = 0, signal?: AbortSignal): AsyncGenerator<SessionStreamEvent> {
    yield* this.streamPath(`/v1/sessions/${encodeURIComponent(sessionId)}/events?after=${after}`, signal);
  }

  async *streamPath(path: string, signal?: AbortSignal, onOpen?: () => void): AsyncGenerator<SessionStreamEvent> {
    const headers = new Headers({ accept: "text/event-stream" });
    if (this.token !== undefined) headers.set("authorization", `Bearer ${this.token}`);
    const response = await this.transport(`${this.baseUrl}${path}`, { headers, signal });
    if (!response.ok || response.body === null) {
      const error = (await response.json().catch(() => ({}))) as Partial<BrainError>;
      throw new BrainError(response.status, error.code ?? "http_error", error.message ?? response.statusText, error.retryable ?? false, error.details);
    }
    onOpen?.();
    const reader = response.body.getReader();
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
          yield { ...(id === undefined || id === "" ? {} : { sequence: Number(id) }), type, data: payload };
        }
      }
    } finally {
      await reader.cancel().catch(() => {});
    }
  }

  async admit(extension: AgentloopBinding): Promise<string> {
    return this.admitAgentloop(inspectAgentloop(extension).component);
  }

  async admitAgentloop(value: Component): Promise<string> {
    return this.admitComponent(value, this.agentloops, "/v1/agentloops", "Agentloop");
  }

  async admitTool(value: Component): Promise<string> {
    return this.admitComponent(value, this.tools, "/v1/tools", "Tool");
  }

  async residentHost(): Promise<{
    readonly hostId: string;
    readonly pump: ResidentHostPump;
    unregister(sessionId: string): void;
  }> {
    if (this.resident !== undefined) return this.resident;
    const opening = (async () => {
      const registration = await this.request<HostRegistration>("POST", "/v1/hosts");
      const hostClient = this.withToken(registration.token);
      const pump = new ResidentHostPump({
        stream: (signal, onOpen) => hostClient.streamPath(`/v1/hosts/${encodeURIComponent(registration.host_id)}/commands`, signal, onOpen),
        result: (value) => hostClient.request("POST", `/v1/hosts/${encodeURIComponent(registration.host_id)}/results`, value),
        emit: (value) => hostClient.request<{ sequence: number }>("POST", `/v1/hosts/${encodeURIComponent(registration.host_id)}/events`, value),
      });
      await pump.start();
      void pump.closed.then(() => {
        if (this.resident === opening) this.resident = undefined;
      });
      return {
        hostId: registration.host_id,
        pump,
        unregister: (sessionId: string) => {
          if (pump.unregister(sessionId) && this.resident === opening) {
            this.resident = undefined;
          }
        },
      };
    })();
    this.resident = opening;
    opening.catch(() => { if (this.resident === opening) this.resident = undefined; });
    return opening;
  }

  private async admitComponent(value: Component, cache: WeakMap<object, Promise<string>>, path: string, subject: string): Promise<string> {
    const source = inspectComponent(value);
    const cached = cache.get(value);
    if (cached !== undefined) return cached;
    const admission = (async () => {
      const bytes = source.artifact instanceof Uint8Array
        ? source.artifact
        : source.artifact.protocol === "file:"
          ? new Uint8Array(await (await import("node:fs/promises")).readFile(source.artifact))
          : new Uint8Array(await (await this.transport(source.artifact)).arrayBuffer());
      if (bytes.byteLength === 0) throw new TypeError(`${subject} Component cannot be empty`);
      const idempotencyKey = `${subject.toLowerCase()}-${await sha256(bytes)}`;
      const result = await this.request<AgentloopAdmission | ToolAdmission>("POST", path, bytes, idempotencyKey, "application/octet-stream");
      if (result.status !== "admitted") throw new BrainError(400, `${subject.toLowerCase()}_rejected`, result.error?.message ?? `${subject} was rejected`, false, result.error?.details);
      return result.identity;
    })();
    cache.set(value, admission);
    admission.catch(() => cache.delete(value));
    return admission;
  }
}

export class Sessions {
  constructor(private readonly client: BrainClient) {}

  async create(options: CreateSessionOptions, operation: OperationOptions = {}): Promise<SessionHandle> {
    validateSessionOptions(options);
    const loop = inspectAgentloop(options.agentloop);
    const identity = await this.client.admitAgentloop(loop.component);
    const environments = collectEnvironments(options);
    const residentTools = (options.tools ?? []).map(inspectResidentTool).filter((value) => value !== undefined);
    const resident = residentTools.length === 0 ? undefined : await this.client.residentHost();
    const implementations = new Map<ToolBinding, unknown>();
    for (const selected of options.tools ?? []) {
      if (inspectResidentTool(selected) !== undefined) continue;
      const placed = inspectPlacedTool(selected);
      const componentSource = placed.implementation !== null && typeof placed.implementation === "object"
        ? (() => { try { return inspectComponent(placed.implementation as Component); } catch { return undefined; } })()
        : undefined;
      implementations.set(selected, componentSource === undefined
        ? structuredClone(placed.implementation)
        : {
            type: "brain_component",
            identity: await this.client.admitTool(placed.implementation as Component),
            configuration: structuredClone(placed.configuration),
          });
    }
    const compiled = compileSession(options, identity, environments, implementations, resident?.hostId);
    const session = await this.client.request<WireSession>("POST", "/v1/sessions", compiled.request, keyOf(operation));
    if (resident !== undefined) {
      const registry = new AppToolRegistry();
      for (const tool of residentTools) registry.register(tool.contract, tool.handler);
      resident.pump.register(session.session_id, registry);
    }
    return new SessionHandle(
      this.client,
      toSessionState(session),
      resident === undefined ? undefined : () => resident.unregister(session.session_id),
    );
  }

  async get(sessionId: string): Promise<SessionHandle> {
    const session = await this.client.request<WireSession>("GET", `/v1/sessions/${encodeURIComponent(sessionId)}`);
    return new SessionHandle(this.client, toSessionState(session));
  }

  async list(): Promise<SessionState[]> {
    const response = await this.client.request<SessionList>("GET", "/v1/sessions");
    return response.sessions.map(toSessionState);
  }
}

export class SessionHandle {
  constructor(
    private readonly client: BrainClient,
    public state: SessionState,
    private readonly unregisterResident?: () => void,
  ) {}
  get id(): string { return this.state.id; }

  async send(input: UserInput | string, operation: OperationOptions = {}): Promise<SessionState> {
    const normalized = typeof input === "string" ? { message: input } : input;
    if (typeof normalized?.message !== "string" || normalized.message === "") throw new TypeError("send needs a non-empty message");
    const session = await this.client.request<WireSession>("POST", `/v1/sessions/${encodeURIComponent(this.id)}/messages`, { input: normalized }, keyOf(operation));
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

  stream(after = 0, signal?: AbortSignal): AsyncGenerator<SessionStreamEvent> {
    return this.client.stream(this.id, after, signal);
  }

  async cancel(operation: OperationOptions = {}): Promise<void> {
    await this.client.request("POST", `/v1/sessions/${encodeURIComponent(this.id)}/cancel`, undefined, keyOf(operation));
  }

  async end(operation: OperationOptions = {}): Promise<SessionState> {
    const session = await this.client.request<WireSession>("POST", `/v1/sessions/${encodeURIComponent(this.id)}/end`, undefined, keyOf(operation));
    this.unregisterResident?.();
    return (this.state = toSessionState(session));
  }

  async delete(operation: OperationOptions = {}): Promise<void> {
    await this.client.request("DELETE", `/v1/sessions/${encodeURIComponent(this.id)}`, undefined, keyOf(operation));
    this.unregisterResident?.();
  }
}

function collectEnvironments(options: CreateSessionOptions): ReadonlyMap<Environment, string> {
  const result = new Map<Environment, string>();
  const add = (environment: Environment) => {
    inspectEnvironment(environment);
    if (!result.has(environment)) result.set(environment, `env_${crypto.randomUUID().replaceAll("-", "")}`);
  };
  add(inspectAgentloop(options.agentloop).environment);
  for (const selected of options.tools ?? []) {
    if (inspectResidentTool(selected) === undefined) add(inspectPlacedTool(selected).environment);
  }
  return result;
}

function compileSession(
  options: CreateSessionOptions,
  agentloopIdentity: string,
  environmentIds: ReadonlyMap<Environment, string>,
  implementations: ReadonlyMap<ToolBinding, unknown>,
  residentHostId?: string,
): { readonly request: CreateSessionRequest } {
  const requirements: SessionEnvironment[] = [...environmentIds].map(([environment, environmentId]) => {
    const source = inspectEnvironment(environment);
    return {
      environment_id: environmentId,
      configuration: structuredClone(source.configuration),
      managed: source.managed,
      ...(source.idleTtlMs === undefined ? {} : { idle_ttl_ms: source.idleTtlMs }),
      bindings: { ...source.bindings },
    };
  });
  const tools: WireBoundTool[] = [];
  const names = new Set<string>();
  for (const selected of options.tools ?? []) {
    const resident = inspectResidentTool(selected);
    const placed = resident === undefined ? inspectPlacedTool(selected) : undefined;
    const definition = resident?.definition ?? placed!.definition;
    if (names.has(definition.name)) throw new TypeError(`Tool name ${definition.name} is duplicated`);
    names.add(definition.name);
    tools.push({
      name: definition.name,
      description: definition.description,
      input_schema: structuredClone(definition.inputSchema),
      ...(definition.outputSchema === undefined ? {} : { output_schema: structuredClone(definition.outputSchema) }),
      ...(resident === undefined
        ? {
            needs: [...placed!.needs],
            binding_names: [...placed!.bindingNames],
            hosting: "provisioned" as const,
            implementation: structuredClone(implementations.get(selected)),
            environment_id: environmentIds.get(placed!.environment)!,
          }
        : {
            needs: [],
            binding_names: [],
            hosting: "resident" as const,
            host_id: residentHostId!,
          }),
    });
  }
  const loop = inspectAgentloop(options.agentloop);
  const request = {
    agentloop: {
      identity: agentloopIdentity,
      configuration: structuredClone(loop.configuration),
      environment_id: environmentIds.get(loop.environment)!,
    },
    model: { provider: options.model.provider, name: options.model.name, api_key: options.model.apiKey },
    system: options.system ?? "",
    ...(options.responseFormat === undefined ? {} : { response_format: structuredClone(options.responseFormat) }),
    tools,
    environments: requirements,
    ...(options.transcript === undefined ? {} : { transcript: structuredClone(options.transcript) }),
    ...(options.idleTtlMs === undefined ? {} : { idle_ttl_ms: options.idleTtlMs }),
  } as CreateSessionRequest;
  return { request };
}

function validateSessionOptions(options: CreateSessionOptions): void {
  if (options === null || typeof options !== "object") throw new TypeError("session options are required");
  inspectAgentloop(options.agentloop);
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
  return options.idempotencyKey ?? crypto.randomUUID();
}

async function sha256(bytes: Uint8Array): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", Uint8Array.from(bytes).buffer));
  return [...digest].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function toSessionState(session: WireSession): SessionState {
  return Object.freeze({ id: session.session_id, status: session.status, lastSequence: session.last_sequence });
}
