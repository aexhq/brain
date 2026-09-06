import { HostPump } from "./client-pump.js";
import { BrainError } from "./errors.js";
import { inspectAgentloop, inspectComponent, inspectEnvironment, inspectTool, isComponent } from "./extensions.js";
import { HostToolRegistry } from "./host.js";
import type {
  AgentloopAdmission, CreateSessionRequest, Environment as WireEnvironment, EventPage, HostRegistration,
  SessionList, SessionSummary as WireSession, SessionTranscript, Tool as WireTool, ToolAdmission,
} from "./generated/session.js";
import type {
  Component, CreateSessionOptions, Environment, OperationOptions, PlacedAgentloop, PlacedTool,
  SessionEvent, SessionState, SessionStreamEvent, UserInput,
} from "./types.js";

export interface BrainOptions {
  baseUrl: string;
  token?: string;
  timeoutMs?: number;
  fetch?: typeof globalThis.fetch;
  /** A host registration to resume, from `credentials()` of an earlier client. */
  credentials?: HostCredentials;
}

/** What identifies this process as a host across clients and restarts. */
export interface HostCredentials { readonly hostId: string; readonly token: string }

interface Host {
  readonly hostId: string;
  readonly pump: HostPump;
  unregister(sessionId: string): void;
}

export class BrainClient {
  readonly baseUrl: string;
  readonly sessions: Sessions;
  private readonly token?: string;
  private readonly timeoutMs?: number;
  private readonly transport: typeof globalThis.fetch;
  private readonly agentloops = new WeakMap<object, Promise<string>>();
  private readonly tools = new WeakMap<object, Promise<string>>();
  private registration?: HostRegistration;
  private host?: Promise<Host>;

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
    if (options.credentials !== undefined) {
      if (!options.credentials.hostId || !options.credentials.token) throw new TypeError("credentials require hostId and token");
      this.registration = { host_id: options.credentials.hostId, token: options.credentials.token };
    }
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

  async admit(extension: PlacedAgentloop): Promise<string> {
    const source = inspectAgentloop(extension);
    if (inspectEnvironment(source.environment).driver.driver !== "brain" || !isComponent(source.implementation)) throw new TypeError("admission needs a Component placed in brainEnv");
    return this.admitAgentloop(source.implementation);
  }

  async admitAgentloop(value: Component): Promise<string> {
    return this.admitComponent(value, this.agentloops, "/v1/agentloops", "Agentloop");
  }

  async admitTool(value: Component): Promise<string> {
    return this.admitComponent(value, this.tools, "/v1/tools", "Tool");
  }

  /** Registers this process as a host, or resumes the registration the client was
   * given, and keeps its command stream open for as long as a session is placed here. */
  async register(): Promise<Host> {
    if (this.host !== undefined) return this.host;
    const opening = (async () => {
      const registration = this.registration ?? await this.request<HostRegistration>("POST", "/v1/hosts");
      this.registration = registration;
      const hostClient = this.withToken(registration.token);
      const pump = new HostPump({
        stream: (signal, onOpen) => hostClient.streamPath(`/v1/hosts/${encodeURIComponent(registration.host_id)}/commands`, signal, onOpen),
        result: (value) => hostClient.request("POST", `/v1/hosts/${encodeURIComponent(registration.host_id)}/results`, value),
        emit: (value) => hostClient.request<{ sequence: number }>("POST", `/v1/hosts/${encodeURIComponent(registration.host_id)}/events`, value),
      });
      await pump.start();
      void pump.closed.then(() => {
        if (this.host === opening) this.host = undefined;
      });
      return {
        hostId: registration.host_id,
        pump,
        unregister: (sessionId: string) => {
          if (pump.unregister(sessionId) && this.host === opening) {
            this.host = undefined;
          }
        },
      };
    })();
    this.host = opening;
    opening.catch(() => { if (this.host === opening) this.host = undefined; });
    return opening;
  }

  /** This host's registration, to hand a later client so it can resume the sessions
   * placed here. */
  async credentials(): Promise<HostCredentials> {
    await this.register();
    return Object.freeze({ hostId: this.registration!.host_id, token: this.registration!.token });
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
      if (result.status !== "admitted") throw new BrainError(400, `${subject.toLowerCase()}_rejected`, result.error?.message ?? `${subject} was rejected`, false, result.error && "details" in result.error ? result.error.details : undefined);
      return result.id;
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
    const key = keyOf(operation);
    const loop = inspectAgentloop(options.agentloop);
    const environments = collectEnvironments(options);
    const tools = (options.tools ?? []).map((placed) => [placed, inspectTool(placed)] as const);
    for (const [, tool] of tools) {
      const placedIn = inspectEnvironment(tool.environment).driver.driver === "host";
      if (tool.handler !== undefined && !placedIn) throw new TypeError(`Tool ${tool.definition.name} has run and must be placed in a hostEnv`);
      if (tool.handler === undefined && placedIn) throw new TypeError(`Tool ${tool.definition.name} is placed in a hostEnv and must have run`);
    }
    const loopDriver = inspectEnvironment(loop.environment).driver.driver;
    if (isComponent(loop.implementation) && loopDriver !== "brain") throw new TypeError("a remote Agentloop needs its Environment's implementation descriptor");
    const implementation = isComponent(loop.implementation)
      ? { type: "brain_component", entrypoint: "turn", id: await this.client.admitAgentloop(loop.implementation) }
      : structuredClone(loop.implementation);
    const hosted = [...environments.keys()].some((environment) => inspectEnvironment(environment).driver.driver === "host");
    const host = hosted ? await this.client.register() : undefined;
    const implementations = new Map<PlacedTool, unknown>();
    for (const [placed, tool] of tools) {
      if (tool.implementation === undefined) {
        implementations.set(placed, { type: "host_function", name: tool.definition.name });
        continue;
      }
      if (isComponent(tool.implementation) && inspectEnvironment(tool.environment).driver.driver !== "brain") throw new TypeError("a remote Tool needs its Environment's implementation descriptor");
      implementations.set(placed, !isComponent(tool.implementation)
        ? structuredClone(tool.implementation)
        : {
            type: "brain_component",
            entrypoint: "run",
            id: await this.client.admitTool(tool.implementation as Component),
            configuration: structuredClone(tool.configuration),
          });
    }
    const request = compileSession(options, implementation, environments, implementations, host?.hostId);
    const session = await this.client.request<WireSession>("POST", "/v1/sessions", request, key);
    if (host !== undefined) {
      const registry = new HostToolRegistry();
      for (const [, tool] of tools) {
        if (tool.handler !== undefined && tool.contract !== undefined) registry.register(inspectEnvironment(tool.environment).name, tool.contract, tool.handler);
      }
      host.pump.register(session.session_id, registry);
    }
    return new SessionHandle(
      this.client,
      toSessionState(session),
      host === undefined ? undefined : () => host.unregister(session.session_id),
    );
  }

  /** Reopens a session. With `tools`, the functions this host holds are attached again:
   * they must be exactly the Tools the session placed in this host. */
  async get(sessionId: string, options: { tools?: readonly PlacedTool[] } = {}): Promise<SessionHandle> {
    const session = await this.client.request<WireSession>("GET", `/v1/sessions/${encodeURIComponent(sessionId)}`);
    const handle = new SessionHandle(this.client, toSessionState(session));
    if (options.tools === undefined) return handle;
    const tools = options.tools.map((placed) => {
      const tool = inspectTool(placed);
      if (tool.handler === undefined || tool.contract === undefined) throw new TypeError("reattachment accepts only Tools with run");
      return tool;
    });
    const host = await this.client.register();
    for await (const event of handle.events()) {
      if (event.type !== "session_creation_ended") continue;
      const configuration = (event.data as { configuration: { tools: WireTool[]; environments: { name: string; driver: string; host_id?: string }[] } }).configuration;
      const here = configuration.environments.filter((environment) => environment.driver === "host" && environment.host_id === host.hostId).map((environment) => environment.name);
      const placed = configuration.tools.flatMap((tool) => Object.keys(tool.placements).filter((environment) => here.includes(environment)).map((environment) => `${tool.name}\0${environment}`)).sort();
      const supplied = tools.map((tool) => `${tool.definition.name}\0${inspectEnvironment(tool.environment).name}`).sort();
      if (placed.length === 0 || JSON.stringify(placed) !== JSON.stringify(supplied)) {
        throw new TypeError("the Tools supplied must be exactly those the session placed in this host");
      }
      const registry = new HostToolRegistry();
      for (const tool of tools) registry.register(inspectEnvironment(tool.environment).name, tool.contract!, tool.handler!);
      host.pump.register(sessionId, registry);
      return new SessionHandle(this.client, toSessionState(session), () => host.unregister(sessionId));
    }
    throw new TypeError("session has no completed creation record");
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
    private readonly unregisterHost?: () => void,
  ) {}
  get id(): string { return this.state.id; }

  async send(input: UserInput | string, operation: OperationOptions = {}): Promise<SessionState> {
    const normalized = typeof input === "string" ? { message: input } : input;
    if (typeof normalized?.message !== "string" || normalized.message === "") throw new TypeError("send needs a non-empty message");
    const session = await this.client.request<WireSession>("POST", `/v1/sessions/${encodeURIComponent(this.id)}/messages`, { input: normalized }, keyOf(operation));
    return (this.state = toSessionState(session));
  }

  transcript(): Promise<SessionTranscript> {
    return this.client.request("GET", `/v1/sessions/${encodeURIComponent(this.id)}/transcript`);
  }

  events(after = 0): AsyncIterable<SessionEvent> {
    const client = this.client;
    const sessionId = this.id;
    return { async *[Symbol.asyncIterator]() {
      let cursor = after;
      for (;;) {
        const page = await client.request<EventPage>("GET", `/v1/sessions/${encodeURIComponent(sessionId)}/events?after=${cursor}`);
        for (const event of page.events) yield { sequence: event.sequence, recordedAt: new Date(event.recorded_at_ms), type: event.event_type, data: event.data };
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
    this.unregisterHost?.();
    return (this.state = toSessionState(session));
  }

  async delete(operation: OperationOptions = {}): Promise<void> {
    await this.client.request("DELETE", `/v1/sessions/${encodeURIComponent(this.id)}`, undefined, keyOf(operation));
    this.unregisterHost?.();
  }
}

/** Every Environment the session places something in, each once. Two values with one
 * name are the same declaration when they say the same thing, and a contradiction
 * otherwise: one name for two different things is refused before any request. */
function collectEnvironments(options: CreateSessionOptions): ReadonlySet<Environment> {
  const result = new Set<Environment>();
  const names = new Map<string, Environment>();
  const add = (environment: Environment) => {
    const source = inspectEnvironment(environment);
    const known = names.get(source.name);
    if (known !== undefined) {
      if (JSON.stringify(inspectEnvironment(known)) !== JSON.stringify(source)) throw new TypeError(`two Environments are named ${source.name}`);
      return;
    }
    names.set(source.name, environment);
    result.add(environment);
  };
  add(inspectAgentloop(options.agentloop).environment);
  for (const selected of options.tools ?? []) add(inspectTool(selected).environment);
  return result;
}

function compileSession(
  options: CreateSessionOptions,
  agentloopImplementation: unknown,
  environments: ReadonlySet<Environment>,
  implementations: ReadonlyMap<PlacedTool, unknown>,
  hostId?: string,
): CreateSessionRequest {
  const entries: WireEnvironment[] = [...environments].map((environment) => {
    const source = inspectEnvironment(environment);
    const configuration = structuredClone(source.configuration) as WireEnvironment["configuration"];
    switch (source.driver.driver) {
      case "brain":
        return { name: source.name, driver: "brain", configuration };
      case "host":
        return { name: source.name, driver: "host", host_id: hostId!, configuration };
      case "http":
        return {
          name: source.name,
          driver: "http",
          url: source.driver.url,
          ...(source.driver.credential === undefined ? {} : { credential: source.driver.credential }),
          configuration,
        };
    }
  });
  const tools = new Map<string, WireTool>();
  for (const selected of options.tools ?? []) {
    const tool = inspectTool(selected);
    const definition = {
      name: tool.definition.name, description: tool.definition.description,
      input_schema: structuredClone(tool.definition.inputSchema),
      ...(tool.definition.outputSchema === undefined ? {} : { output_schema: structuredClone(tool.definition.outputSchema) }),
    };
    const environment = inspectEnvironment(tool.environment).name;
    const known = tools.get(definition.name);
    if (known !== undefined) {
      const { placements: _placements, ...canonical } = known;
      if (JSON.stringify(canonical) !== JSON.stringify(definition)) throw new TypeError(`Tool ${definition.name} has conflicting definitions`);
      if (Object.hasOwn(known.placements, environment)) throw new TypeError(`Tool ${definition.name} is duplicated in Environment ${environment}`);
    }
    const entry = known ?? { ...definition, placements: Object.create(null) as WireTool["placements"] };
    entry.placements[environment] = { needs: [...tool.needs], implementation: structuredClone(implementations.get(selected)) };
    tools.set(definition.name, entry);
  }
  const loop = inspectAgentloop(options.agentloop);
  return {
    agentloop: {
      implementation: structuredClone(agentloopImplementation),
      configuration: structuredClone(loop.configuration),
      environment: inspectEnvironment(loop.environment).name,
      needs: [...loop.needs],
    },
    model: { provider: options.model.provider, name: options.model.name, api_key: options.model.apiKey },
    system: options.system ?? "",
    ...(options.responseFormat === undefined ? {} : { response_format: structuredClone(options.responseFormat) as CreateSessionRequest["response_format"] }),
    tools: [...tools.values()],
    environments: entries,
    ...(options.transcript === undefined ? {} : { transcript: structuredClone(options.transcript) as CreateSessionRequest["transcript"] }),
    ...(options.idleTtlMs === undefined ? {} : { idle_ttl_ms: options.idleTtlMs }),
  };
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
