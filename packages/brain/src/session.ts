import type {
  CreateSessionRequest,
  Event,
  MessageAccepted,
  RetentionUpdate,
  Session as SessionData,
  SessionList as SessionListData,
  SessionState,
  ToolDefinition,
} from "./generated/session.js";
import { prepareComponents, type ComponentExtension } from "./components.js";

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
import { CustomerEnvironment, type WebSocketFactory } from "./customer.js";
import { SessionChildren, SessionSandbox, SessionStorage } from "./resources.js";

export type SessionInput = string;

export interface ModelOptions {
  component: ComponentExtension<"model">;
  /** Usage-projection provenance. Defaults to the component's metadata name. */
  provider?: string;
  name: string;
  apiKey: string;
  baseUrl?: string;
  maxOutputTokens?: number;
  temperature?: number;
  reasoningEffort?: "low" | "medium" | "high";
}

export interface CreateSessionOptions {
  model: ModelOptions;
  agentloop: ComponentExtension<"agentloop">;
  /** Omitted or empty grants no tools. A non-empty list is the exact grant. */
  tools?: readonly SessionTool[];
  /** Logical Environment components. A Tool environment grant is inferred only with one entry. */
  environments?: Readonly<Record<string, ComponentExtension<"environment">>>;
  systemPrompt?: string;
  /** Write-only values for environment names declared by managed Tools. */
  secrets?: Record<string, string>;
  /** Maximum direct network authority available to managed sandboxes in this session. */
  network?: NetworkPolicy;
  /** Replacement attempts after an unrecoverable provider outcome. Defaults to one. */
  providerRecoveryRetries?: number;
  client?: {
    /** Replacement sends to the same customer process/operation. Defaults to one. */
    submitRetries?: number;
  };
  metadata?: Record<string, string>;
  /** Finite durable-history deadline. Omit to use the Brain deployment default. */
  retainUntil?: Date | string;
}

export type ComponentToolConfig = Readonly<Record<string, unknown>> & {
  readonly definition: ToolDefinition;
};

export type SessionTool = Tool | ComponentExtension<"tool", ComponentToolConfig>;

export type NetworkDestination =
  | { host: string; ports: [443]; protocol: "tls" }
  | { cidr: string; ports: [number, ...number[]]; protocol: "tcp" };

export type NetworkPolicy =
  | { outbound: "none" | "public" }
  | { outbound: "allowlist"; destinations: [NetworkDestination, ...NetworkDestination[]] };

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
  provider: string;
  componentDigest: string;
  world: string;
  name: string;
  baseUrl?: string;
}

export interface SessionSummary {
  id: string;
  state: SessionState;
  model: ModelSummary;
  createdAt: string;
  updatedAt: string;
  retainUntil: string;
  metadata: Readonly<Record<string, string | undefined>>;
}

export class Sessions {
  readonly #transport: Transport;
  readonly #webSocketFactory: WebSocketFactory | undefined;
  readonly #clientId: string | undefined;
  #customerEnvironment: Promise<CustomerEnvironment> | undefined;
  #closed = false;

  constructor(transport: Transport, webSocketFactory?: WebSocketFactory, clientId?: string) {
    this.#transport = transport;
    this.#webSocketFactory = webSocketFactory;
    this.#clientId = clientId;
  }

  async create(options: CreateSessionOptions, request: RequestOptions = {}): Promise<Session> {
    if (this.#closed) throw new SessionError("Brain client is closed");
    const selections = options.tools ?? [];
    const componentTools = selections.filter(isComponentTool);
    const legacyTools = selections.filter((value): value is Tool => !isComponentTool(value));
    const compiledTools = await compileTools(legacyTools);
    const environments = Object.entries(options.environments ?? {});
    const components = await prepareComponents([
      options.model.component,
      options.agentloop,
      ...componentTools,
      ...environments.map(([, environment]) => environment),
    ]);
    const modelComponent = components.bindings[0];
    const agentloop = components.bindings[1];
    if (modelComponent === undefined || agentloop === undefined) {
      throw new TypeError("Session Model and Agentloop components are required");
    }
    const provider = options.model.provider ?? options.model.component.metadata.name;
    if (provider === undefined) {
      throw new TypeError("Model provider provenance requires model.provider or component metadata.name");
    }
    const componentToolBindings = components.bindings.slice(2, 2 + componentTools.length);
    const environmentBindings = components.bindings.slice(2 + componentTools.length);
    let legacyIndex = 0;
    let componentIndex = 0;
    const toolItems = selections.map((selection) => {
      if (!isComponentTool(selection)) return compiledTools.items[legacyIndex++];
      const binding = componentToolBindings[componentIndex++];
      if (binding === undefined) throw new TypeError("Tool component binding is missing");
      const definition = componentToolDefinition(selection);
      const needsEnvironment = binding.grants.includes("environment");
      if (needsEnvironment && environments.length !== 1) {
        throw new TypeError("A Tool with the environment grant requires exactly one declared Environment");
      }
      return {
        definition,
        executor: {
          kind: "component" as const,
          component_digest: binding.component_digest,
          world: binding.world,
          config: binding.config,
          grants: binding.grants,
          ...(needsEnvironment ? { environment: environments[0]![0] } : {}),
        },
      };
    });
    const environmentConfig = Object.fromEntries(environments.map(([name], index) => {
      const binding = environmentBindings[index];
      if (binding === undefined) throw new TypeError(`Environment component binding ${name} is missing`);
      return [name, {
        component_digest: binding.component_digest,
        world: binding.world,
        config: binding.config,
      }];
    }));
    await this.#ensureCustomerEnvironment(compiledTools.clientRegistrations, request.signal);
    const body = {
      model: {
        component_digest: modelComponent.component_digest,
        world: modelComponent.world,
        config: modelComponent.config,
        provider,
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
      agentloop: {
        component_digest: agentloop.component_digest,
        world: agentloop.world,
        config: agentloop.config,
      },
      component_artifacts: components.artifacts,
      tools: {
        items: toolItems,
      },
      ...(environments.length === 0 ? {} : { environments: environmentConfig }),
      ...(compiledTools.bundles.length === 0 ? {} : { tool_bundles: compiledTools.bundles }),
      ...(options.secrets === undefined ? {} : { secrets: options.secrets }),
      ...(options.systemPrompt === undefined ? {} : { system_prompt: options.systemPrompt }),
      ...(options.metadata === undefined ? {} : { metadata: options.metadata }),
      ...(options.retainUntil === undefined
        ? {}
        : { retain_until: normalizeTimestamp(options.retainUntil, "retainUntil") }),
      ...(options.network === undefined ? {} : { network: options.network }),
      ...(options.providerRecoveryRetries === undefined
        ? {}
        : { provider_recovery_retries: options.providerRecoveryRetries }),
      ...((this.#clientId === undefined && options.client?.submitRetries === undefined)
        ? {}
        : {
            client: {
              ...(this.#clientId === undefined ? {} : { id: this.#clientId }),
              ...(options.client?.submitRetries === undefined ? {} : { submit_retries: options.client.submitRetries }),
            },
          }),
    } as unknown as CreateSessionRequest;
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

  /** Stop the process-scoped customer runner and all reconnect/heartbeat activity. */
  close(): void {
    if (this.#closed) return;
    this.#closed = true;
    const environment = this.#customerEnvironment;
    this.#customerEnvironment = undefined;
    if (environment !== undefined) void environment.then((value) => value.close()).catch(() => undefined);
  }

  async #ensureCustomerEnvironment(
    registrations: readonly import("./tools.js").ClientRegistration[],
    signal?: AbortSignal,
  ): Promise<void> {
    if (this.#closed) throw new SessionError("Brain client is closed");
    if (registrations.length === 0) return;
    if (this.#clientId === undefined) {
      throw new TypeError("Customer-app Tools require Brain({ client: { id } })");
    }
    if (this.#webSocketFactory === undefined) {
      throw new TypeError("This runtime does not provide WebSocket; pass a webSocketFactory to Brain");
    }
    if (this.#customerEnvironment === undefined) {
      let partial: CustomerEnvironment | undefined;
      const startup = (async () => {
        try {
          partial = new CustomerEnvironment(
            async () => {
              // A request AbortSignal belongs only to that request. The multiplexed customer
              // runner has its own lifetime and must remain reconnectable after the create ends.
              const grant = await this.#transport.customerEnvironmentGrant(this.#clientId!);
              return {
                request: { url: grant.url, protocol: grant.protocol },
                observe: (observation) => this.#transport.customerEnvironmentObserve(
                  grant.observationUrl,
                  grant.observationToken,
                  observation,
                ),
              };
            },
            registrations,
            this.#webSocketFactory!,
            { clientId: this.#clientId! },
          );
          await partial.ready;
          if (this.#closed) {
            partial.close();
            throw new SessionError("Brain client is closed");
          }
          return partial;
        } catch (error) {
          partial?.close();
          throw error;
        }
      })();
      this.#customerEnvironment = startup;
      void startup.catch(() => {
        if (this.#customerEnvironment === startup) this.#customerEnvironment = undefined;
      });
      await waitWithSignal(startup, signal);
      return;
    }
    const environment = await waitWithSignal(this.#customerEnvironment, signal);
    await waitWithSignal(environment.register(registrations), signal);
  }
}

function isComponentTool(value: SessionTool): value is ComponentExtension<"tool", ComponentToolConfig> {
  return value.kind === "brain.component";
}

function componentToolDefinition(value: ComponentExtension<"tool", ComponentToolConfig>): ToolDefinition {
  const config = value.config;
  if (config === null || typeof config !== "object" || Array.isArray(config)) {
    throw new TypeError("Tool component config must be an object containing definition");
  }
  const definition = config.definition;
  if (definition === null || typeof definition !== "object" || Array.isArray(definition)) {
    throw new TypeError("Tool component config.definition is required");
  }
  return definition;
}

function waitWithSignal<T>(promise: Promise<T>, signal?: AbortSignal): Promise<T> {
  if (signal === undefined) return promise;
  if (signal.aborted) return Promise.reject(abortError(signal.reason));
  return new Promise<T>((resolve, reject) => {
    const cleanup = (): void => signal.removeEventListener("abort", onAbort);
    const onAbort = (): void => {
      cleanup();
      reject(abortError(signal.reason));
    };
    signal.addEventListener("abort", onAbort, { once: true });
    promise.then(
      (value) => { cleanup(); resolve(value); },
      (error) => { cleanup(); reject(error); },
    );
  });
}

export class Session implements SessionSummary {
  readonly #transport: Transport;
  #data: SessionData;
  readonly sandbox: SessionSandbox;
  readonly storage: SessionStorage;
  readonly children: SessionChildren;
  constructor(transport: Transport, data: SessionData) {
    this.#transport = transport;
    this.#data = data;
    this.sandbox = new SessionSandbox(transport, data.id);
    this.storage = new SessionStorage(transport, data.id);
    this.children = new SessionChildren(transport, data.id);
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
      componentDigest: this.#data.model.component_digest,
      world: this.#data.model.world,
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

  get retainUntil(): string {
    return this.#data.retain_until;
  }

  async suspend(options: Pick<RequestOptions, "signal"> = {}): Promise<this> {
    this.#data = await this.#transport.json<SessionData>(
      "POST",
      `/v1/sessions/${encodeURIComponent(this.id)}/suspend`,
      { signal: options.signal },
    );
    return this;
  }

  async resume(options: Pick<RequestOptions, "signal"> = {}): Promise<this> {
    this.#data = await this.#transport.json<SessionData>(
      "POST",
      `/v1/sessions/${encodeURIComponent(this.id)}/resume`,
      { signal: options.signal },
    );
    return this;
  }

  async setRetention(
    value: Date | string,
    options: Pick<RequestOptions, "signal"> & { allowShorten?: boolean } = {},
  ): Promise<this> {
    const body: RetentionUpdate = {
      retain_until: normalizeTimestamp(value, "retainUntil"),
      allow_shorten: options.allowShorten ?? false,
    };
    this.#data = await this.#transport.json<SessionData>(
      "POST",
      `/v1/sessions/${encodeURIComponent(this.id)}/retention`,
      { body, signal: options.signal },
    );
    return this;
  }

  async end(options: Pick<RequestOptions, "signal"> = {}): Promise<this> {
    this.#data = await this.#transport.json<SessionData>(
      "POST",
      `/v1/sessions/${encodeURIComponent(this.id)}/end`,
      { signal: options.signal },
    );
    return this;
  }

  async delete(options: Pick<RequestOptions, "signal"> = {}): Promise<void> {
    await this.#transport.json<void>("DELETE", `/v1/sessions/${encodeURIComponent(this.id)}`, {
      signal: options.signal,
    });
    this.#data = {
      ...this.#data,
      state: "deleting" as SessionState,
      turn_state: "idle",
    };
  }

  private markIdle(): void {
    const { turn_phase: _completedPhase, ...data } = this.#data;
    this.#data = { ...data, turn_state: "idle" };
  }
}

function normalizeTimestamp(value: Date | string, field: string): string {
  const parsed = value instanceof Date ? value : new Date(value);
  if (!Number.isFinite(parsed.getTime())) throw new TypeError(`${field} must be a valid timestamp`);
  return parsed.toISOString();
}
