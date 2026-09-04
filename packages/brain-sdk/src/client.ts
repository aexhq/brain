import { createHash, randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";

import { AppToolRegistry, type AppToolCall } from "./app.js";
import { ClientToolPump, type PumpTransport } from "./client-pump.js";
import { assertEnvironmentBindable, bindEnvironment, endEnvironment, inspectAgentloop, inspectBoundTool, inspectClientTool, inspectEnvironment, inspectServedTool } from "./extensions.js";
import { BrainError } from "./errors.js";
import type {
  AgentloopAdmission, BoundTool as WireBoundTool, CreateSessionRequest, EnvironmentRequirement,
  EventPage, Session as WireSession, SessionList,
} from "./generated/session.js";
import type {
  Agentloop, BoundTool, CreateSessionOptions, Environment, OperationOptions, ServedTool, SessionEvent, SessionState, SessionStreamEvent, UserInput,
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

  /** A client on the same server and transport, presenting a different bearer —
   * how a join speaks with the share key instead of the API token. */
  withToken(token: string): BrainClient {
    return new BrainClient({ baseUrl: this.baseUrl, token, timeoutMs: this.timeoutMs, fetch: this.transport });
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

  /** The session's live event feed over SSE: the journalled backlog after `after`,
   * then records as they are appended, plus the never-journalled streaming deltas.
   * Ends when the session ends or the server drops a lagging subscriber — iterate
   * again from the last seen sequence to resume. */
  async *stream(sessionId: string, after = 0, signal?: AbortSignal): AsyncGenerator<SessionStreamEvent> {
    yield* this.streamPath(`/v1/sessions/${encodeURIComponent(sessionId)}/events?after=${after}`, signal);
  }

  /** The SSE reader behind `stream`, on any feed path — the events feed and the
   * serve feed carry the same frames. */
  async *streamPath(path: string, signal?: AbortSignal): AsyncGenerator<SessionStreamEvent> {
    const headers = new Headers({ accept: "text/event-stream" });
    if (this.token !== undefined) headers.set("authorization", `Bearer ${this.token}`);
    const response = await this.transport(`${this.baseUrl}${path}`, { headers, signal });
    if (!response.ok || response.body === null) {
      const error = (await response.json().catch(() => ({}))) as Partial<BrainError>;
      throw new BrainError(response.status, error.code ?? "http_error", error.message ?? response.statusText, error.retryable ?? false, error.details);
    }
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
          try { payload = JSON.parse(text); } catch { /* a non-JSON payload stays a string */ }
          yield { ...(id === undefined || id === "" ? {} : { sequence: Number(id) }), type, data: payload };
        }
      }
    } finally {
      await reader.cancel().catch(() => {});
    }
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
    let pump: ClientToolPump | undefined;
    if (compiled.clientTools.length > 0) {
      const registry = new AppToolRegistry();
      for (const { contract, handler } of compiled.clientTools) registry.register(contract, handler);
      pump = new ClientToolPump(this.client, session.session_id, registry, session.last_sequence);
      pump.start();
    }
    return new SessionHandle(this.client, toSessionState(session), [...compiled.environments.keys()], pump);
  }

  async get(sessionId: string): Promise<SessionHandle> {
    const session = await this.client.request<WireSession>("GET", `/v1/sessions/${encodeURIComponent(sessionId)}`);
    return new SessionHandle(this.client, toSessionState(session), []);
  }

  /**
   * Join a session from another process to serve its tools. The share key —
   * `session.shareKey` in the process that created it — is the whole address: it
   * names the session and authorizes serving, nothing else. Register handlers with
   * `serve`; the SDK holds the session's serve feed open, answers each call, and
   * reconnects with backoff after a drop.
   */
  join(shareKey: string): ServeHandle {
    return new ServeHandle(this.client, shareKey);
  }

  async list(): Promise<SessionState[]> {
    const response = await this.client.request<SessionList>("GET", "/v1/sessions");
    return response.sessions.map(toSessionState);
  }
}

const shareKeyPattern = /^sk\.(ses_[A-Za-z0-9]{20,32})\.[0-9a-f]{64}$/u;

/**
 * One joined session: the registration surface for serving its tools from this
 * process. Each `serve` claims that tool's seat on the session's serve feed —
 * last connection wins, so a reloaded page displaces its own dead predecessor.
 */
export class ServeHandle {
  readonly sessionId: string;
  private readonly serveClient: BrainClient;
  private readonly registry = new AppToolRegistry();
  private pump: ClientToolPump | undefined;
  private cursor = 0;
  private closed = false;

  constructor(client: BrainClient, shareKey: string) {
    const match = shareKeyPattern.exec(shareKey);
    if (match === null) throw new TypeError("join needs a session share key (sk.<session>.<mac>)");
    this.sessionId = match[1]!;
    this.serveClient = client.withToken(shareKey);
  }

  /** Register the function that answers this served tool, and (re)connect the feed
   * claiming it. Several `serve` calls share one connection. */
  serve<Input, Output>(tool: ServedTool<Input, Output>, handler: (input: Input, call: AppToolCall) => Output | Promise<Output>): this {
    const served = inspectServedTool(tool);
    if (served === undefined) throw new TypeError("serve takes a served tool — one declared with tool({...}) and no execute");
    if (this.closed) throw new Error("this join is closed");
    this.registry.register(served.contract, handler as (input: unknown, call: AppToolCall) => unknown);
    this.connect();
    return this;
  }

  /** Stop serving: the feed closes and in-flight handlers are cancelled. */
  close(): void {
    this.closed = true;
    this.pump?.stop();
    this.pump = undefined;
  }

  private connect(): void {
    if (this.pump !== undefined) {
      // Reconnect with the wider tool set, keeping the cursor so nothing replays.
      this.cursor = this.pump.position();
      this.pump.stop();
    }
    const client = this.serveClient;
    const sessionId = this.sessionId;
    const tools = encodeURIComponent(this.registry.names().join(","));
    const transport: PumpTransport = {
      // `after=0` means "never seen anything": the serve feed's pending mode. A real
      // cursor resumes exactly, like the events feed.
      stream: (_, after, signal) => client.streamPath(`/v1/sessions/${encodeURIComponent(sessionId)}/serve?tools=${tools}${after > 0 ? `&after=${after}` : ""}`, signal),
      request: (method, path, body, idempotencyKey) => client.request(method, path, body, idempotencyKey),
    };
    this.pump = new ClientToolPump(transport, sessionId, this.registry, this.cursor);
    this.pump.start();
  }
}

export class SessionHandle {
  constructor(private readonly client: BrainClient, public state: SessionState, private readonly environments: readonly Environment[], private readonly pump?: ClientToolPump) {}
  get id(): string { return this.state.id; }
  /** The scoped credential another process joins with (`sessions.join`) to serve
   * this session's tools. It opens the serve feed and answers tool calls — nothing
   * else — so it is safe to hand to a page. */
  get shareKey(): string { return this.state.shareKey; }

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

  /** The live event feed: journalled records after `after` plus streaming deltas,
   * until the session ends or the stream drops (resume from the last sequence). */
  stream(after = 0, signal?: AbortSignal): AsyncGenerator<SessionStreamEvent> {
    return this.client.stream(this.id, after, signal);
  }

  async cancel(operation: OperationOptions = {}): Promise<void> {
    await this.client.request("POST", `/v1/sessions/${encodeURIComponent(this.id)}/cancel`, undefined, keyOf(operation));
  }

  async end(operation: OperationOptions = {}): Promise<SessionState> {
    const session = await this.client.request<WireSession>("POST", `/v1/sessions/${encodeURIComponent(this.id)}/end`, undefined, keyOf(operation));
    this.release();
    return (this.state = toSessionState(session));
  }

  async delete(operation: OperationOptions = {}): Promise<void> {
    await this.client.request("DELETE", `/v1/sessions/${encodeURIComponent(this.id)}`, undefined, keyOf(operation));
    this.release();
  }

  private release(): void {
    this.pump?.stop();
    for (const environment of this.environments) endEnvironment(environment);
  }
}

function compileSession(options: CreateSessionOptions, agentloopIdentity: string): { readonly request: CreateSessionRequest; readonly environments: ReadonlyMap<Environment, string>; readonly clientTools: readonly NonNullable<ReturnType<typeof inspectClientTool>>[] } {
  const environments = new Map<Environment, string>();
  const requirements: EnvironmentRequirement[] = [];
  const tools: WireBoundTool[] = [];
  const clientTools: NonNullable<ReturnType<typeof inspectClientTool>>[] = [];
  const names = new Set<string>();
  for (const selected of options.tools ?? []) {
    const client = inspectClientTool(selected);
    if (client !== undefined) {
      if (names.has(client.definition.name)) throw new TypeError(`Tool name ${client.definition.name} is duplicated`);
      names.add(client.definition.name);
      tools.push({
        name: client.definition.name,
        description: client.definition.description,
        input_schema: structuredClone(client.definition.inputSchema),
        ...(client.definition.outputSchema === undefined ? {} : { output_schema: structuredClone(client.definition.outputSchema) }),
        needs: [],
        binding_names: [],
        hosting: "client",
      });
      clientTools.push(client);
      continue;
    }
    // A served tool is client-hosted on the wire — parked and answered over the API —
    // but this process registers no handler: whoever joins with the share key does.
    const served = inspectServedTool(selected);
    if (served !== undefined) {
      if (names.has(served.definition.name)) throw new TypeError(`Tool name ${served.definition.name} is duplicated`);
      names.add(served.definition.name);
      tools.push({
        name: served.definition.name,
        description: served.definition.description,
        input_schema: structuredClone(served.definition.inputSchema),
        ...(served.definition.outputSchema === undefined ? {} : { output_schema: structuredClone(served.definition.outputSchema) }),
        needs: [],
        binding_names: [],
        hosting: "client",
      });
      continue;
    }
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
      needs: [...bound.needs],
      binding_names: [...bound.bindingNames],
      program: structuredClone(bound.program),
      environment_id: environmentId,
    });
  }
  const agentloop = inspectAgentloop(options.agentloop);
  return { request: {
    agentloop: { identity: agentloopIdentity, configuration: structuredClone(agentloop.configuration) },
    model: { provider: options.model.provider, name: options.model.name, api_key: options.model.apiKey },
    system: options.system ?? "",
    ...(options.responseFormat === undefined ? {} : { response_format: structuredClone(options.responseFormat) }),
    tools,
    environments: requirements,
    // The event id is left behind deliberately: it names an event in the session it came
    // from, and this is a different session, so Brain mints them again.
    ...(options.history === undefined ? {} : { history: options.history.map((event) => ({ sequence: event.sequence, recorded_at_ms: event.recordedAt.getTime(), event_type: event.type, data: event.data })) }),
    ...(options.idleTtlMs === undefined ? {} : { idle_ttl_ms: options.idleTtlMs }),
  }, environments, clientTools };
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
  return Object.freeze({ id: session.session_id, status: session.status, lastSequence: session.last_sequence, shareKey: session.share_key });
}
