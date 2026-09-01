import { z } from "zod";

import { createCallbackRouter, refuseUpgrade, resolveCallbackRoute, type CallbackRoute, type CallbackRouter } from "./callbacks.js";
import type { CapabilityHandles, CapabilityProviderFactory, GrantSet } from "./capabilities.js";
import { EsmToolHost, capabilityHandles, invokeProvisioned, type ProvisionedToolArtifact, type ProvisionedToolModule } from "./host.js";
import type { AppToolCall, AppToolContract } from "./app.js";
import type { BoundTool, Agentloop, CapabilityName, ClientTool, Environment, ModelMessage, ModelResponse, Schema, SchemaInput, SchemaOutput, Tool, ToolDefinition, UserInput } from "./types.js";

const capabilityNames: readonly CapabilityName[] = ["exec", "fs", "net", "js", "page"];

export const extensionSource = Symbol.for("@aexhq/brain/extension-source");

export interface AgentloopInput {
  readonly context: { readonly state?: unknown };
  readonly observation:
    | { readonly type: "session_started" }
    | { readonly type: "user_message"; readonly input: UserInput }
    | { readonly type: "model_completed"; readonly response: ModelResponse }
    | { readonly type: "tools_completed"; readonly results: readonly unknown[] }
    | { readonly type: "emitted"; readonly event: unknown }
    | { readonly type: "cancelled" };
  readonly configuration: unknown;
  readonly runtime: { readonly logicalTimeMs: bigint };
}

export interface ToolCall { readonly callId: string; readonly name: string; readonly input: unknown }
export interface ModelTurnRequest {
  readonly messages: readonly ModelMessage[];
  readonly response_format?: unknown;
  readonly max_output_tokens?: number;
}
export type AgentloopAction =
  | { readonly type: "model"; readonly request: ModelTurnRequest }
  | { readonly type: "tools"; readonly calls: readonly ToolCall[] }
  | { readonly type: "emit"; readonly event: unknown }
  | { readonly type: "reply"; readonly input: UserInput }
  | { readonly type: "finish"; readonly result?: unknown }
  | { readonly type: "fail"; readonly code: string; readonly message: string; readonly retryable: boolean };

export interface AgentloopTurn {
  readonly logicalTime: Date;
  readonly signal: AbortSignal;
  model(request: ModelTurnRequest): AgentloopAction;
  tools(calls: readonly ToolCall[]): AgentloopAction;
  emit(event: unknown): AgentloopAction;
  reply(input: UserInput | string): AgentloopAction;
  done(result?: unknown): AgentloopAction;
  fail(code: string, message: string, options?: { readonly retryable?: boolean }): AgentloopAction;
}

type AgentloopHandler<Input> = (input: Input, turn: AgentloopTurn) => AgentloopAction;
export interface AgentloopAuthor<Options> {
  readonly options: Options;
  readonly on: {
    start(handler: AgentloopHandler<{ readonly type: "session_started" }>): void;
    message(handler: AgentloopHandler<{ readonly type: "user_message"; readonly input: UserInput }>): void;
    model(handler: AgentloopHandler<{ readonly type: "model_completed"; readonly response: ModelResponse }>): void;
    tools(handler: AgentloopHandler<{ readonly type: "tools_completed"; readonly results: readonly unknown[] }>): void;
    event(handler: AgentloopHandler<{ readonly type: "emitted"; readonly event: unknown }>): void;
    cancel(handler: AgentloopHandler<{ readonly type: "cancelled" }>): void;
  };
  state<Value extends Schema>(schema: Value, initial: () => SchemaOutput<Value>): SchemaOutput<Value>;
  readonly context: { estimateTokens(messages: readonly unknown[]): number };
}

type AgentloopSetup<Options> = (author: AgentloopAuthor<Options>) => void;
type AgentloopSource = { readonly kind: "agentloop"; readonly options?: Schema; readonly setup: AgentloopSetup<unknown>; artifact?: URL | Uint8Array; name?: string };

type BindingSchemas = Readonly<Record<string, Schema>>;
export interface ToolContract<
  OptionsSchema extends Schema | undefined,
  InputSchema extends Schema,
  OutputSchema extends Schema | undefined,
  Requires extends readonly CapabilityName[] = readonly CapabilityName[],
  Bindings extends BindingSchemas = BindingSchemas,
> {
  readonly description: string;
  readonly options?: OptionsSchema;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
  /** Capabilities the tool needs from its environment. The whole declaration:
   * the run context is typed from it (an undeclared capability does not
   * type-check), Brain rejects a bind to an environment that does not provide
   * them, and an empty (or absent) list binds anywhere. */
  readonly requires?: Requires;
  /** Binding names plus value shapes. Only the names enter the manifest; values are
   * supplied at session create and injected by the environment at runtime. */
  readonly bindings?: Bindings;
}
/** The context a tool's run handler receives: typed handles for exactly the
 * declared `requires`, typed `bindings` values, and the invocation plumbing. */
export type ToolRunContext<Options, Requires extends readonly CapabilityName[] = readonly [], Bindings extends BindingSchemas = Record<never, Schema>> =
  Pick<CapabilityHandles, Requires[number]> & {
    readonly bindings: { readonly [Name in keyof Bindings]: SchemaOutput<Bindings[Name]> };
    readonly options: Options;
    readonly signal: AbortSignal;
    readonly deadline: Date;
    /** The invocation's call id (`requestId` remains as its historic alias). */
    readonly callId: string;
    readonly requestId: string;
    readonly workspace?: string;
    progress(value: unknown): void;
  };
export interface ToolAuthor<Options, Input, Output, RunContext = ToolRunContext<Options>> {
  setup(handler: (context: { readonly options: Options; readonly signal: AbortSignal; readonly requestId: string; readonly workspace?: string }) => void | Promise<void>): void;
  run(handler: (input: Input, context: RunContext) => Output | Promise<Output>): void;
}
type ToolSetup<Options, Input, Output, RunContext = ToolRunContext<Options>> = (author: ToolAuthor<Options, Input, Output, RunContext>) => void;
type ToolSource = {
  readonly kind: "tool";
  readonly contract: ToolContract<Schema | undefined, Schema, Schema | undefined>;
  readonly initialize?: (context: unknown) => void | Promise<void>;
  readonly run: (input: unknown, context: unknown) => unknown | Promise<unknown>;
  name?: string;
};

export interface EnvironmentMethod<Input = void, Output = void> {
  readonly kind: "method";
  readonly input?: Schema;
  readonly output?: Schema;
  readonly handler: (input: Input, context: unknown) => Output | Promise<Output>;
}
export interface EnvironmentStream<Input = void, Item = unknown> {
  readonly kind: "stream";
  readonly input?: Schema;
  readonly item: Schema;
  readonly handler: (input: Input, context: unknown) => AsyncIterable<Item>;
}
type EnvironmentMember = EnvironmentMethod<never, unknown> | EnvironmentStream<never, unknown>;
export interface EnvironmentInstanceAuthor<Instance> {
  run(handler: (request: unknown, context: { readonly instance: Instance; readonly signal: AbortSignal; readonly requestId: string }) => unknown | Promise<unknown>): void;
  close(handler: (context: { readonly instance: Instance; readonly signal: AbortSignal; readonly requestId: string }) => void | Promise<void>): void;
  /** Register a capability provider. What the environment provides becomes the
   * `provides` list on its setup and attach receipts — the other half of
   * Brain's `requires ⊆ provides` bind check — and hosted tools' handles wire
   * to these providers in-process. Grant clamping is the provider's job; the
   * shared `clamp` helper covers exec bounds and fs-root confinement. */
  readonly provide: { readonly [Name in CapabilityName]: (factory: CapabilityProviderFactory<Instance, Name>) => void };
  /** Opt in to hosting provisioned tools. `esm()` accepts the artifacts this
   * process can serve (`brain build` output); attach provisions name payloads
   * by content identity, so the bytes travel out of band — registered here —
   * and a provision naming an unregistered identity fails the attach. */
  readonly host: { esm(options?: { readonly artifacts?: readonly ProvisionedToolArtifact[] }): void };
  readonly on: {
    attach(handler: (context: { readonly instance: Instance; readonly sessionId: string; readonly signal: AbortSignal; readonly requestId: string }) => void | Promise<void>): void;
    cancel(handler: (context: { readonly instance: Instance; readonly signal: AbortSignal; readonly requestId: string }) => void | Promise<void>): void;
    detach(handler: (context: { readonly instance: Instance; readonly sessionId: string; readonly signal: AbortSignal; readonly requestId: string }) => void | Promise<void>): void;
  };
  method(handler: (input: void, context: unknown) => void | Promise<void>): EnvironmentMethod<void, void>;
  method<InputSchema extends Schema, OutputSchema extends Schema>(contract: { readonly input: InputSchema; readonly output: OutputSchema }, handler: (input: SchemaOutput<InputSchema>, context: unknown) => SchemaOutput<OutputSchema> | Promise<SchemaOutput<OutputSchema>>): EnvironmentMethod<SchemaOutput<InputSchema>, SchemaOutput<OutputSchema>>;
  stream<InputSchema extends Schema, ItemSchema extends Schema>(contract: { readonly input: InputSchema; readonly item: ItemSchema }, handler: (input: SchemaOutput<InputSchema>, context: unknown) => AsyncIterable<SchemaOutput<ItemSchema>>): EnvironmentStream<SchemaOutput<InputSchema>, SchemaOutput<ItemSchema>>;
}
export interface EnvironmentAuthor<Options> {
  readonly options: Options;
  open<Instance>(handler: (context: { readonly options: Options; readonly id: string; readonly signal: AbortSignal; readonly requestId: string }) => Instance | Promise<Instance>): EnvironmentInstanceAuthor<Instance>;
  readonly route: {
    /** Route callback-hosted tool invocations to the author's app. Without a resolver
     * the environment terminates the app's outbound WebSocket channel and expects a
     * `channelToken` string option; a resolver can pick channel or signed-POST mode
     * from the environment's own options instead. */
    callbacks(resolve?: (context: { readonly options: Options }) => CallbackRoute): void;
  };
}
type PublicEnvironmentMembers<Members extends Record<string, EnvironmentMember>> = {
  [Name in keyof Members]: Members[Name] extends EnvironmentMethod<infer Input, infer Output>
    ? [Input] extends [void] ? () => Promise<Output> : (input: Input) => Promise<Output>
    : Members[Name] extends EnvironmentStream<infer Input, infer Item> ? (input: Input) => AsyncIterable<Item> : never;
};
type ConfiguredFactory<OptionsSchema extends Schema, Value> = undefined extends SchemaInput<OptionsSchema>
  ? (options?: SchemaInput<OptionsSchema>) => Value
  : (options: SchemaInput<OptionsSchema>) => Value;
type EnvironmentSetup<Options, Members extends Record<string, EnvironmentMember>> = (author: EnvironmentAuthor<Options>) => Members;
interface EnvironmentRegistration {
  readonly open: (context: unknown) => unknown | Promise<unknown>;
  readonly run: (request: unknown, context: unknown) => unknown | Promise<unknown>;
  readonly close: (context: unknown) => void | Promise<void>;
  readonly attach?: (context: unknown) => void | Promise<void>;
  readonly cancel?: (context: unknown) => void | Promise<void>;
  readonly detach?: (context: unknown) => void | Promise<void>;
}
type ProviderFactories = Readonly<Partial<Record<CapabilityName, (context: { readonly instance: unknown; readonly grants: GrantSet }) => unknown>>>;
type EnvironmentSource = { readonly kind: "environment"; readonly options?: Schema; readonly setup: EnvironmentSetup<unknown, Record<string, EnvironmentMember>>; name?: string; runtimeName?: string; readonly members: Readonly<Record<string, EnvironmentMember>>; readonly registration: EnvironmentRegistration; readonly callbacks?: (context: { readonly options: unknown }) => CallbackRoute; readonly providers: ProviderFactories; readonly esmHost?: EsmToolHost };
type ExtensionFactory = Function & { [extensionSource]?: AgentloopSource | ToolSource | EnvironmentSource };

const agentloops = new WeakMap<object, { readonly artifact: URL | Uint8Array; readonly configuration: unknown }>();
const tools = new WeakMap<object, { readonly definition: ToolDefinition; readonly implementationName: string; readonly configuration: unknown; readonly requires: readonly CapabilityName[]; readonly bindingNames: readonly string[]; readonly hosting?: "callback" }>();
const clientTools = new WeakMap<object, { readonly definition: ToolDefinition; readonly contract: AppToolContract; readonly handler: (input: unknown, call: AppToolCall) => unknown }>();
const environments = new WeakMap<object, EnvironmentRuntime>();
const bindings = new WeakMap<object, { readonly tool: Tool; readonly environment: Environment }>();
const initializedTools = new WeakMap<ToolSource, Promise<void>>();
interface EnvironmentRuntime {
  readonly configuration: unknown;
  client?: { callEnvironment(sessionId: string, environmentId: string, name: string, input: unknown): Promise<unknown> };
  sessionId?: string;
  environmentId?: string;
  ended: boolean;
}

export function agentloop(setup: AgentloopSetup<Record<string, never>>): (options?: undefined) => Agentloop;
export function agentloop<OptionsSchema extends Schema>(contract: { readonly options: OptionsSchema }, setup: AgentloopSetup<SchemaOutput<OptionsSchema>>): (options: SchemaInput<OptionsSchema>) => Agentloop;
export function agentloop<OptionsSchema extends Schema>(contractOrSetup: { readonly options: OptionsSchema } | AgentloopSetup<Record<string, never>>, possibleSetup?: AgentloopSetup<SchemaOutput<OptionsSchema>>) {
  const options = typeof contractOrSetup === "function" ? undefined : contractOrSetup.options;
  const setup = (typeof contractOrSetup === "function" ? contractOrSetup : possibleSetup) as AgentloopSetup<unknown> | undefined;
  if (setup === undefined) throw new TypeError("agentloop requires a setup function");
  const factory = ((value?: unknown) => {
    const source = sourceOf(factory, "agentloop") as AgentloopSource;
    if (source.artifact === undefined || source.name === undefined) throw new Error("Agentloop must be built with brain build before it can be used");
    const configuration = options === undefined ? requireNoOptions(value) : options.parse(value);
    const extension = Object.freeze({});
    agentloops.set(extension, { artifact: source.artifact, configuration: structuredClone(configuration) });
    return extension;
  }) as ExtensionFactory;
  defineSource(factory, { kind: "agentloop", ...(options === undefined ? {} : { options }), setup });
  return factory;
}

export function tool<
  OptionsSchema extends Schema | undefined,
  InputSchema extends Schema,
  OutputSchema extends Schema | undefined = undefined,
  const Requires extends readonly CapabilityName[] = readonly [],
  Bindings extends BindingSchemas = Record<never, Schema>,
>(
  contract: ToolContract<OptionsSchema, InputSchema, OutputSchema, Requires, Bindings>,
  setup: ToolSetup<
    OptionsSchema extends Schema ? SchemaOutput<OptionsSchema> : Record<string, never>,
    SchemaOutput<InputSchema>,
    OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown,
    ToolRunContext<OptionsSchema extends Schema ? SchemaOutput<OptionsSchema> : Record<string, never>, Requires, Bindings>
  >,
): (options?: OptionsSchema extends Schema ? SchemaInput<OptionsSchema> : undefined) => Tool<SchemaOutput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown> {
  if (typeof contract.description !== "string" || contract.description.length > 8_192) throw new TypeError("Tool description exceeds its contract bound");
  if (typeof setup !== "function") throw new TypeError("tool requires a setup function");
  const requires = Object.freeze([...(contract.requires ?? [])]);
  if (requires.some((name) => !capabilityNames.includes(name)) || new Set(requires).size !== requires.length) throw new TypeError("tool requires must be unique capability names");
  const bindingNames = Object.freeze(Object.keys(contract.bindings ?? {}));
  if (bindingNames.some((name) => !validIdentifier(name))) throw new TypeError("tool binding names must be identifiers");
  const registered = collectTool(setup as ToolSetup<unknown, unknown, unknown>);
  const factory = ((value?: unknown) => {
    const source = sourceOf(factory, "tool") as ToolSource;
    if (source.name === undefined) throw new Error("Tool extension must be built with brain build before it can be used");
    const options = contract.options === undefined ? requireNoOptions(value) : contract.options.parse(value);
    const definition: ToolDefinition = Object.freeze({
      name: source.name,
      description: contract.description,
      inputSchema: Object.freeze(z.toJSONSchema(contract.input) as Record<string, unknown>),
      ...(contract.output === undefined ? {} : { outputSchema: Object.freeze(z.toJSONSchema(contract.output) as Record<string, unknown>) }),
    });
    const instance = { useIn(environment: Environment): BoundTool {
      if (!environments.has(environment)) throw new TypeError("useIn requires an Environment extension");
      const bound = Object.freeze({});
      bindings.set(bound, { tool: instance as Tool, environment });
      return bound as BoundTool;
    } } as Tool;
    tools.set(instance, { definition, implementationName: source.name, configuration: structuredClone(options), requires, bindingNames });
    return Object.freeze(instance);
  }) as ExtensionFactory;
  defineSource(factory, { kind: "tool", contract: contract as ToolContract<Schema | undefined, Schema, Schema | undefined>, ...registered });
  return factory as never;
}

/**
 * Declare a tool whose implementation stays in the author's own process.
 *
 * With an `execute` function the result is a client-hosted tool: pass it straight to
 * `sessions.create` and the SDK answers each call off the session's event feed — no
 * environment, no server, no channel. The manifest carries `hosting: "client"`.
 *
 * Without `execute` it is a callback tool for session composition: the bound
 * environment routes each invocation to the app over a channel or signed POST (see
 * `appTools` and `route.callbacks`). The manifest carries `hosting: "callback"`, no
 * payload, empty `requires`, so it binds anywhere.
 */
export function appTool<InputSchema extends Schema, OutputSchema extends Schema | undefined = undefined>(contract: {
  readonly name: string;
  readonly description: string;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
  readonly execute: (
    input: SchemaOutput<InputSchema>,
    call: AppToolCall,
  ) => OutputSchema extends Schema ? SchemaOutput<OutputSchema> | Promise<SchemaOutput<OutputSchema>> : unknown;
}): ClientTool<SchemaOutput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown>;
export function appTool<InputSchema extends Schema, OutputSchema extends Schema | undefined = undefined>(contract: {
  readonly name: string;
  readonly description: string;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
}): Tool<SchemaOutput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown>;
export function appTool(contract: {
  readonly name: string;
  readonly description: string;
  readonly input: Schema;
  readonly output?: Schema;
  readonly execute?: (input: unknown, call: AppToolCall) => unknown;
}): unknown {
  if (!validIdentifier(contract?.name)) throw new TypeError("appTool needs an identifier-shaped name");
  if (typeof contract.description !== "string" || contract.description.length === 0 || contract.description.length > 8_192) throw new TypeError("appTool description exceeds its contract bound");
  if ("execute" in contract && typeof contract.execute !== "function") throw new TypeError("appTool execute must be a function");
  const definition: ToolDefinition = Object.freeze({
    name: contract.name,
    description: contract.description,
    inputSchema: Object.freeze(z.toJSONSchema(contract.input) as Record<string, unknown>),
    ...(contract.output === undefined ? {} : { outputSchema: Object.freeze(z.toJSONSchema(contract.output) as Record<string, unknown>) }),
  });
  if (contract.execute !== undefined) {
    const instance = Object.freeze({});
    clientTools.set(instance, {
      definition,
      contract: { name: contract.name, description: contract.description, input: contract.input, ...(contract.output === undefined ? {} : { output: contract.output }) },
      handler: contract.execute,
    });
    return instance;
  }
  const instance = { useIn(environment: Environment): BoundTool {
    if (!environments.has(environment)) throw new TypeError("useIn requires an Environment extension");
    const bound = Object.freeze({});
    bindings.set(bound, { tool: instance as Tool, environment });
    return bound as BoundTool;
  } } as Tool;
  tools.set(instance, { definition, implementationName: contract.name, configuration: Object.freeze({}), requires: Object.freeze([]), bindingNames: Object.freeze([]), hosting: "callback" });
  return Object.freeze(instance);
}

/** The registration behind a client-hosted `appTool`, or undefined for anything else. */
export function inspectClientTool(value: unknown): { readonly definition: ToolDefinition; readonly contract: AppToolContract; readonly handler: (input: unknown, call: AppToolCall) => unknown } | undefined {
  return typeof value === "object" && value !== null ? clientTools.get(value) : undefined;
}

function collectTool(setup: ToolSetup<unknown, unknown, unknown>): Pick<ToolSource, "initialize" | "run"> {
  let initialize: ToolSource["initialize"];
  let run: ToolSource["run"] | undefined;
  const author: ToolAuthor<unknown, unknown, unknown> = {
    setup(handler) {
      if (initialize !== undefined) throw new TypeError("tool may register setup only once");
      initialize = handler as ToolSource["initialize"];
    },
    run(handler) {
      if (run !== undefined) throw new TypeError("tool may register run only once");
      run = handler as ToolSource["run"];
    },
  };
  const registered = setup(author);
  if (isPromise(registered)) throw new TypeError("tool setup must be synchronous");
  if (run === undefined) throw new TypeError("tool must register run");
  return { ...(initialize === undefined ? {} : { initialize }), run };
}

export async function executeTool(factory: unknown, options: unknown, input: unknown, context: { readonly signal: AbortSignal; readonly deadlineMs: number; readonly requestId?: string; readonly workspace?: string; progress?(value: unknown): void }): Promise<unknown> {
  const source = sourceOf(factory as ExtensionFactory, "tool") as ToolSource;
  const parsedOptions = source.contract.options === undefined
    ? options === undefined ? Object.freeze({}) : requireEmptyConfiguration(options)
    : source.contract.options.parse(options);
  if (source.initialize !== undefined) {
    let initialized = initializedTools.get(source);
    if (initialized === undefined) {
      initialized = Promise.resolve(source.initialize({ options: parsedOptions, signal: context.signal, requestId: context.requestId ?? "setup", ...(context.workspace === undefined ? {} : { workspace: context.workspace }) }));
      initializedTools.set(source, initialized);
    }
    await initialized;
  }
  const parsedInput = source.contract.input.parse(input);
  const result = await source.run(parsedInput, {
    options: parsedOptions,
    signal: context.signal,
    deadline: new Date(context.deadlineMs),
    callId: context.requestId ?? "call",
    requestId: context.requestId ?? "call",
    bindings: Object.freeze({}),
    ...(context.workspace === undefined ? {} : { workspace: context.workspace }),
    progress: context.progress ?? (() => {}),
  });
  return source.contract.output === undefined ? result : source.contract.output.parse(result);
}

/** The channel side of a generated environment handler: mount it on the host HTTP
 * server's `upgrade` event so apps can hold their callback channel to it. */
export interface EnvironmentChannel {
  upgrade(request: import("node:http").IncomingMessage, socket: import("node:stream").Duplex, head?: Uint8Array): void;
}
export type EnvironmentHandler = ((command: unknown) => Promise<unknown>) & { readonly channel: EnvironmentChannel };

interface HostedTool {
  readonly manifest: { readonly name: string; readonly requires: readonly CapabilityName[]; readonly binding_names: readonly string[] };
  readonly module: ProvisionedToolModule;
  readonly handles: Readonly<Partial<CapabilityHandles>>;
}
interface EnvironmentAttachment {
  readonly sessionId: string;
  readonly bindings: Readonly<Record<string, string>>;
  readonly hosted: ReadonlyMap<string, HostedTool>;
}
interface EnvironmentInstanceState {
  readonly value: unknown;
  readonly options: unknown;
  readonly attachments: Map<string, EnvironmentAttachment>;
  readonly provisioned: Set<string>;
  readonly router?: CallbackRouter;
}

/** What a provisioned ESM bundle default-exports: `brain build` wraps the tool
 * definition with this, so the host can validate input against the tool's own
 * schema (the manifest was generated from it), resolve run once at provision,
 * and execute with the context it wires. Binding values replace options for
 * provisioned tools; an options schema is parsed empty, so only defaults apply. */
export function provisionedToolRuntime(factory: unknown): ProvisionedToolModule {
  const source = sourceOf(factory as ExtensionFactory, "tool") as ToolSource;
  const options = source.contract.options === undefined ? Object.freeze({}) : source.contract.options.parse({});
  return Object.freeze({
    kind: "brain.provisioned-tool/v1" as const,
    ...(source.initialize === undefined ? {} : {
      initialize: (context: { readonly signal: AbortSignal; readonly requestId: string }) => source.initialize?.({ options, ...context }),
    }),
    parseInput: (input: unknown) => source.contract.input.parse(input),
    run: async (input: unknown, context: object) => {
      const result = await source.run(input, { options, ...context });
      return source.contract.output === undefined ? result ?? null : source.contract.output.parse(result);
    },
  });
}

export function createEnvironmentHandler(factory: unknown): EnvironmentHandler {
  const source = sourceOf(factory as ExtensionFactory, "environment") as EnvironmentSource;
  const provides = capabilityNames.filter((name) => source.providers[name] !== undefined);
  const instances = new Map<string, EnvironmentInstanceState>();
  const receipts = new Map<string, { readonly identity: string; readonly response: unknown }>();
  const active = new Map<string, AbortController>();
  const handler = async (raw: unknown) => {
    const command = environmentCommand(raw);
    const operation = command.operation;
    const prior = receipts.get(operation.operation_id);
    if (prior !== undefined) {
      if (prior.identity !== operation.request_identity) return environmentResponse(operation, { type: "conflict", expected_identity: prior.identity, actual_identity: operation.request_identity });
      return prior.response;
    }
    const controller = new AbortController();
    active.set(operation.operation_id, controller);
    let receipt: unknown;
    try {
      const context = { signal: controller.signal, requestId: operation.operation_id };
      switch (operation.request.type) {
        case "setup": {
          if (!plainObject(operation.request.configuration)) throw new TypeError("Environment configuration must be an object");
          const options = { ...operation.request.configuration };
          delete options.driver;
          let instance = instances.get(operation.environment_id);
          if (instance === undefined) {
            const value = await source.registration.open({ ...context, id: operation.environment_id, options });
            const router = source.callbacks === undefined ? undefined : createCallbackRouter(resolveCallbackRoute(source.callbacks({ options })));
            instance = { value, options, attachments: new Map(), provisioned: new Set(), ...(router === undefined ? {} : { router }) };
            instances.set(operation.environment_id, instance);
          }
          receipt = { type: "accepted", provides };
          break;
        }
        case "attach": {
          const instance = requiredEnvironmentInstance(instances, operation.environment_id);
          if (operation.attachment_id === undefined) throw new TypeError("attach requires an attachment identity");
          if (!plainObject(operation.request.grants) || !Array.isArray(operation.request.provisions) || !plainObject(operation.request.bindings)) throw new TypeError("attach requires grants, provisions, and bindings");
          // Only provisioned tools arrive as provisions; anything else invoked here is
          // callback-hosted and belongs to the router, when one is registered. With
          // host.esm the SDK hosts the provisions itself; without it they stay on the
          // environment's own run handler (the legacy runtime path).
          for (const provision of operation.request.provisions) {
            if (plainObject(provision) && plainObject(provision.manifest) && typeof provision.manifest.name === "string") instance.provisioned.add(provision.manifest.name);
          }
          const hosted = source.esmHost === undefined
            ? new Map<string, HostedTool>()
            : await provisionTools(source, instance.value, operation.request as unknown as { grants: GrantSet; provisions: unknown[]; bindings: Record<string, unknown> }, context);
          instance.attachments.set(operation.attachment_id, {
            sessionId: operation.session_id,
            bindings: Object.freeze({ ...(operation.request.bindings as Record<string, string>) }),
            hosted,
          });
          await source.registration.attach?.({ ...context, instance: instance.value, sessionId: operation.session_id });
          receipt = { type: "accepted", provides };
          break;
        }
        case "invoke": {
          const { instance, attachment } = authorizedEnvironmentInstance(instances, operation);
          if (typeof operation.request.call_id !== "string" || typeof operation.request.tool !== "string") throw new TypeError("Environment Tool invocation is invalid");
          // Dispatch order: a tool the SDK hosts in-process, then the callback
          // router for anything not provisioned here, then the environment's own
          // run handler as the legacy floor.
          const hostedTool = attachment.hosted.get(operation.request.tool);
          if (hostedTool !== undefined) {
            const deadline = operation.request.deadline_ms;
            if (typeof deadline !== "number" || !Number.isInteger(deadline) || deadline < 1) throw new TypeError("Environment Tool invocation needs a deadline");
            receipt = { type: "outcome", outcome: await invokeProvisioned(hostedTool.module, {
              callId: operation.request.call_id,
              input: operation.request.input,
              deadlineMs: deadline,
              signal: controller.signal,
              handles: hostedTool.handles,
              bindings: pickBindings(hostedTool.manifest.binding_names, attachment.bindings),
            }) };
            break;
          }
          if (instance.router !== undefined && !instance.provisioned.has(operation.request.tool)) {
            const deadline = operation.request.deadline_ms;
            if (typeof deadline !== "number" || !Number.isInteger(deadline) || deadline < 1) throw new TypeError("Environment Tool invocation needs a deadline");
            const outcome = await instance.router.invoke(
              { call_id: operation.request.call_id, name: operation.request.tool, arguments: operation.request.input, deadline_ms: deadline },
              controller.signal,
            );
            receipt = { type: "outcome", outcome };
            break;
          }
          // A tool that ran and failed is an in-band error outcome; the outer catch
          // stays for operations that never reached the tool.
          try {
            const value = await source.registration.run(operation.request, { ...context, instance: instance.value, sessionId: operation.session_id });
            receipt = { type: "outcome", outcome: { status: "ok", value: value ?? null } };
          } catch (error) {
            receipt = { type: "outcome", outcome: { status: "error", error: { code: "tool_error", message: String(error instanceof Error ? error.message : error).slice(0, 4096) } } };
          }
          break;
        }
        case "call": {
          const { instance } = authorizedEnvironmentInstance(instances, operation);
          if (typeof operation.request.name !== "string") throw new TypeError("Environment method name is invalid");
          const member = source.members[operation.request.name];
          if (member === undefined) throw new TypeError(`unknown Environment method ${operation.request.name}`);
          const input = member.input === undefined ? requireEmptyCallInput(operation.request.input) : member.input.parse(operation.request.input);
          if (member.kind === "method") {
            const output = await member.handler(input as never, { ...context, instance: instance.value, sessionId: operation.session_id });
            receipt = { type: "result", output: member.output === undefined ? output ?? null : member.output.parse(output) };
          } else {
            const items: unknown[] = [];
            for await (const item of member.handler(input as never, { ...context, instance: instance.value, sessionId: operation.session_id })) items.push(member.item.parse(item));
            receipt = { type: "result", output: items };
          }
          break;
        }
        case "cancel": {
          if (typeof operation.request.target_operation_id !== "string") throw new TypeError("Environment cancellation target is invalid");
          active.get(operation.request.target_operation_id)?.abort(new Error("Environment operation cancelled"));
          const instance = instances.get(operation.environment_id);
          if (instance !== undefined) await source.registration.cancel?.({ ...context, instance: instance.value });
          receipt = { type: "accepted" };
          break;
        }
        case "detach": {
          const instance = requiredEnvironmentInstance(instances, operation.environment_id);
          if (operation.attachment_id === undefined) throw new TypeError("detach requires an attachment identity");
          instance.attachments.delete(operation.attachment_id);
          await source.registration.detach?.({ ...context, instance: instance.value, sessionId: operation.session_id });
          receipt = { type: "accepted" };
          break;
        }
        case "teardown": {
          const instance = requiredEnvironmentInstance(instances, operation.environment_id);
          if (instance.attachments.size > 0) throw new Error("cannot close an Environment with active attachments");
          instance.router?.close();
          await source.registration.close({ ...context, instance: instance.value });
          instances.delete(operation.environment_id);
          receipt = { type: "accepted" };
          break;
        }
        default: throw new TypeError("unsupported Environment request");
      }
    } catch (error) {
      receipt = { type: "failure", code: "environment_error", message: String(error instanceof Error ? error.message : error).slice(0, 4096), retryable: false };
    } finally {
      active.delete(operation.operation_id);
    }
    const response = environmentResponse(operation, receipt);
    receipts.set(operation.operation_id, { identity: operation.request_identity, response });
    return response;
  };
  const channel: EnvironmentChannel = {
    upgrade(request, socket, head = new Uint8Array(0)) {
      const pathname = new URL(request.url ?? "/", "http://environment").pathname;
      const candidates = [...instances.entries()].filter(([, instance]) => instance.router !== undefined);
      const match = candidates.find(([id]) => pathname.endsWith(`/environments/${id}/channel`)) ?? (candidates.length === 1 ? candidates[0] : undefined);
      if (match === undefined || match[1].router?.upgrade(request, socket, head) !== true) refuseUpgrade(socket, 404, "no callback channel here");
    },
  };
  return Object.assign(handler, { channel });
}

interface RuntimeEnvironmentOperation {
  readonly operation_id: string;
  readonly request_identity: string;
  readonly environment_id: string;
  readonly session_id: string;
  readonly attachment_id?: string;
  readonly request: Record<string, unknown> & { readonly type: string };
}

function environmentCommand(raw: unknown): { readonly operation: RuntimeEnvironmentOperation } {
  if (!plainObject(raw) || raw.contract !== "environment/v2" || !plainObject(raw.operation)) throw new TypeError("invalid environment/v2 command");
  const operation = raw.operation;
  for (const name of ["operation_id", "request_identity", "environment_id", "session_id"] as const) if (typeof operation[name] !== "string" || operation[name].length === 0) throw new TypeError(`Environment operation ${name} is required`);
  if (!plainObject(operation.request) || typeof operation.request.type !== "string") throw new TypeError("Environment operation request is invalid");
  return { operation: operation as unknown as RuntimeEnvironmentOperation };
}

function requiredEnvironmentInstance(instances: Map<string, EnvironmentInstanceState>, id: string) {
  const instance = instances.get(id);
  if (instance === undefined) throw new Error(`Environment ${id} is not open`);
  return instance;
}

function authorizedEnvironmentInstance(instances: Map<string, EnvironmentInstanceState>, operation: RuntimeEnvironmentOperation) {
  const instance = requiredEnvironmentInstance(instances, operation.environment_id);
  const attachment = operation.attachment_id === undefined ? undefined : instance.attachments.get(operation.attachment_id);
  if (attachment === undefined || attachment.sessionId !== operation.session_id) throw new Error("Environment attachment does not authorize this session");
  return { instance, attachment };
}

/** Resolve an attach's provisions to loaded, initialized modules with their
 * handles wired to this environment's providers. Any failure here fails the
 * attach receipt — a broken tool never reaches its first model call. */
async function provisionTools(
  source: EnvironmentSource,
  instanceValue: unknown,
  request: { readonly grants: GrantSet; readonly provisions: readonly unknown[]; readonly bindings: Readonly<Record<string, unknown>> },
  context: { readonly signal: AbortSignal; readonly requestId: string },
): Promise<ReadonlyMap<string, HostedTool>> {
  const hosted = new Map<string, HostedTool>();
  if (request.provisions.length === 0) return hosted;
  if (source.esmHost === undefined) throw new Error("this environment does not host provisioned tools");
  // One provider instance per capability per attachment: the factory sees the
  // open instance and this attachment's grants, and clamps against them.
  const providers: Partial<Record<CapabilityName, unknown>> = {};
  for (const name of capabilityNames) {
    const factory = source.providers[name];
    if (factory !== undefined) providers[name] = factory({ instance: instanceValue, grants: request.grants });
  }
  for (const provision of request.provisions) {
    if (!plainObject(provision) || !plainObject(provision.manifest) || typeof provision.payload_identity !== "string") throw new TypeError("attach provision is invalid");
    const manifest = provision.manifest as unknown as HostedTool["manifest"] & { readonly hosting?: string; readonly payload?: { readonly kind?: string } };
    if (typeof manifest.name !== "string" || !Array.isArray(manifest.requires) || !Array.isArray(manifest.binding_names)) throw new TypeError("attach provision manifest is invalid");
    if (manifest.payload?.kind !== "esm") throw new Error(`tool ${manifest.name} does not carry an esm payload; only esm payloads are hosted`);
    const missingCapability = manifest.requires.find((name: CapabilityName) => providers[name] === undefined);
    if (missingCapability !== undefined) throw new Error(`tool ${manifest.name} requires ${missingCapability}, which this environment does not provide`);
    const missingBinding = manifest.binding_names.find((name) => typeof request.bindings[name] !== "string");
    if (missingBinding !== undefined) throw new Error(`tool ${manifest.name} needs a value for binding ${missingBinding}`);
    const module = await source.esmHost.provision(provision.payload_identity, context);
    hosted.set(manifest.name, { manifest, module, handles: capabilityHandles(manifest.requires, providers as Readonly<Partial<CapabilityHandles>>) });
  }
  return hosted;
}

function pickBindings(names: readonly string[], values: Readonly<Record<string, string>>): Readonly<Record<string, string>> {
  const picked: Record<string, string> = {};
  for (const name of names) picked[name] = values[name] as string;
  return picked;
}

function environmentResponse(operation: RuntimeEnvironmentOperation, receipt: unknown) {
  return { contract: "environment/v2", operation_id: operation.operation_id, request_identity: operation.request_identity, receipt };
}

function requireEmptyCallInput(value: unknown): Record<string, never> {
  if (!plainObject(value) || Object.keys(value).length !== 0) throw new TypeError("Environment method does not accept input");
  return Object.freeze({});
}

export function environment<Members extends Record<string, EnvironmentMember>>(setup: EnvironmentSetup<Record<string, never>, Members>): (options?: undefined) => Environment & PublicEnvironmentMembers<Members>;
export function environment<OptionsSchema extends Schema, Members extends Record<string, EnvironmentMember>>(contract: { readonly options: OptionsSchema }, setup: EnvironmentSetup<SchemaOutput<OptionsSchema>, Members>): ConfiguredFactory<OptionsSchema, Environment & PublicEnvironmentMembers<Members>>;
export function environment<OptionsSchema extends Schema, Members extends Record<string, EnvironmentMember>>(contractOrSetup: { readonly options: OptionsSchema } | EnvironmentSetup<Record<string, never>, Members>, possibleSetup?: EnvironmentSetup<SchemaOutput<OptionsSchema>, Members>) {
  const optionsSchema = typeof contractOrSetup === "function" ? undefined : contractOrSetup.options;
  const setup = (typeof contractOrSetup === "function" ? contractOrSetup : possibleSetup) as EnvironmentSetup<unknown, Members> | undefined;
  if (setup === undefined) throw new TypeError("environment requires a setup function");
  const collected = collectEnvironment(setup);
  const members = collected.members;
  const factory = ((value?: unknown) => {
    const source = sourceOf(factory, "environment") as EnvironmentSource;
    if (source.name === undefined) throw new Error("Environment extension must be built with brain build before it can be used");
    const options = optionsSchema === undefined ? requireNoOptions(value) : optionsSchema.parse(value);
    if (!plainObject(options)) throw new TypeError("Environment options schema must produce an object");
    const runtime: EnvironmentRuntime = { configuration: { driver: source.runtimeName ?? source.name, ...structuredClone(options) }, ended: false };
    const instance: Record<string, unknown> = {};
    for (const [name, member] of Object.entries(source.members)) {
      if (member.kind === "stream") {
        instance[name] = (input?: unknown) => ({
          async *[Symbol.asyncIterator]() {
            if (runtime.client === undefined || runtime.sessionId === undefined || runtime.environmentId === undefined || runtime.ended) throw new Error("Environment stream is available only while its session is attached");
            const parsed = member.input === undefined ? requireNoOptions(input) : member.input.parse(input);
            const output = await runtime.client.callEnvironment(runtime.sessionId, runtime.environmentId, name, parsed);
            if (!Array.isArray(output)) throw new TypeError(`Environment stream ${name} returned a non-array batch`);
            for (const item of output) yield member.item.parse(item);
          },
        });
      } else {
        instance[name] = async (input?: unknown) => {
          if (runtime.client === undefined || runtime.sessionId === undefined || runtime.environmentId === undefined || runtime.ended) throw new Error("Environment method is available only while its session is attached");
          const parsed = member.input === undefined ? requireNoOptions(input) : member.input.parse(input);
          const output = await runtime.client.callEnvironment(runtime.sessionId, runtime.environmentId, name, parsed);
          return member.output === undefined ? output : member.output.parse(output);
        };
      }
    }
    environments.set(instance, runtime);
    return Object.freeze(instance);
  }) as ExtensionFactory;
  defineSource(factory, { kind: "environment", ...(optionsSchema === undefined ? {} : { options: optionsSchema }), setup: setup as EnvironmentSetup<unknown, Record<string, EnvironmentMember>>, members, registration: collected.registration, ...(collected.callbacks === undefined ? {} : { callbacks: collected.callbacks }), providers: collected.providers, ...(collected.esmHost === undefined ? {} : { esmHost: collected.esmHost }) });
  return factory as never;
}

function collectEnvironment(setup: EnvironmentSetup<unknown, Record<string, EnvironmentMember>>): { readonly members: Readonly<Record<string, EnvironmentMember>>; readonly registration: EnvironmentRegistration; readonly callbacks?: (context: { readonly options: unknown }) => CallbackRoute; readonly providers: ProviderFactories; readonly esmHost?: EsmToolHost } {
  let open: EnvironmentRegistration["open"] | undefined;
  let run: EnvironmentRegistration["run"] | undefined;
  let close: EnvironmentRegistration["close"] | undefined;
  let attach: EnvironmentRegistration["attach"];
  let cancel: EnvironmentRegistration["cancel"];
  let detach: EnvironmentRegistration["detach"];
  let callbacks: ((context: { readonly options: unknown }) => CallbackRoute) | undefined;
  const providers: Partial<Record<CapabilityName, (context: { readonly instance: unknown; readonly grants: GrantSet }) => unknown>> = {};
  let esmHost: EsmToolHost | undefined;
  const provideOne = (name: CapabilityName) => (factory: (context: { readonly instance: never; readonly grants: GrantSet }) => unknown) => {
    if (providers[name] !== undefined) throw new TypeError(`environment may provide ${name} only once`);
    if (typeof factory !== "function") throw new TypeError(`provide.${name} requires a provider factory`);
    providers[name] = factory as (context: { readonly instance: unknown; readonly grants: GrantSet }) => unknown;
  };
  const author: EnvironmentAuthor<unknown> = {
    options: Object.freeze({}),
    route: {
      callbacks(resolve) {
        if (callbacks !== undefined) throw new TypeError("environment may register route.callbacks only once");
        callbacks = resolve ?? (({ options }) => {
          const token = plainObject(options) ? options.channelToken : undefined;
          if (typeof token !== "string" || token.length === 0) throw new TypeError("route.callbacks() without a resolver needs a channelToken string option");
          return { mode: "channel", token };
        });
      },
    },
    open<Instance>(handler: (context: { readonly options: unknown; readonly id: string; readonly signal: AbortSignal; readonly requestId: string }) => Instance | Promise<Instance>) {
      if (open !== undefined) throw new TypeError("environment may register open only once");
      open = handler as EnvironmentRegistration["open"];
      const instance: EnvironmentInstanceAuthor<Instance> = {
        run(handler) { if (run !== undefined) throw new TypeError("environment may register run only once"); run = handler as EnvironmentRegistration["run"]; },
        close(handler) { if (close !== undefined) throw new TypeError("environment may register close only once"); close = handler as EnvironmentRegistration["close"]; },
        provide: { exec: provideOne("exec"), fs: provideOne("fs"), net: provideOne("net"), js: provideOne("js"), page: provideOne("page") },
        host: {
          esm(options) {
            if (esmHost !== undefined) throw new TypeError("environment may register host.esm only once");
            esmHost = new EsmToolHost();
            for (const artifact of options?.artifacts ?? []) esmHost.register(artifact);
          },
        },
        on: {
          attach(handler) { if (attach !== undefined) throw new TypeError("environment may register attach only once"); attach = handler as EnvironmentRegistration["attach"]; },
          cancel(handler) { if (cancel !== undefined) throw new TypeError("environment may register cancel only once"); cancel = handler as EnvironmentRegistration["cancel"]; },
          detach(handler) { if (detach !== undefined) throw new TypeError("environment may register detach only once"); detach = handler as EnvironmentRegistration["detach"]; },
        },
        method(first: unknown, second?: unknown) {
          return second === undefined
            ? Object.freeze({ kind: "method" as const, handler: first as EnvironmentMethod["handler"] })
            : Object.freeze({ kind: "method" as const, input: (first as { input: Schema }).input, output: (first as { output: Schema }).output, handler: second as EnvironmentMethod["handler"] });
        },
        stream<InputSchema extends Schema, ItemSchema extends Schema>(contract: { input: InputSchema; item: ItemSchema }, handler: (input: SchemaOutput<InputSchema>, context: unknown) => AsyncIterable<SchemaOutput<ItemSchema>>) {
          return Object.freeze({ kind: "stream" as const, input: contract.input, item: contract.item, handler });
        },
      };
      return instance;
    },
  };
  const members = setup(author);
  if (isPromise(members)) throw new TypeError("environment setup must be synchronous");
  if (open === undefined || run === undefined || close === undefined) throw new TypeError("environment must register open, run, and close");
  if (!plainObject(members)) throw new TypeError("environment setup must return its public methods");
  for (const [name, member] of Object.entries(members)) {
    if (!validIdentifier(name) || !plainObject(member) || (member.kind !== "method" && member.kind !== "stream")) throw new TypeError(`Environment member ${name} must be created with method or stream`);
  }
  return {
    members: Object.freeze({ ...members }),
    registration: { open, run, close, ...(attach === undefined ? {} : { attach }), ...(cancel === undefined ? {} : { cancel }), ...(detach === undefined ? {} : { detach }) },
    ...(callbacks === undefined ? {} : { callbacks }),
    providers: Object.freeze({ ...providers }),
    ...(esmHost === undefined ? {} : { esmHost }),
  };
}

export function installExtensionIdentity(factory: unknown, name: string, artifact?: URL | Uint8Array, runtimeName?: string): void {
  if (typeof factory !== "function" || !validIdentifier(name)) throw new TypeError("invalid generated extension identity");
  const source = (factory as ExtensionFactory)[extensionSource];
  if (source === undefined) throw new TypeError(`export ${name} is not an extension`);
  if (source.name !== undefined) throw new TypeError(`extension ${name} already has an identity`);
  source.name = name;
  if (source.kind === "environment") source.runtimeName = runtimeName ?? name;
  if (source.kind === "agentloop") {
    if (artifact === undefined) throw new TypeError(`Agentloop ${name} has no built artifact`);
    source.artifact = artifact;
  }
}

export function inspectAgentloop(value: Agentloop): { readonly artifact: URL | Uint8Array; readonly configuration: unknown } {
  const metadata = agentloops.get(value);
  if (metadata === undefined) throw new TypeError("agentloop must be created by a built Agentloop");
  return metadata;
}

export function inspectBoundTool(value: BoundTool): { readonly definition: ToolDefinition; readonly implementationName: string; readonly configuration: unknown; readonly requires: readonly CapabilityName[]; readonly bindingNames: readonly string[]; readonly hosting?: "callback"; readonly environment: Environment } {
  const binding = bindings.get(value);
  const metadata = binding === undefined ? undefined : tools.get(binding.tool);
  if (binding === undefined || metadata === undefined) throw new TypeError("tools must be configured with useIn");
  return { ...metadata, environment: binding.environment };
}

export function inspectEnvironment(value: Environment): { readonly configuration: unknown } {
  const metadata = environments.get(value);
  if (metadata === undefined) throw new TypeError("invalid Environment extension");
  return metadata;
}

export function assertEnvironmentBindable(value: Environment): void {
  const metadata = environments.get(value);
  if (metadata === undefined) throw new TypeError("invalid Environment extension");
  if (metadata.client !== undefined || metadata.ended) throw new Error("Environment reference is already attached to a session");
}

export function bindEnvironment(value: Environment, client: EnvironmentRuntime["client"], sessionId: string, environmentId: string): void {
  const metadata = environments.get(value);
  if (metadata === undefined) throw new TypeError("invalid Environment extension");
  if (metadata.client !== undefined) throw new Error("Environment reference is already attached to a session");
  metadata.client = client;
  metadata.sessionId = sessionId;
  metadata.environmentId = environmentId;
}

export function endEnvironment(value: Environment): void {
  const metadata = environments.get(value);
  if (metadata !== undefined) metadata.ended = true;
}

export function activateAgentloop(factory: unknown, input: AgentloopInput): { readonly context: { readonly protocolVersion: "agentloop/v1"; readonly items: readonly unknown[]; readonly state: unknown }; readonly decision: Exclude<AgentloopAction, { type: "reply" }> } {
  const source = sourceOf(factory as ExtensionFactory, "agentloop") as AgentloopSource;
  const options = source.options === undefined ? requireEmptyConfiguration(input.configuration) : source.options.parse(input.configuration);
  const envelope = parseStateEnvelope(input.context.state);
  if (input.observation.type === "emitted" && Object.hasOwn(envelope, "pendingReply")) return output(envelope.slots, false, undefined, { type: "finish", result: envelope.pendingReply });
  const handlers = new Map<string, AgentloopHandler<never>>();
  const schemas: Schema[] = [];
  const slots: unknown[] = [];
  const on = (name: string) => (handler: AgentloopHandler<never>) => {
    if (handlers.has(name)) throw new TypeError(`agentloop may register ${name} only once`);
    handlers.set(name, handler);
  };
  const author: AgentloopAuthor<unknown> = {
    options,
    on: { start: on("session_started"), message: on("user_message"), model: on("model_completed"), tools: on("tools_completed"), event: on("emitted"), cancel: on("cancelled") } as AgentloopAuthor<unknown>["on"],
    state(schema, initial) {
      const index = schemas.length;
      schemas.push(schema);
      const value = index < envelope.slots.length ? schema.parse(envelope.slots[index]) : schema.parse(initial());
      slots.push(value);
      return value;
    },
    context: { estimateTokens(messages) { return Math.ceil(JSON.stringify(messages).length / 4); } },
  };
  const registered = source.setup(author);
  if (isPromise(registered)) throw new TypeError("Agentloop setup must be synchronous");
  const handler = handlers.get(input.observation.type);
  const action = handler === undefined ? defaultAction(input.observation.type) : handler(input.observation as never, turn(input.runtime.logicalTimeMs));
  if (isPromise(action)) throw new TypeError("Agentloop handlers must be synchronous");
  for (let index = 0; index < schemas.length; index += 1) slots[index] = schemas[index]!.parse(slots[index]);
  if (action.type === "reply") return output(slots, true, action.input, { type: "emit", event: { type: "assistant_message", message: action.input.message } });
  return output(slots, false, undefined, action);
}

function turn(logicalTimeMs: bigint): AgentloopTurn {
  const signal = new AbortController().signal;
  return Object.freeze({
    logicalTime: new Date(Number(logicalTimeMs)), signal,
    model: (request: Parameters<AgentloopTurn["model"]>[0]) => ({ type: "model" as const, request }),
    tools: (calls: readonly ToolCall[]) => ({ type: "tools" as const, calls }),
    emit: (event: unknown) => ({ type: "emit" as const, event }),
    reply: (input: UserInput | string) => ({ type: "reply" as const, input: typeof input === "string" ? { message: input } : input }),
    done: (result?: unknown) => ({ type: "finish" as const, ...(result === undefined ? {} : { result }) }),
    fail: (code: string, message: string, options: { readonly retryable?: boolean } = {}) => ({ type: "fail" as const, code, message, retryable: options.retryable ?? false }),
  });
}

function output(slots: readonly unknown[], hasPendingReply: boolean, pendingReply: unknown, decision: Exclude<AgentloopAction, { type: "reply" }>) {
  return { context: { protocolVersion: "agentloop/v1" as const, items: [], state: { version: 1, slots, ...(hasPendingReply ? { pendingReply } : {}) } }, decision };
}

function defaultAction(type: AgentloopInput["observation"]["type"]): Exclude<AgentloopAction, { type: "reply" }> {
  if (type === "session_started") return { type: "finish" };
  if (type === "cancelled") return { type: "fail", code: "cancelled", message: "turn cancelled", retryable: false };
  throw new Error(`Agentloop did not register an ${type} handler`);
}

function parseStateEnvelope(value: unknown): { readonly slots: readonly unknown[]; readonly pendingReply?: unknown } {
  if (value === undefined) return { slots: [] };
  if (!plainObject(value) || value.version !== 1 || !Array.isArray(value.slots)) throw new TypeError("Agentloop state envelope is invalid");
  return { slots: value.slots, ...(Object.hasOwn(value, "pendingReply") ? { pendingReply: value.pendingReply } : {}) };
}

function defineSource(factory: ExtensionFactory, source: AgentloopSource | ToolSource | EnvironmentSource): void { Object.defineProperty(factory, extensionSource, { value: source }); }
function sourceOf(factory: ExtensionFactory, kind: AgentloopSource["kind"] | ToolSource["kind"] | EnvironmentSource["kind"]) {
  const source = factory?.[extensionSource];
  if (source?.kind !== kind) throw new TypeError(`expected a ${kind} extension`);
  return source;
}
function requireNoOptions(value: unknown): Record<string, never> {
  if (value !== undefined) throw new TypeError("this extension does not accept options");
  return Object.freeze({});
}
function requireEmptyConfiguration(value: unknown): Record<string, never> {
  if (!plainObject(value) || Object.keys(value).length !== 0) throw new TypeError("this extension does not accept options");
  return Object.freeze({});
}
function validIdentifier(value: unknown): value is string { return typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(value); }
function plainObject(value: unknown): value is Record<string, unknown> { return value !== null && typeof value === "object" && !Array.isArray(value); }
function isPromise(value: unknown): value is Promise<unknown> { return value !== null && (typeof value === "object" || typeof value === "function") && typeof (value as { then?: unknown }).then === "function"; }
