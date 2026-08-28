import { z } from "zod";

import type { BoundTool, BrainExtension, Environment, Schema, SchemaInput, SchemaOutput, Tool, ToolDefinition } from "./types.js";

export const extensionSource = Symbol.for("@aexhq/brain/extension-source");

export interface BrainInput {
  readonly context: { readonly state?: unknown };
  readonly observation:
    | { readonly type: "session_started" }
    | { readonly type: "user_message"; readonly content: unknown }
    | { readonly type: "model_completed"; readonly response: unknown }
    | { readonly type: "tools_completed"; readonly results: readonly unknown[] }
    | { readonly type: "emitted"; readonly event: unknown }
    | { readonly type: "cancelled" };
  readonly configuration: unknown;
  readonly runtime: { readonly logicalTimeMs: bigint };
}

export interface ToolCall { readonly callId: string; readonly name: string; readonly input: unknown }
export type BrainAction =
  | { readonly type: "model"; readonly request: { readonly messages: readonly unknown[]; readonly response_format?: unknown; readonly max_output_tokens?: number } }
  | { readonly type: "tools"; readonly calls: readonly ToolCall[] }
  | { readonly type: "emit"; readonly event: unknown }
  | { readonly type: "reply"; readonly content: unknown }
  | { readonly type: "finish"; readonly result?: unknown }
  | { readonly type: "fail"; readonly code: string; readonly message: string; readonly retryable: boolean };

export interface BrainTurn {
  readonly logicalTime: Date;
  readonly signal: AbortSignal;
  model(request: { readonly messages: readonly unknown[]; readonly response_format?: unknown; readonly max_output_tokens?: number }): BrainAction;
  tools(calls: readonly ToolCall[]): BrainAction;
  emit(event: unknown): BrainAction;
  reply(content: unknown): BrainAction;
  done(result?: unknown): BrainAction;
  fail(code: string, message: string, options?: { readonly retryable?: boolean }): BrainAction;
}

type BrainHandler<Input> = (input: Input, turn: BrainTurn) => BrainAction;
export interface BrainAuthor<Options> {
  readonly options: Options;
  readonly on: {
    start(handler: BrainHandler<{ readonly type: "session_started" }>): void;
    message(handler: BrainHandler<{ readonly type: "user_message"; readonly content: unknown }>): void;
    model(handler: BrainHandler<{ readonly type: "model_completed"; readonly response: unknown }>): void;
    tools(handler: BrainHandler<{ readonly type: "tools_completed"; readonly results: readonly unknown[] }>): void;
    event(handler: BrainHandler<{ readonly type: "emitted"; readonly event: unknown }>): void;
    cancel(handler: BrainHandler<{ readonly type: "cancelled" }>): void;
  };
  state<Value extends Schema>(schema: Value, initial: () => SchemaOutput<Value>): SchemaOutput<Value>;
  readonly context: { estimateTokens(messages: readonly unknown[]): number };
}

type BrainSetup<Options> = (author: BrainAuthor<Options>) => void;
type BrainSource = { readonly kind: "brain"; readonly options?: Schema; readonly setup: BrainSetup<unknown>; artifact?: URL | Uint8Array; name?: string };

export interface ToolContract<OptionsSchema extends Schema | undefined, InputSchema extends Schema, OutputSchema extends Schema | undefined> {
  readonly description: string;
  readonly options?: OptionsSchema;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
}
export interface ToolAuthor<Options, Input, Output> {
  setup(handler: (context: { readonly options: Options; readonly signal: AbortSignal; readonly requestId: string; readonly workspace?: string }) => void | Promise<void>): void;
  run(handler: (input: Input, context: { readonly options: Options; readonly signal: AbortSignal; readonly deadline: Date; readonly requestId: string; readonly workspace?: string; progress(value: unknown): void }) => Output | Promise<Output>): void;
}
type ToolSetup<Options, Input, Output> = (author: ToolAuthor<Options, Input, Output>) => void;
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
type EnvironmentSource = { readonly kind: "environment"; readonly options?: Schema; readonly setup: EnvironmentSetup<unknown, Record<string, EnvironmentMember>>; name?: string; runtimeName?: string; readonly members: Readonly<Record<string, EnvironmentMember>>; readonly registration: EnvironmentRegistration };
type ExtensionFactory = Function & { [extensionSource]?: BrainSource | ToolSource | EnvironmentSource };

const brains = new WeakMap<object, { readonly artifact: URL | Uint8Array; readonly configuration: unknown }>();
const tools = new WeakMap<object, { readonly definition: ToolDefinition; readonly implementationName: string; readonly configuration: unknown }>();
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

export function brain(setup: BrainSetup<Record<string, never>>): (options?: undefined) => BrainExtension;
export function brain<OptionsSchema extends Schema>(contract: { readonly options: OptionsSchema }, setup: BrainSetup<SchemaOutput<OptionsSchema>>): (options: SchemaInput<OptionsSchema>) => BrainExtension;
export function brain<OptionsSchema extends Schema>(contractOrSetup: { readonly options: OptionsSchema } | BrainSetup<Record<string, never>>, possibleSetup?: BrainSetup<SchemaOutput<OptionsSchema>>) {
  const options = typeof contractOrSetup === "function" ? undefined : contractOrSetup.options;
  const setup = (typeof contractOrSetup === "function" ? contractOrSetup : possibleSetup) as BrainSetup<unknown> | undefined;
  if (setup === undefined) throw new TypeError("brain requires a setup function");
  const factory = ((value?: unknown) => {
    const source = sourceOf(factory, "brain") as BrainSource;
    if (source.artifact === undefined || source.name === undefined) throw new Error("Brain extension must be built with brain build before it can be used");
    const configuration = options === undefined ? requireNoOptions(value) : options.parse(value);
    const extension = Object.freeze({});
    brains.set(extension, { artifact: source.artifact, configuration: structuredClone(configuration) });
    return extension;
  }) as ExtensionFactory;
  defineSource(factory, { kind: "brain", ...(options === undefined ? {} : { options }), setup });
  return factory;
}

export function tool<OptionsSchema extends Schema | undefined, InputSchema extends Schema, OutputSchema extends Schema | undefined = undefined>(
  contract: ToolContract<OptionsSchema, InputSchema, OutputSchema>,
  setup: ToolSetup<OptionsSchema extends Schema ? SchemaOutput<OptionsSchema> : Record<string, never>, SchemaOutput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown>,
): (options?: OptionsSchema extends Schema ? SchemaInput<OptionsSchema> : undefined) => Tool<SchemaOutput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown> {
  if (typeof contract.description !== "string" || contract.description.length > 8_192) throw new TypeError("Tool description exceeds its contract bound");
  if (typeof setup !== "function") throw new TypeError("tool requires a setup function");
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
    tools.set(instance, { definition, implementationName: source.name, configuration: structuredClone(options) });
    return Object.freeze(instance);
  }) as ExtensionFactory;
  defineSource(factory, { kind: "tool", contract: contract as ToolContract<Schema | undefined, Schema, Schema | undefined>, ...registered });
  return factory as never;
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
    requestId: context.requestId ?? "call",
    ...(context.workspace === undefined ? {} : { workspace: context.workspace }),
    progress: context.progress ?? (() => {}),
  });
  return source.contract.output === undefined ? result : source.contract.output.parse(result);
}

export function createEnvironmentHandler(factory: unknown): (command: unknown) => Promise<unknown> {
  const source = sourceOf(factory as ExtensionFactory, "environment") as EnvironmentSource;
  const instances = new Map<string, { readonly value: unknown; readonly options: unknown; readonly attachments: Map<string, string> }>();
  const receipts = new Map<string, { readonly identity: string; readonly response: unknown }>();
  const active = new Map<string, AbortController>();
  return async (raw: unknown) => {
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
            instance = { value, options, attachments: new Map() };
            instances.set(operation.environment_id, instance);
          }
          receipt = { type: "accepted" };
          break;
        }
        case "attach": {
          const instance = requiredEnvironmentInstance(instances, operation.environment_id);
          if (operation.attachment_id === undefined) throw new TypeError("attach requires an attachment identity");
          instance.attachments.set(operation.attachment_id, operation.session_id);
          await source.registration.attach?.({ ...context, instance: instance.value, sessionId: operation.session_id });
          receipt = { type: "accepted" };
          break;
        }
        case "execute": {
          const instance = authorizedEnvironmentInstance(instances, operation);
          if (!plainObject(operation.request.tool) || typeof operation.request.tool.call_id !== "string") throw new TypeError("Environment Tool execution is invalid");
          const output = await source.registration.run(operation.request, { ...context, instance: instance.value, sessionId: operation.session_id });
          receipt = { type: "tool_result", result: { call_id: operation.request.tool.call_id, output, is_error: false } };
          break;
        }
        case "call": {
          const instance = authorizedEnvironmentInstance(instances, operation);
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
  if (!plainObject(raw) || raw.contract !== "environment/v1" || !plainObject(raw.operation)) throw new TypeError("invalid environment/v1 command");
  const operation = raw.operation;
  for (const name of ["operation_id", "request_identity", "environment_id", "session_id"] as const) if (typeof operation[name] !== "string" || operation[name].length === 0) throw new TypeError(`Environment operation ${name} is required`);
  if (!plainObject(operation.request) || typeof operation.request.type !== "string") throw new TypeError("Environment operation request is invalid");
  return { operation: operation as unknown as RuntimeEnvironmentOperation };
}

function requiredEnvironmentInstance(instances: Map<string, { readonly value: unknown; readonly options: unknown; readonly attachments: Map<string, string> }>, id: string) {
  const instance = instances.get(id);
  if (instance === undefined) throw new Error(`Environment ${id} is not open`);
  return instance;
}

function authorizedEnvironmentInstance(instances: Map<string, { readonly value: unknown; readonly options: unknown; readonly attachments: Map<string, string> }>, operation: RuntimeEnvironmentOperation) {
  const instance = requiredEnvironmentInstance(instances, operation.environment_id);
  if (operation.attachment_id === undefined || instance.attachments.get(operation.attachment_id) !== operation.session_id) throw new Error("Environment attachment does not authorize this session");
  return instance;
}

function environmentResponse(operation: RuntimeEnvironmentOperation, receipt: unknown) {
  return { contract: "environment/v1", operation_id: operation.operation_id, request_identity: operation.request_identity, receipt };
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
  defineSource(factory, { kind: "environment", ...(optionsSchema === undefined ? {} : { options: optionsSchema }), setup: setup as EnvironmentSetup<unknown, Record<string, EnvironmentMember>>, members, registration: collected.registration });
  return factory as never;
}

function collectEnvironment(setup: EnvironmentSetup<unknown, Record<string, EnvironmentMember>>): { readonly members: Readonly<Record<string, EnvironmentMember>>; readonly registration: EnvironmentRegistration } {
  let open: EnvironmentRegistration["open"] | undefined;
  let run: EnvironmentRegistration["run"] | undefined;
  let close: EnvironmentRegistration["close"] | undefined;
  let attach: EnvironmentRegistration["attach"];
  let cancel: EnvironmentRegistration["cancel"];
  let detach: EnvironmentRegistration["detach"];
  const author: EnvironmentAuthor<unknown> = {
    options: Object.freeze({}),
    open<Instance>(handler: (context: { readonly options: unknown; readonly id: string; readonly signal: AbortSignal; readonly requestId: string }) => Instance | Promise<Instance>) {
      if (open !== undefined) throw new TypeError("environment may register open only once");
      open = handler as EnvironmentRegistration["open"];
      const instance: EnvironmentInstanceAuthor<Instance> = {
        run(handler) { if (run !== undefined) throw new TypeError("environment may register run only once"); run = handler as EnvironmentRegistration["run"]; },
        close(handler) { if (close !== undefined) throw new TypeError("environment may register close only once"); close = handler as EnvironmentRegistration["close"]; },
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
  return { members: Object.freeze({ ...members }), registration: { open, run, close, ...(attach === undefined ? {} : { attach }), ...(cancel === undefined ? {} : { cancel }), ...(detach === undefined ? {} : { detach }) } };
}

export function installExtensionIdentity(factory: unknown, name: string, artifact?: URL | Uint8Array, runtimeName?: string): void {
  if (typeof factory !== "function" || !validIdentifier(name)) throw new TypeError("invalid generated extension identity");
  const source = (factory as ExtensionFactory)[extensionSource];
  if (source === undefined) throw new TypeError(`export ${name} is not an extension`);
  if (source.name !== undefined) throw new TypeError(`extension ${name} already has an identity`);
  source.name = name;
  if (source.kind === "environment") source.runtimeName = runtimeName ?? name;
  if (source.kind === "brain") {
    if (artifact === undefined) throw new TypeError(`Brain extension ${name} has no built artifact`);
    source.artifact = artifact;
  }
}

export function inspectBrain(value: BrainExtension): { readonly artifact: URL | Uint8Array; readonly configuration: unknown } {
  const metadata = brains.get(value);
  if (metadata === undefined) throw new TypeError("brain must be created by a built Brain extension");
  return metadata;
}

export function inspectBoundTool(value: BoundTool): { readonly definition: ToolDefinition; readonly implementationName: string; readonly configuration: unknown; readonly environment: Environment } {
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

export function activateBrain(factory: unknown, input: BrainInput): { readonly context: { readonly protocolVersion: "agentloop/v1"; readonly items: readonly unknown[]; readonly state: unknown }; readonly decision: Exclude<BrainAction, { type: "reply" }> } {
  const source = sourceOf(factory as ExtensionFactory, "brain") as BrainSource;
  const options = source.options === undefined ? requireEmptyConfiguration(input.configuration) : source.options.parse(input.configuration);
  const envelope = parseStateEnvelope(input.context.state);
  if (input.observation.type === "emitted" && Object.hasOwn(envelope, "pendingReply")) return output(envelope.slots, false, undefined, { type: "finish", result: envelope.pendingReply });
  const handlers = new Map<string, BrainHandler<never>>();
  const schemas: Schema[] = [];
  const slots: unknown[] = [];
  const on = (name: string) => (handler: BrainHandler<never>) => {
    if (handlers.has(name)) throw new TypeError(`brain may register ${name} only once`);
    handlers.set(name, handler);
  };
  const author: BrainAuthor<unknown> = {
    options,
    on: { start: on("session_started"), message: on("user_message"), model: on("model_completed"), tools: on("tools_completed"), event: on("emitted"), cancel: on("cancelled") } as BrainAuthor<unknown>["on"],
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
  if (isPromise(registered)) throw new TypeError("Brain setup must be synchronous");
  const handler = handlers.get(input.observation.type);
  const action = handler === undefined ? defaultAction(input.observation.type) : handler(input.observation as never, turn(input.runtime.logicalTimeMs));
  if (isPromise(action)) throw new TypeError("Brain handlers must be synchronous");
  for (let index = 0; index < schemas.length; index += 1) slots[index] = schemas[index]!.parse(slots[index]);
  if (action.type === "reply") return output(slots, true, action.content, { type: "emit", event: { type: "assistant_message", content: action.content } });
  return output(slots, false, undefined, action);
}

function turn(logicalTimeMs: bigint): BrainTurn {
  const signal = new AbortController().signal;
  return Object.freeze({
    logicalTime: new Date(Number(logicalTimeMs)), signal,
    model: (request: Parameters<BrainTurn["model"]>[0]) => ({ type: "model" as const, request }),
    tools: (calls: readonly ToolCall[]) => ({ type: "tools" as const, calls }),
    emit: (event: unknown) => ({ type: "emit" as const, event }),
    reply: (content: unknown) => ({ type: "reply" as const, content }),
    done: (result?: unknown) => ({ type: "finish" as const, ...(result === undefined ? {} : { result }) }),
    fail: (code: string, message: string, options: { readonly retryable?: boolean } = {}) => ({ type: "fail" as const, code, message, retryable: options.retryable ?? false }),
  });
}

function output(slots: readonly unknown[], hasPendingReply: boolean, pendingReply: unknown, decision: Exclude<BrainAction, { type: "reply" }>) {
  return { context: { protocolVersion: "agentloop/v1" as const, items: [], state: { version: 1, slots, ...(hasPendingReply ? { pendingReply } : {}) } }, decision };
}

function defaultAction(type: BrainInput["observation"]["type"]): Exclude<BrainAction, { type: "reply" }> {
  if (type === "session_started") return { type: "finish" };
  if (type === "cancelled") return { type: "fail", code: "cancelled", message: "turn cancelled", retryable: false };
  throw new Error(`Brain did not register an ${type} handler`);
}

function parseStateEnvelope(value: unknown): { readonly slots: readonly unknown[]; readonly pendingReply?: unknown } {
  if (value === undefined) return { slots: [] };
  if (!plainObject(value) || value.version !== 1 || !Array.isArray(value.slots)) throw new TypeError("Brain state envelope is invalid");
  return { slots: value.slots, ...(Object.hasOwn(value, "pendingReply") ? { pendingReply: value.pendingReply } : {}) };
}

function defineSource(factory: ExtensionFactory, source: BrainSource | ToolSource | EnvironmentSource): void { Object.defineProperty(factory, extensionSource, { value: source }); }
function sourceOf(factory: ExtensionFactory, kind: BrainSource["kind"] | ToolSource["kind"] | EnvironmentSource["kind"]) {
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
