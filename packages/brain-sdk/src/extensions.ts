import { z } from "zod";

import { EsmToolHost, invokeProvisioned, invokeWithEnvelope, messageOf, substituteScript, type ProvisionedToolArtifact, type ProvisionedToolManifest, type ProvisionedToolModule } from "./host.js";
import type { AppToolCall, AppToolContract } from "./app.js";
import type { BoundTool, Agentloop, ClientTool, Environment, HttpProgramRequest, ModelMessage, ModelResponse, Program, ResourceName, Resources, Schema, SchemaInput, SchemaOutput, ServedTool, ToolDefinition, UserInput } from "./types.js";

export const extensionSource = Symbol.for("@aexhq/brain/extension-source");

/** What Brain hands the loop for one turn. */
export interface AgentloopInput {
  readonly input: UserInput;
  /** The transcript as it stands: what the next model call would see. */
  readonly transcript: readonly ModelMessage[];
  /** The loop's slots by name, as it last returned them. */
  readonly slots: Readonly<Record<string, unknown>>;
  /** Every record on the session's feed since the loop last ran, oldest first. */
  readonly events: readonly TurnEvent[];
  readonly configuration: unknown;
  /** The system prompt the session was created with. */
  readonly system: string;
  /** The tools the session was created with. */
  readonly tools: readonly ToolDefinition[];
  readonly runtime: { readonly logicalTimeMs: bigint };
}

/** One record off the session's feed, as the loop sees it between turns. */
export interface TurnEvent { readonly sequence: number; readonly type: string; readonly data: unknown }

/** Brain's services as the loop's host exposes them: JSON in, JSON out. The build
 * wires these to the component's host imports; a test passes its own. */
export interface TurnHost {
  model(requestJson: string): string | Promise<string>;
  dispatch(callsJson: string): string | Promise<string>;
  append(kind: string, payloadJson: string): number | bigint | string | Promise<number | bigint | string>;
  telemetry(recordJson: string): void;
}

export interface ToolCall { readonly callId: string; readonly name: string; readonly input: unknown }
export interface ToolCallResult { readonly callId: string; readonly output: unknown; readonly isError: boolean }
/** One model call, as the loop decides it. The loop owns what the model is told: what it
 * leaves out is what the session was created with. */
export interface ModelTurnRequest {
  /** The system prompt for this call. Omit for `author.system`; empty for none. */
  readonly system?: string;
  /** Tools to offer on this call, by name. Omit for all of `author.tools`; each name
   * given must be one of them. */
  readonly tools?: readonly string[];
  readonly messages: readonly ModelMessage[];
  /** Omit for the session's response format, if it was created with one. */
  readonly response_format?: unknown;
  readonly max_output_tokens?: number;
}

/** What a turn handler returns to say the turn is done. Made by `turn.done`. */
export interface TurnResult { readonly [turnResult]: true; readonly result?: unknown }
const turnResult = Symbol.for("@aexhq/brain/turn-result");

/** A failure the loop raises to end the turn with a code. */
export class AgentloopFailure extends Error {
  readonly code: string;
  readonly retryable: boolean;
  constructor(code: string, message: string, retryable = false) {
    super(message);
    this.name = "AgentloopFailure";
    this.code = code;
    this.retryable = retryable;
  }
}

/**
 * One turn, from the loop's side. `transcript` is the conversation the loop hands back:
 * push the user message, the model's answer and every tool result onto it, or replace it
 * wholesale to compact. Every service call journals before it acts.
 */
export interface AgentloopTurn {
  readonly input: UserInput;
  readonly transcript: ModelMessage[];
  readonly events: readonly TurnEvent[];
  readonly system: string;
  readonly tools: readonly ToolDefinition[];
  readonly logicalTime: Date;
  /** One model call. */
  model(request: ModelTurnRequest): Promise<ModelResponse>;
  /** One or many tool calls, run together. Call it once per call for sequential dispatch. */
  dispatch(calls: readonly ToolCall[]): Promise<ToolCallResult[]>;
  /** The loop's own record on the session's feed; Brain's own kinds are refused. */
  append(kind: string, payload: unknown): Promise<number>;
  telemetry(record: unknown): void;
  /** Emits an `output_emitted` record carrying an assistant message. */
  reply(text: string): Promise<void>;
  done(result?: unknown): TurnResult;
  /** Ends the turn with a failure code. */
  fail(code: string, message: string, options?: { readonly retryable?: boolean }): never;
}

export type AgentloopTurnHandler = (turn: AgentloopTurn) => Promise<TurnResult | void> | TurnResult | void;

export interface AgentloopAuthor<Options> {
  readonly options: Options;
  /** The system prompt the session was created with. Used unless a model call sends its own. */
  readonly system: string;
  /** The tools the session was created with. All are offered unless a model call names a subset. */
  readonly tools: readonly ToolDefinition[];
  /** State the loop keeps beside the transcript, by name. Validated on the way in and
   * out; the object returned is live for the turn and saved when the turn ends. */
  slot<Value extends Schema>(name: string, schema: Value, initial: () => SchemaOutput<Value>): SchemaOutput<Value>;
  /** The turn: exactly one per loop. */
  turn(handler: AgentloopTurnHandler): void;
  readonly context: { estimateTokens(messages: readonly unknown[]): number };
}

type AgentloopSetup<Options> = (author: AgentloopAuthor<Options>) => void;
type AgentloopSource = { readonly kind: "agentloop"; readonly options?: Schema; readonly setup: AgentloopSetup<unknown>; artifact?: URL | Uint8Array; name?: string };

type BindingSchemas = Readonly<Record<string, Schema>>;
/** The declaration behind a built (`esm`) tool. Its program is the module
 * `brain build` bundles from the authored code. */
export interface ToolContract<
  OptionsSchema extends Schema | undefined,
  InputSchema extends Schema,
  OutputSchema extends Schema | undefined,
  Bindings extends BindingSchemas = BindingSchemas,
> {
  readonly description: string;
  readonly options?: OptionsSchema;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
  /** Resource names the program operates on (`fs`, `process`, `net`, `dom`,
   * `secrets`, or a namespaced vendor name). Brain rejects a bind to an
   * environment that does not declare them; an empty (or absent) list binds
   * anywhere. Inside the program, reach the resource through the platform's own
   * API — `node:fs`, `fetch`, `document` — never through Brain. */
  readonly needs?: readonly ResourceName[];
  /** Binding names plus value shapes. Only the names enter the manifest; values are
   * supplied at session create and injected by the environment at runtime. */
  readonly bindings?: Bindings;
}
/** A tool that is one shell script. `$name` in the script is replaced with the
 * input property of that name before the environment runs it; other `$`
 * references are left for the shell. The environment's shell executor decides
 * what the output is — the official environments return
 * `{ exit_code, stdout, stderr }`. */
export interface ShellToolContract<InputSchema extends Schema, OutputSchema extends Schema | undefined, Bindings extends BindingSchemas = BindingSchemas> {
  readonly description: string;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
  readonly needs?: readonly ResourceName[];
  readonly bindings?: Bindings;
  readonly script: string;
}
/** A tool that is one HTTP request to an endpoint the environment fronts: the
 * input travels as the JSON body and the response body is the output. */
export interface HttpToolContract<InputSchema extends Schema, OutputSchema extends Schema | undefined, Bindings extends BindingSchemas = BindingSchemas> {
  readonly description: string;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
  readonly needs?: readonly ResourceName[];
  readonly bindings?: Bindings;
  readonly request: HttpProgramRequest;
}
/** The context a built tool's run handler receives: the binding values, the
 * invocation plumbing, and nothing about resources — the program reaches those
 * through the platform. The working directory is the environment's `fs` root by
 * convention. */
export type ToolRunContext<Options, Bindings extends BindingSchemas = Record<never, Schema>> = {
  readonly bindings: { readonly [Name in keyof Bindings]: SchemaOutput<Bindings[Name]> };
  readonly options: Options;
  readonly signal: AbortSignal;
  readonly deadline: Date;
  /** The invocation's call id (`requestId` remains as its historic alias). */
  readonly callId: string;
  readonly requestId: string;
  progress(value: unknown): void;
};
export interface ToolAuthor<Options, Input, Output, RunContext = ToolRunContext<Options>> {
  setup(handler: (context: { readonly options: Options; readonly signal: AbortSignal; readonly requestId: string }) => void | Promise<void>): void;
  run(handler: (input: Input, context: RunContext) => Output | Promise<Output>): void;
}
type ToolSetup<Options, Input, Output, RunContext = ToolRunContext<Options>> = (author: ToolAuthor<Options, Input, Output, RunContext>) => void;
type ToolProgramSource =
  | { readonly kind: "esm" }
  | { readonly kind: "shell"; readonly script: string }
  | { readonly kind: "http"; readonly request: HttpProgramRequest };
type ToolSource = {
  readonly kind: "tool";
  readonly contract: ToolContract<Schema | undefined, Schema, Schema | undefined>;
  readonly programSource: ToolProgramSource;
  readonly initialize?: (context: unknown) => void | Promise<void>;
  readonly run?: (input: unknown, context: unknown) => unknown | Promise<unknown>;
  name?: string;
  program?: Program;
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
/** One program launch as an environment's executor sees it. */
export interface ExecutionCall {
  readonly callId: string;
  readonly deadline: Date;
  readonly signal: AbortSignal;
  /** The values for the tool's declared binding names. */
  readonly bindings: Readonly<Record<string, string>>;
  readonly sessionId: string;
  readonly requestId: string;
}
/** Runs a `shell` program: the script, with its input references already
 * substituted, in the environment's own way (a local `sh -c`, a command in a
 * VM, an SSH session). What it returns is the tool's output. */
export type ShellExecutor<Instance> = (context: { readonly instance: Instance } & ExecutionCall, script: string) => unknown | Promise<unknown>;
export interface HttpExecutorRequest extends HttpProgramRequest { readonly body: string }
/** Runs an `http` program. The default executor uses the global `fetch` and
 * returns the parsed JSON response (or its text); register one to route the
 * request through the environment's own network path. */
export type HttpExecutor<Instance> = (context: { readonly instance: Instance } & ExecutionCall, request: HttpExecutorRequest) => unknown | Promise<unknown>;
export interface EnvironmentInstanceAuthor<Instance> {
  close(handler: (context: { readonly instance: Instance; readonly signal: AbortSignal; readonly requestId: string }) => void | Promise<void>): void;
  /** Opt in to launching programs of each kind. What is registered here becomes
   * the `runtimes` list on the environment's setup and attach receipts — the
   * runtime half of Brain's bind check. `esm()` accepts the artifacts this process
   * can serve (`brain build` output); attach provisions name payloads by content
   * identity, so the bytes travel out of band — registered here — and a provision
   * naming an unregistered identity fails the attach. */
  readonly execute: {
    esm(options?: { readonly artifacts?: readonly ProvisionedToolArtifact[] }): void;
    shell(handler: ShellExecutor<Instance>): void;
    http(handler?: HttpExecutor<Instance>): void;
  };
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
/** What an environment declares about itself: its options schema and the
 * resources a program finds there. What it executes is derived from the
 * executors it registers. */
export interface EnvironmentContract<OptionsSchema extends Schema | undefined = undefined> {
  readonly options?: OptionsSchema;
  readonly resources?: Resources;
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
  readonly close: (context: unknown) => void | Promise<void>;
  readonly attach?: (context: unknown) => void | Promise<void>;
  readonly cancel?: (context: unknown) => void | Promise<void>;
  readonly detach?: (context: unknown) => void | Promise<void>;
}
interface Executors {
  readonly esm?: EsmToolHost;
  readonly shell?: ShellExecutor<unknown>;
  readonly http?: HttpExecutor<unknown>;
}
type EnvironmentSource = { readonly kind: "environment"; readonly options?: Schema; readonly resources: Resources; readonly setup: EnvironmentSetup<unknown, Record<string, EnvironmentMember>>; name?: string; runtimeName?: string; readonly members: Readonly<Record<string, EnvironmentMember>>; readonly registration: EnvironmentRegistration; readonly executors: Executors };
type ExtensionFactory = Function & { [extensionSource]?: AgentloopSource | ToolSource | EnvironmentSource };

const agentloops = new WeakMap<object, { readonly artifact: URL | Uint8Array; readonly configuration: unknown }>();
const tools = new WeakMap<object, { readonly definition: ToolDefinition; readonly implementationName: string; readonly configuration: unknown; readonly needs: readonly ResourceName[]; readonly bindingNames: readonly string[]; readonly program: Program }>();
const clientTools = new WeakMap<object, { readonly definition: ToolDefinition; readonly contract: AppToolContract; readonly handler: (input: unknown, call: AppToolCall) => unknown }>();
const servedTools = new WeakMap<object, { readonly definition: ToolDefinition; readonly contract: AppToolContract }>();
const environments = new WeakMap<object, EnvironmentRuntime>();
const bindings = new WeakMap<object, { readonly tool: object; readonly environment: Environment }>();
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

/** What a built tool's factory takes: the environment that hosts it, flattened with
 * the tool's own options. `env` is the placement; everything else is configuration. */
export type ToolPlacement<OptionsSchema extends Schema | undefined> = { readonly env: Environment } & (OptionsSchema extends Schema ? SchemaInput<OptionsSchema> : Record<never, never>);

type BoundToolFactory<OptionsSchema extends Schema | undefined, InputSchema extends Schema, OutputSchema extends Schema | undefined> =
  (placement: ToolPlacement<OptionsSchema>) => BoundTool<SchemaOutput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown>;

/**
 * One word, three forms.
 *
 * `tool(contract, setup)` authors a built tool: `brain build` packages it as an
 * `esm` program, and its factory places an instance with
 * `bash({ env: vm, ...options })` — the environment and the tool's configuration
 * arrive together. `tool.shell(contract)` and `tool.http(contract)` author the
 * other two program kinds with no code of their own.
 *
 * `tool({ name, ..., execute })` declares a tool that runs in this process: pass it
 * straight to `sessions.create` and the SDK answers each call off the session's
 * event feed.
 *
 * `tool({ name, ... })` with no `execute` declares a served tool: some other process
 * answers it by joining the session with its share key (`sessions.join`) and
 * registering the function with `serve`.
 */
function toolFunction<
  OptionsSchema extends Schema | undefined,
  InputSchema extends Schema,
  OutputSchema extends Schema | undefined = undefined,
  Bindings extends BindingSchemas = Record<never, Schema>,
>(
  contract: ToolContract<OptionsSchema, InputSchema, OutputSchema, Bindings>,
  setup: ToolSetup<
    OptionsSchema extends Schema ? SchemaOutput<OptionsSchema> : Record<string, never>,
    SchemaOutput<InputSchema>,
    OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown,
    ToolRunContext<OptionsSchema extends Schema ? SchemaOutput<OptionsSchema> : Record<string, never>, Bindings>
  >,
): BoundToolFactory<OptionsSchema, InputSchema, OutputSchema>;
function toolFunction<InputSchema extends Schema, OutputSchema extends Schema | undefined = undefined>(contract: {
  readonly name: string;
  readonly description: string;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
  readonly execute: (
    input: SchemaOutput<InputSchema>,
    call: AppToolCall,
  ) => OutputSchema extends Schema ? SchemaOutput<OutputSchema> | Promise<SchemaOutput<OutputSchema>> : unknown;
}): ClientTool<SchemaOutput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown>;
function toolFunction<InputSchema extends Schema, OutputSchema extends Schema | undefined = undefined>(contract: {
  readonly name: string;
  readonly description: string;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
}): ServedTool<SchemaOutput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown>;
function toolFunction(
  contract: ToolContract<Schema | undefined, Schema, Schema | undefined> & { readonly name?: string; readonly execute?: (input: unknown, call: AppToolCall) => unknown },
  setup?: ToolSetup<unknown, unknown, unknown>,
): unknown {
  if (typeof contract?.description !== "string" || contract.description.length > 8_192) throw new TypeError("Tool description exceeds its contract bound");
  if (setup !== undefined) {
    if (typeof setup !== "function") throw new TypeError("tool requires a setup function");
    return programTool(contract, { kind: "esm" }, collectTool(setup));
  }
  return appHostedTool(contract as { name: string; description: string; input: Schema; output?: Schema; execute?: (input: unknown, call: AppToolCall) => unknown });
}

function shellTool<InputSchema extends Schema, OutputSchema extends Schema | undefined = undefined, Bindings extends BindingSchemas = Record<never, Schema>>(
  contract: ShellToolContract<InputSchema, OutputSchema, Bindings>,
): BoundToolFactory<undefined, InputSchema, OutputSchema> {
  if (typeof contract?.description !== "string" || contract.description.length > 8_192) throw new TypeError("Tool description exceeds its contract bound");
  if (typeof contract.script !== "string" || contract.script.length === 0 || contract.script.length > 262_144) throw new TypeError("a shell tool needs a script within the contract bound");
  return programTool(contract, { kind: "shell", script: contract.script }) as unknown as BoundToolFactory<undefined, InputSchema, OutputSchema>;
}

function httpTool<InputSchema extends Schema, OutputSchema extends Schema | undefined = undefined, Bindings extends BindingSchemas = Record<never, Schema>>(
  contract: HttpToolContract<InputSchema, OutputSchema, Bindings>,
): BoundToolFactory<undefined, InputSchema, OutputSchema> {
  if (typeof contract?.description !== "string" || contract.description.length > 8_192) throw new TypeError("Tool description exceeds its contract bound");
  const request = contract.request;
  if (!plainObject(request) || typeof request.method !== "string" || !/^[A-Z]{3,16}$/u.test(request.method) || typeof request.url !== "string" || request.url.length === 0 || request.url.length > 8_192) {
    throw new TypeError("an http tool needs a request with an upper-case method and a url");
  }
  if (request.headers !== undefined && (!plainObject(request.headers) || Object.values(request.headers).some((value) => typeof value !== "string"))) throw new TypeError("http tool request headers must be strings");
  const template: HttpProgramRequest = { method: request.method, url: request.url, ...(request.headers === undefined ? {} : { headers: { ...request.headers } }) };
  return programTool(contract, { kind: "http", request: template }) as unknown as BoundToolFactory<undefined, InputSchema, OutputSchema>;
}

export const tool: typeof toolFunction & { readonly shell: typeof shellTool; readonly http: typeof httpTool } = Object.assign(toolFunction, { shell: shellTool, http: httpTool });

function programTool(contract: ToolContract<Schema | undefined, Schema, Schema | undefined>, programSource: ToolProgramSource, registered: Pick<ToolSource, "initialize" | "run"> = {}): ExtensionFactory {
  const needs = Object.freeze([...(contract.needs ?? [])]);
  if (needs.some((name) => !validResourceName(name)) || new Set(needs).size !== needs.length) throw new TypeError("tool needs must be unique resource names");
  const bindingNames = Object.freeze(Object.keys(contract.bindings ?? {}));
  if (bindingNames.some((name) => !validIdentifier(name))) throw new TypeError("tool binding names must be identifiers");
  // `env` is the placement key on every factory's options object; an option under
  // the same name could never be passed.
  const optionShape = (contract.options as unknown as { shape?: Record<string, unknown> } | undefined)?.shape;
  if (optionShape !== undefined && typeof optionShape === "object" && optionShape.env !== undefined) {
    throw new TypeError("a tool option cannot be named env; env places the tool in its Environment");
  }
  const factory = ((value?: unknown) => {
    const source = sourceOf(factory, "tool") as ToolSource;
    if (source.name === undefined || source.program === undefined) throw new Error("Tool extension must be built with brain build before it can be used");
    if (value === null || typeof value !== "object" || !("env" in value)) throw new TypeError(`${source.name} is placed with { env }: pass the Environment that hosts it`);
    const { env, ...rest } = value as { env: Environment } & Record<string, unknown>;
    if (typeof env !== "object" || env === null || !environments.has(env)) throw new TypeError("env must be an Environment extension");
    const options = contract.options === undefined ? requireNoExtraOptions(source.name, rest) : contract.options.parse(rest);
    const definition: ToolDefinition = Object.freeze({
      name: source.name,
      description: contract.description,
      inputSchema: Object.freeze(z.toJSONSchema(contract.input) as Record<string, unknown>),
      ...(contract.output === undefined ? {} : { outputSchema: Object.freeze(z.toJSONSchema(contract.output) as Record<string, unknown>) }),
    });
    const instance = Object.freeze({});
    tools.set(instance, { definition, implementationName: source.name, configuration: structuredClone(options), needs, bindingNames, program: source.program });
    bindings.set(instance, { tool: instance, environment: env });
    return instance;
  }) as ExtensionFactory;
  defineSource(factory, { kind: "tool", contract, programSource, ...registered });
  return factory;
}

function appHostedTool(contract: { readonly name: string; readonly description: string; readonly input: Schema; readonly output?: Schema; readonly execute?: (input: unknown, call: AppToolCall) => unknown }): unknown {
  if (!validIdentifier(contract?.name)) throw new TypeError("an app tool needs an identifier-shaped name");
  if (contract.description.length === 0) throw new TypeError("an app tool needs a description");
  if ("execute" in contract && typeof contract.execute !== "function") throw new TypeError("execute must be a function");
  const definition: ToolDefinition = Object.freeze({
    name: contract.name,
    description: contract.description,
    inputSchema: Object.freeze(z.toJSONSchema(contract.input) as Record<string, unknown>),
    ...(contract.output === undefined ? {} : { outputSchema: Object.freeze(z.toJSONSchema(contract.output) as Record<string, unknown>) }),
  });
  const appContract: AppToolContract = { name: contract.name, description: contract.description, input: contract.input, ...(contract.output === undefined ? {} : { output: contract.output }) };
  const instance = Object.freeze({});
  if (contract.execute !== undefined) {
    clientTools.set(instance, { definition, contract: appContract, handler: contract.execute });
  } else {
    servedTools.set(instance, { definition, contract: appContract });
  }
  return instance;
}

/** The registration behind a tool with an `execute`, or undefined for anything else. */
export function inspectClientTool(value: unknown): { readonly definition: ToolDefinition; readonly contract: AppToolContract; readonly handler: (input: unknown, call: AppToolCall) => unknown } | undefined {
  return typeof value === "object" && value !== null ? clientTools.get(value) : undefined;
}

/** The declaration behind a served tool (no `execute`), or undefined for anything else. */
export function inspectServedTool(value: unknown): { readonly definition: ToolDefinition; readonly contract: AppToolContract } | undefined {
  return typeof value === "object" && value !== null ? servedTools.get(value) : undefined;
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

/** The declared program behind a tool factory, before or after `brain build`. */
export function inspectToolProgram(factory: unknown): ToolProgramSource {
  return (sourceOf(factory as ExtensionFactory, "tool") as ToolSource).programSource;
}

/** Run a built (`esm`) tool in this process — the local development loop and the
 * guest runner's path. Shell and http tools have no code of their own to run. */
export async function executeTool(factory: unknown, options: unknown, input: unknown, context: { readonly signal: AbortSignal; readonly deadlineMs: number; readonly requestId?: string; progress?(value: unknown): void }): Promise<unknown> {
  const source = sourceOf(factory as ExtensionFactory, "tool") as ToolSource;
  if (source.run === undefined) throw new TypeError(`a ${source.programSource.kind} tool has no code to run in-process; its environment executes it`);
  const parsedOptions = source.contract.options === undefined
    ? options === undefined ? Object.freeze({}) : requireEmptyConfiguration(options)
    : source.contract.options.parse(options);
  if (source.initialize !== undefined) {
    let initialized = initializedTools.get(source);
    if (initialized === undefined) {
      initialized = Promise.resolve(source.initialize({ options: parsedOptions, signal: context.signal, requestId: context.requestId ?? "setup" }));
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
    progress: context.progress ?? (() => {}),
  });
  return source.contract.output === undefined ? result : source.contract.output.parse(result);
}

export type EnvironmentHandler = (command: unknown) => Promise<unknown>;

type HostedTool =
  | { readonly kind: "esm"; readonly manifest: ProvisionedToolManifest; readonly module: ProvisionedToolModule }
  | { readonly kind: "shell"; readonly manifest: ProvisionedToolManifest; readonly script: string }
  | { readonly kind: "http"; readonly manifest: ProvisionedToolManifest; readonly request: HttpProgramRequest };
interface EnvironmentAttachment {
  readonly sessionId: string;
  readonly bindings: Readonly<Record<string, string>>;
  readonly hosted: ReadonlyMap<string, HostedTool>;
}
interface EnvironmentInstanceState {
  readonly value: unknown;
  readonly options: unknown;
  readonly attachments: Map<string, EnvironmentAttachment>;
}

/** What a provisioned ESM bundle default-exports: `brain build` wraps the tool
 * definition with this, so the host can validate input against the tool's own
 * schema (the manifest was generated from it), resolve run once at provision,
 * and execute with the context it wires. Binding values replace options for
 * provisioned tools; an options schema is parsed empty, so only defaults apply. */
export function provisionedToolRuntime(factory: unknown): ProvisionedToolModule {
  const source = sourceOf(factory as ExtensionFactory, "tool") as ToolSource;
  if (source.run === undefined) throw new TypeError(`a ${source.programSource.kind} tool is not an esm program`);
  const run = source.run;
  const options = source.contract.options === undefined ? Object.freeze({}) : source.contract.options.parse({});
  return Object.freeze({
    kind: "brain.provisioned-tool/v1" as const,
    ...(source.initialize === undefined ? {} : {
      initialize: (context: { readonly signal: AbortSignal; readonly requestId: string }) => source.initialize?.({ options, ...context }),
    }),
    parseInput: (input: unknown) => source.contract.input.parse(input),
    run: async (input: unknown, context: object) => {
      const result = await run(input, { options, ...context });
      return source.contract.output === undefined ? result ?? null : source.contract.output.parse(result);
    },
  });
}

const ENVIRONMENT_CONTRACT = "environment/v1";

export function createEnvironmentHandler(factory: unknown): EnvironmentHandler {
  const source = sourceOf(factory as ExtensionFactory, "environment") as EnvironmentSource;
  const runtimes = (["esm", "shell", "http"] as const).filter((name) => source.executors[name] !== undefined);
  const declaration = { runtimes, resources: structuredClone(source.resources) };
  const instances = new Map<string, EnvironmentInstanceState>();
  // Answers already given, by operation. A redelivery carries the same session and
  // sequence and gets the same answer instead of running the effect again.
  const receipts = new Map<string, unknown>();
  const active = new Map<string, AbortController>();
  const handler = async (raw: unknown) => {
    const command = environmentCommand(raw);
    const operation = command.operation;
    const key = operationKey(operation.session_id, operation.sequence);
    const prior = receipts.get(key);
    if (prior !== undefined) return prior;
    const controller = new AbortController();
    active.set(key, controller);
    let receipt: unknown;
    try {
      const context = { signal: controller.signal, requestId: key };
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
          receipt = { type: "accepted", ...declaration };
          break;
        }
        case "attach": {
          const instance = requiredEnvironmentInstance(instances, operation.environment_id);
          if (operation.attachment_id === undefined) throw new TypeError("attach requires an attachment identity");
          if (!Array.isArray(operation.request.provisions) || !plainObject(operation.request.bindings)) throw new TypeError("attach requires provisions and bindings");
          const hosted = await provisionTools(source, operation.request as unknown as { provisions: unknown[]; bindings: Record<string, unknown> }, context);
          instance.attachments.set(operation.attachment_id, {
            sessionId: operation.session_id,
            bindings: Object.freeze({ ...(operation.request.bindings as Record<string, string>) }),
            hosted,
          });
          await source.registration.attach?.({ ...context, instance: instance.value, sessionId: operation.session_id });
          receipt = { type: "accepted", ...declaration };
          break;
        }
        case "invoke": {
          const { instance, attachment } = authorizedEnvironmentInstance(instances, operation);
          if (typeof operation.request.call_id !== "string" || typeof operation.request.tool !== "string") throw new TypeError("Environment Tool invocation is invalid");
          const hostedTool = attachment.hosted.get(operation.request.tool);
          if (hostedTool === undefined) throw new Error(`no provisioned tool named ${operation.request.tool} is attached`);
          const deadline = operation.request.deadline_ms;
          if (typeof deadline !== "number" || !Number.isInteger(deadline) || deadline < 1) throw new TypeError("Environment Tool invocation needs a deadline");
          const invocation = {
            callId: operation.request.call_id,
            input: operation.request.input,
            deadlineMs: deadline,
            signal: controller.signal,
            bindings: pickBindings(hostedTool.manifest.binding_names, attachment.bindings),
          };
          const call = (signal: AbortSignal, deadlineAt: Date) => ({ instance: instance.value, callId: invocation.callId, deadline: deadlineAt, signal, bindings: invocation.bindings, sessionId: operation.session_id, requestId: key });
          let outcome;
          if (hostedTool.kind === "esm") {
            outcome = await invokeProvisioned(hostedTool.module, invocation);
          } else if (hostedTool.kind === "shell") {
            const executor = source.executors.shell;
            if (executor === undefined) throw new Error("this environment does not execute shell programs");
            outcome = await invokeWithEnvelope(invocation, ({ signal, deadline: deadlineAt }) => executor(call(signal, deadlineAt), substituteScript(hostedTool.script, invocation.input)));
          } else {
            const executor = source.executors.http;
            if (executor === undefined) throw new Error("this environment does not execute http programs");
            outcome = await invokeWithEnvelope(invocation, ({ signal, deadline: deadlineAt }) => executor(call(signal, deadlineAt), { ...hostedTool.request, body: JSON.stringify(invocation.input ?? null) }));
          }
          receipt = { type: "outcome", outcome };
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
          if (!positiveInteger(operation.request.target_sequence)) throw new TypeError("Environment cancellation target is invalid");
          active.get(operationKey(operation.session_id, operation.request.target_sequence))?.abort(new Error("Environment operation cancelled"));
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
      receipt = { type: "failure", code: "environment_error", message: messageOf(error), retryable: false };
    } finally {
      active.delete(key);
    }
    const response = environmentResponse(operation, receipt);
    receipts.set(key, response);
    return response;
  };
  return handler;
}

interface RuntimeEnvironmentOperation {
  /** The sequence of the journal record that started this operation. With the
   * session id, its name. */
  readonly sequence: number;
  readonly environment_id: string;
  readonly session_id: string;
  readonly attachment_id?: string;
  readonly request: Record<string, unknown> & { readonly type: string };
}

function operationKey(sessionId: string, sequence: number): string { return `${sessionId}:${sequence}`; }
function positiveInteger(value: unknown): value is number { return typeof value === "number" && Number.isInteger(value) && value > 0; }

function environmentCommand(raw: unknown): { readonly operation: RuntimeEnvironmentOperation } {
  if (!plainObject(raw) || raw.contract !== ENVIRONMENT_CONTRACT || !plainObject(raw.operation)) throw new TypeError(`invalid ${ENVIRONMENT_CONTRACT} command`);
  const operation = raw.operation;
  if (!positiveInteger(operation.sequence)) throw new TypeError("Environment operation sequence is required");
  for (const name of ["environment_id", "session_id"] as const) if (typeof operation[name] !== "string" || operation[name].length === 0) throw new TypeError(`Environment operation ${name} is required`);
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

/** Resolve an attach's provisions to programs this environment can launch: the
 * program kind must have an executor, every needed resource must be declared,
 * every binding must have a value, and an esm bundle is imported and initialized
 * here. Any failure fails the attach receipt — a broken tool never reaches its
 * first model call. */
async function provisionTools(
  source: EnvironmentSource,
  request: { readonly provisions: readonly unknown[]; readonly bindings: Readonly<Record<string, unknown>> },
  context: { readonly signal: AbortSignal; readonly requestId: string },
): Promise<ReadonlyMap<string, HostedTool>> {
  const hosted = new Map<string, HostedTool>();
  for (const provision of request.provisions) {
    if (!plainObject(provision) || !plainObject(provision.manifest) || typeof provision.payload_identity !== "string") throw new TypeError("attach provision is invalid");
    const manifest = provision.manifest as unknown as ProvisionedToolManifest;
    if (typeof manifest.name !== "string" || !Array.isArray(manifest.needs) || !Array.isArray(manifest.binding_names) || !plainObject(manifest.program)) throw new TypeError("attach provision manifest is invalid");
    const program = manifest.program;
    if (program.identity !== provision.payload_identity) throw new Error(`tool ${manifest.name} names a payload that is not its program`);
    const missingResource = manifest.needs.find((name) => !Object.hasOwn(source.resources, name));
    if (missingResource !== undefined) throw new Error(`tool ${manifest.name} needs ${missingResource}, which this environment does not provide`);
    const missingBinding = manifest.binding_names.find((name) => typeof request.bindings[name] !== "string");
    if (missingBinding !== undefined) throw new Error(`tool ${manifest.name} needs a value for binding ${missingBinding}`);
    if (program.kind === "esm") {
      if (source.executors.esm === undefined) throw new Error(`tool ${manifest.name} is an esm program, which this environment does not execute`);
      hosted.set(manifest.name, { kind: "esm", manifest, module: await source.executors.esm.provision(program.identity, context) });
    } else if (program.kind === "shell") {
      if (source.executors.shell === undefined) throw new Error(`tool ${manifest.name} is a shell program, which this environment does not execute`);
      if (typeof program.script !== "string" || program.script.length === 0) throw new TypeError(`tool ${manifest.name} carries no script`);
      hosted.set(manifest.name, { kind: "shell", manifest, script: program.script });
    } else if (program.kind === "http") {
      if (source.executors.http === undefined) throw new Error(`tool ${manifest.name} is an http program, which this environment does not execute`);
      if (!plainObject(program.request) || typeof program.request.method !== "string" || typeof program.request.url !== "string") throw new TypeError(`tool ${manifest.name} carries no request`);
      hosted.set(manifest.name, { kind: "http", manifest, request: program.request });
    } else {
      throw new TypeError(`tool ${manifest.name} names an unknown program kind`);
    }
  }
  return hosted;
}

function pickBindings(names: readonly string[], values: Readonly<Record<string, string>>): Readonly<Record<string, string>> {
  const picked: Record<string, string> = {};
  for (const name of names) picked[name] = values[name] as string;
  return picked;
}

function environmentResponse(operation: RuntimeEnvironmentOperation, receipt: unknown) {
  return { contract: ENVIRONMENT_CONTRACT, sequence: operation.sequence, receipt };
}

function requireEmptyCallInput(value: unknown): Record<string, never> {
  if (!plainObject(value) || Object.keys(value).length !== 0) throw new TypeError("Environment method does not accept input");
  return Object.freeze({});
}

/** The default `http` executor: the global `fetch`, JSON in, JSON (or text) out,
 * and a non-2xx status as an error whose code carries the status. */
async function fetchExecutor(context: ExecutionCall, request: HttpExecutorRequest): Promise<unknown> {
  const response = await fetch(request.url, {
    method: request.method,
    headers: { "content-type": "application/json", ...(request.headers ?? {}) },
    body: request.body,
    signal: context.signal,
  });
  const text = await response.text();
  let body: unknown = text;
  try { body = JSON.parse(text); } catch { /* a non-JSON body stays text */ }
  if (!response.ok) throw Object.assign(new Error(`${request.method} ${request.url} returned ${response.status}`), { code: `http_${response.status}`, details: body });
  return body;
}

export function environment<Members extends Record<string, EnvironmentMember>>(setup: EnvironmentSetup<Record<string, never>, Members>): (options?: undefined) => Environment & PublicEnvironmentMembers<Members>;
export function environment<OptionsSchema extends Schema, Members extends Record<string, EnvironmentMember>>(contract: EnvironmentContract<OptionsSchema> & { readonly options: OptionsSchema }, setup: EnvironmentSetup<SchemaOutput<OptionsSchema>, Members>): ConfiguredFactory<OptionsSchema, Environment & PublicEnvironmentMembers<Members>>;
export function environment<Members extends Record<string, EnvironmentMember>>(contract: EnvironmentContract<undefined>, setup: EnvironmentSetup<Record<string, never>, Members>): (options?: undefined) => Environment & PublicEnvironmentMembers<Members>;
export function environment<OptionsSchema extends Schema, Members extends Record<string, EnvironmentMember>>(contractOrSetup: EnvironmentContract<OptionsSchema | undefined> | EnvironmentSetup<Record<string, never>, Members>, possibleSetup?: EnvironmentSetup<SchemaOutput<OptionsSchema>, Members>) {
  const contract = typeof contractOrSetup === "function" ? {} : contractOrSetup;
  const optionsSchema = contract.options;
  const setup = (typeof contractOrSetup === "function" ? contractOrSetup : possibleSetup) as EnvironmentSetup<unknown, Members> | undefined;
  if (setup === undefined) throw new TypeError("environment requires a setup function");
  const resources = declaredResources(contract.resources);
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
  defineSource(factory, { kind: "environment", ...(optionsSchema === undefined ? {} : { options: optionsSchema }), resources, setup: setup as EnvironmentSetup<unknown, Record<string, EnvironmentMember>>, members, registration: collected.registration, executors: collected.executors });
  return factory as never;
}

function declaredResources(value: Resources | undefined): Resources {
  if (value === undefined) return Object.freeze({});
  if (!plainObject(value)) throw new TypeError("environment resources must be an object keyed by resource name");
  for (const [name, block] of Object.entries(value)) {
    if (!validResourceName(name)) throw new TypeError(`${name} is not a resource name`);
    if (!plainObject(block)) throw new TypeError(`resource ${name} must declare an object`);
  }
  return Object.freeze(structuredClone(value));
}

function collectEnvironment(setup: EnvironmentSetup<unknown, Record<string, EnvironmentMember>>): { readonly members: Readonly<Record<string, EnvironmentMember>>; readonly registration: EnvironmentRegistration; readonly executors: Executors } {
  let open: EnvironmentRegistration["open"] | undefined;
  let close: EnvironmentRegistration["close"] | undefined;
  let attach: EnvironmentRegistration["attach"];
  let cancel: EnvironmentRegistration["cancel"];
  let detach: EnvironmentRegistration["detach"];
  const executors: { esm?: EsmToolHost; shell?: ShellExecutor<unknown>; http?: HttpExecutor<unknown> } = {};
  const author: EnvironmentAuthor<unknown> = {
    options: Object.freeze({}),
    open<Instance>(handler: (context: { readonly options: unknown; readonly id: string; readonly signal: AbortSignal; readonly requestId: string }) => Instance | Promise<Instance>) {
      if (open !== undefined) throw new TypeError("environment may register open only once");
      open = handler as EnvironmentRegistration["open"];
      const instance: EnvironmentInstanceAuthor<Instance> = {
        close(handler) { if (close !== undefined) throw new TypeError("environment may register close only once"); close = handler as EnvironmentRegistration["close"]; },
        execute: {
          esm(options) {
            if (executors.esm !== undefined) throw new TypeError("environment may register execute.esm only once");
            executors.esm = new EsmToolHost();
            for (const artifact of options?.artifacts ?? []) executors.esm.register(artifact);
          },
          shell(handler) {
            if (executors.shell !== undefined) throw new TypeError("environment may register execute.shell only once");
            if (typeof handler !== "function") throw new TypeError("execute.shell requires an executor function");
            executors.shell = handler as ShellExecutor<unknown>;
          },
          http(handler) {
            if (executors.http !== undefined) throw new TypeError("environment may register execute.http only once");
            if (handler !== undefined && typeof handler !== "function") throw new TypeError("execute.http takes an executor function");
            executors.http = (handler ?? ((context, request) => fetchExecutor(context, request))) as HttpExecutor<unknown>;
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
  if (open === undefined || close === undefined) throw new TypeError("environment must register open and close");
  if (!plainObject(members)) throw new TypeError("environment setup must return its public methods");
  for (const [name, member] of Object.entries(members)) {
    if (!validIdentifier(name) || !plainObject(member) || (member.kind !== "method" && member.kind !== "stream")) throw new TypeError(`Environment member ${name} must be created with method or stream`);
  }
  return {
    members: Object.freeze({ ...members }),
    registration: { open, close, ...(attach === undefined ? {} : { attach }), ...(cancel === undefined ? {} : { cancel }), ...(detach === undefined ? {} : { detach }) },
    executors: Object.freeze({ ...executors }),
  };
}

export function installExtensionIdentity(factory: unknown, name: string, artifact?: URL | Uint8Array, runtimeName?: string, program?: Program): void {
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
  if (source.kind === "tool") {
    if (!plainObject(program) || program.kind !== source.programSource.kind || typeof program.identity !== "string") throw new TypeError(`Tool ${name} has no built program`);
    source.program = structuredClone(program);
  }
}

export function inspectAgentloop(value: Agentloop): { readonly artifact: URL | Uint8Array; readonly configuration: unknown } {
  const metadata = agentloops.get(value);
  if (metadata === undefined) throw new TypeError("agentloop must be created by a built Agentloop");
  return metadata;
}

export function inspectBoundTool(value: BoundTool): { readonly definition: ToolDefinition; readonly implementationName: string; readonly configuration: unknown; readonly needs: readonly ResourceName[]; readonly bindingNames: readonly string[]; readonly program: Program; readonly environment: Environment } {
  const binding = bindings.get(value);
  const metadata = binding === undefined ? undefined : tools.get(binding.tool);
  if (binding === undefined || metadata === undefined) throw new TypeError("a built tool must be placed with its factory's { env } option");
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

/**
 * Runs one turn of a loop against a host. The build calls this from the component's
 * exported `turn` with the real host imports; a test calls it with a fake host.
 */
export async function runTurn(factory: unknown, input: AgentloopInput, host: TurnHost): Promise<{ readonly transcript: ModelMessage[]; readonly slots: Record<string, unknown>; readonly result?: unknown }> {
  const source = sourceOf(factory as ExtensionFactory, "agentloop") as AgentloopSource;
  const options = source.options === undefined ? requireEmptyConfiguration(input.configuration) : source.options.parse(input.configuration);
  const slots = new Map<string, { readonly schema: Schema; readonly value: unknown }>();
  let handler: AgentloopTurnHandler | undefined;
  const author: AgentloopAuthor<unknown> = {
    options,
    system: input.system,
    tools: input.tools,
    slot(name, schema, initial) {
      if (!validIdentifier(name)) throw new TypeError(`slot name ${JSON.stringify(name)} is not an identifier`);
      if (slots.has(name)) throw new TypeError(`agentloop may declare slot ${name} only once`);
      const value = Object.hasOwn(input.slots, name) ? schema.parse(input.slots[name]) : schema.parse(initial());
      slots.set(name, { schema, value });
      return value as ReturnType<typeof schema.parse>;
    },
    turn(next) {
      if (handler !== undefined) throw new TypeError("agentloop may register one turn handler");
      handler = next;
    },
    context: { estimateTokens(messages) { return Math.ceil(JSON.stringify(messages).length / 4); } },
  };
  const registered = source.setup(author);
  if (isPromise(registered)) throw new TypeError("Agentloop setup must be synchronous");
  if (handler === undefined) throw new TypeError("Agentloop registered no turn handler");
  const transcript: ModelMessage[] = [...input.transcript];
  const turn: AgentloopTurn = {
    input: input.input,
    transcript,
    events: input.events,
    system: input.system,
    tools: input.tools,
    logicalTime: new Date(Number(input.runtime.logicalTimeMs)),
    async model(request) {
      return JSON.parse(await host.model(JSON.stringify(request))) as ModelResponse;
    },
    async dispatch(calls) {
      const wire = calls.map((call) => ({ call_id: call.callId, name: call.name, input: call.input }));
      const results = JSON.parse(await host.dispatch(JSON.stringify(wire))) as { call_id: string; output: unknown; is_error: boolean }[];
      return results.map((result) => ({ callId: result.call_id, output: result.output, isError: result.is_error }));
    },
    async append(kind, payload) {
      return Number(await host.append(kind, JSON.stringify(payload ?? null)));
    },
    telemetry(record) {
      host.telemetry(JSON.stringify(record ?? null));
    },
    async reply(text) {
      await turn.append("output_emitted", { type: "assistant_message", message: text });
    },
    done(result) {
      return { [turnResult]: true, ...(result === undefined ? {} : { result }) };
    },
    fail(code, message, options = {}) {
      throw new AgentloopFailure(code, message, options.retryable ?? false);
    },
  };
  const outcome = await handler(turn);
  const saved: Record<string, unknown> = {};
  for (const [name, slot] of slots) saved[name] = slot.schema.parse(slot.value);
  const result = outcome !== undefined && outcome !== null && typeof outcome === "object" && (outcome as TurnResult)[turnResult] === true ? (outcome as TurnResult).result : undefined;
  return { transcript, slots: saved, ...(result === undefined ? {} : { result }) };
}

function defineSource(factory: ExtensionFactory, source: AgentloopSource | ToolSource | EnvironmentSource): void { Object.defineProperty(factory, extensionSource, { value: source }); }
function sourceOf(factory: ExtensionFactory, kind: AgentloopSource["kind"] | ToolSource["kind"] | EnvironmentSource["kind"]) {
  const source = factory?.[extensionSource];
  if (source?.kind !== kind) throw new TypeError(`expected a ${kind} extension`);
  return source;
}
function requireNoExtraOptions(name: string, rest: Record<string, unknown>): Record<string, never> {
  if (Object.keys(rest).length !== 0) throw new TypeError(name + " does not accept options beyond env");
  return Object.freeze({});
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
function validResourceName(value: unknown): value is string { return typeof value === "string" && /^[a-z][a-z0-9_]{0,63}(?::[A-Za-z0-9._-]{1,64})?$/u.test(value); }
function plainObject(value: unknown): value is Record<string, unknown> { return value !== null && typeof value === "object" && !Array.isArray(value); }
function isPromise(value: unknown): value is Promise<unknown> { return value !== null && (typeof value === "object" || typeof value === "function") && typeof (value as { then?: unknown }).then === "function"; }
