import { z } from "zod";

import type { HostToolCall, HostToolContract } from "./host.js";
import type {
  Component, Environment, Outcome, PlacedAgentloop, PlacedTool, Schema, SchemaInput, SchemaOutput,
  ToolDefinition,
} from "./types.js";

const source = Symbol.for("@aexhq/brain/extension-source");
const factorySource = Symbol.for("@aexhq/brain/tool-factory");

interface ComponentSource {
  readonly kind: "component";
  readonly artifact: URL | Uint8Array;
}

/** How Brain reaches an Environment. Applications never write it: the factories do. */
export type EnvironmentDriver =
  | { readonly driver: "brain" }
  | { readonly driver: "host" }
  | { readonly driver: "http"; readonly url: string; readonly credential?: string };

interface EnvironmentSource {
  readonly kind: "environment";
  readonly automatic?: boolean;
  readonly name: string;
  readonly driver: EnvironmentDriver;
  readonly configuration: unknown;
}

interface AgentloopSource {
  readonly kind: "agentloop";
  readonly implementation: Component | Readonly<Record<string, unknown>>;
  readonly configuration: unknown;
  readonly environment: Environment;
}

interface ToolSource {
  readonly kind: "tool";
  readonly definition: ToolDefinition;
  /** What the Environment interprets: a Component to admit, a descriptor, or nothing
   * when the Tool is a function this process holds. */
  readonly implementation: Component | Readonly<Record<string, unknown>> | undefined;
  readonly handler?: (input: unknown, call: HostToolCall) => unknown;
  readonly contract?: HostToolContract;
  readonly load?: () => Promise<ToolSource>;
  readonly configuration: unknown;
  readonly environment: Environment;
}

type ExtensionSource = ComponentSource | EnvironmentSource | AgentloopSource | ToolSource;
type Branded = object & { readonly [source]?: ExtensionSource };

export function component(artifact: URL | Uint8Array): Component {
  if (!(artifact instanceof URL) && !(artifact instanceof Uint8Array)) {
    throw new TypeError("component needs a URL or Uint8Array");
  }
  if (artifact instanceof Uint8Array && artifact.byteLength === 0) {
    throw new TypeError("component bytes cannot be empty");
  }
  return branded({ kind: "component", artifact });
}

type Options<OptionsSchema extends Schema | undefined> =
  OptionsSchema extends Schema ? SchemaOutput<OptionsSchema> : Record<string, never>;

/** An Environment reached over HTTP. The extension defines its options and how each
 * instance is reached; Brain reads the URL and the optional credential and carries the
 * configuration unread. */
export interface EnvironmentContract<OptionsSchema extends Schema | undefined = undefined> {
  readonly options?: OptionsSchema;
  readonly url: (options: Options<OptionsSchema>) => string;
  readonly credential?: (options: Options<OptionsSchema>) => string | undefined;
  readonly configure?: (options: Options<OptionsSchema>) => unknown;
}

/** Every Environment is named at instantiation: the name is unique within a session
 * and is how records refer to it. */
type Named<OptionsSchema extends Schema | undefined> = { readonly name: string } &
  (OptionsSchema extends Schema ? SchemaInput<OptionsSchema> : Record<never, never>);

export function environment<OptionsSchema extends Schema | undefined = undefined>(
  contract: EnvironmentContract<OptionsSchema>,
): (instance: Named<OptionsSchema>) => Environment {
  if (typeof contract.url !== "function") throw new TypeError("an Environment contract needs a url function");
  return (raw: Named<OptionsSchema>) => {
    const { name, options } = namedOptions(contract.options, raw);
    const url = contract.url(options as never);
    validateUrl(url);
    const credential = contract.credential?.(options as never);
    if (credential !== undefined && (typeof credential !== "string" || credential.length === 0)) {
      throw new TypeError("an Environment credential must be a non-empty string");
    }
    const configured = contract.configure?.(options as never) ?? options;
    return branded({
      kind: "environment",
      name,
      driver: { driver: "http", url, ...(credential === undefined ? {} : { credential }) },
      configuration: clone(configured),
    });
  };
}

export interface BrainEnvOptions {
  readonly name: string;
  /** Server environment variables mounted under `/secrets`, by name. */
  readonly secrets?: readonly string[];
  /** HTTP(S) origins, optionally `scheme://*.domain`, within server policy. */
  readonly network?: readonly string[];
  readonly filesystem?: {
    readonly workspace?: "read" | "write";
    readonly scratch?: "read" | "write";
  };
}

const brainOptions = z.strictObject({
  name: z.string().regex(/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u),
  secrets: z.array(z.string()).optional(),
  network: z.array(z.string().refine((value) => {
    try {
      const url = new URL(value);
      return ["http:", "https:"].includes(url.protocol) && url.username === "" && url.password === ""
        && ["", "/"].includes(url.pathname) && url.search === "" && url.hash === "";
    } catch { return false; }
  }, "network must contain HTTP(S) origins")).optional(),
  filesystem: z.strictObject({ workspace: z.enum(["read", "write"]).optional(), scratch: z.enum(["read", "write"]).optional() }).optional(),
});

/** A fresh Wasmtime instance per invocation, with this Environment's configured grants.
 * The server's policy is the ceiling; omitted access stays denied. */
export function brainEnv(options: BrainEnvOptions): Environment {
  const { name, secrets, ...configuration } = brainOptions.parse(options);
  if (secrets !== undefined) uniqueNames(secrets, "brainEnv secrets", identifierPattern);
  return branded({
    kind: "environment", name, driver: { driver: "brain" },
    configuration: { ...configuration, ...(secrets === undefined ? {} : { secrets }) },
  });
}

/** This process, registered with Brain as a host: a Tool placed here is a function this
 * process holds, and Brain sends it the call over the connection the client keeps open. */
export function hostEnv(options: { readonly name: string }): Environment {
  if (!isRecord(options)) throw new TypeError("hostEnv needs { name }");
  identifier(options.name, "Environment name");
  return branded({ kind: "environment", name: options.name, driver: { driver: "host" }, configuration: {} });
}

const defaultHost = branded<Environment>({
  kind: "environment", name: "brain-sdk-host", driver: { driver: "host" },
  configuration: { sdk_default: true }, automatic: true,
});

export function placeDefaultTools(tools: readonly PlacedTool[], name: string): PlacedTool[] {
  const environment = branded<Environment>({
    kind: "environment", name, driver: { driver: "host" }, configuration: { sdk_default: true },
  });
  return tools.map(placed => {
    const tool = inspectTool(placed);
    return inspectEnvironment(tool.environment).automatic ? branded({ ...tool, environment }) : placed;
  });
}

export interface AgentloopContract<OptionsSchema extends Schema | undefined = undefined> {
  readonly options?: OptionsSchema;
  readonly implementation: Component | Readonly<Record<string, unknown>> | ((options: Options<OptionsSchema>) => Readonly<Record<string, unknown>>);
}

type Placement<OptionsSchema extends Schema | undefined> = { readonly env: Environment } &
  (OptionsSchema extends Schema ? SchemaInput<OptionsSchema> : Record<never, never>);

export function agentloop<OptionsSchema extends Schema | undefined = undefined>(
  contract: AgentloopContract<OptionsSchema>,
): (placement: Placement<OptionsSchema>) => PlacedAgentloop {
  return ((raw: unknown) => {
    const { env, options } = placedOptions(contract.options, raw);
    return branded({
      kind: "agentloop",
      implementation: typeof contract.implementation === "function" ? clone(contract.implementation(options as never)) : isComponent(contract.implementation) ? contract.implementation : clone(contract.implementation),
      configuration: clone(options),
      environment: env,
    });
  }) as (placement: Placement<OptionsSchema>) => PlacedAgentloop;
}

export interface ToolRunContext<Options, Output = unknown> extends HostToolCall<Output> {
  readonly options: Readonly<Options>;
  emit(kind: string, data: unknown): Promise<number>;
}

type ToolReturn<OutputSchema extends Schema | undefined> =
  | void
  | (OutputSchema extends Schema ? SchemaInput<OutputSchema> : unknown)
  | Outcome<OutputSchema extends Schema ? SchemaInput<OutputSchema> : unknown>;

/** One Tool: what the model is told and either a
 * function this process runs or an implementation its Environment interprets. Where it
 * runs is decided at placement, `{ env }`. */
export interface ToolContract<OptionsSchema extends Schema | undefined, InputSchema extends Schema, OutputSchema extends Schema | undefined> {
  readonly name: string;
  readonly description: string;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
  readonly options?: OptionsSchema;
  readonly run?: (
    input: SchemaOutput<InputSchema>,
    context: ToolRunContext<Options<OptionsSchema>, OutputSchema extends Schema ? SchemaInput<OutputSchema> : unknown>,
  ) => ToolReturn<OutputSchema> | Promise<ToolReturn<OutputSchema>>;
  readonly implementation?: Component | Readonly<Record<string, unknown>> | ((options: Options<OptionsSchema>) => unknown);
}

export type ToolPlacement<OptionsSchema extends Schema | undefined> = { readonly env?: Environment } &
  (OptionsSchema extends Schema ? SchemaInput<OptionsSchema> : Record<never, never>);

export type ToolFactory<OptionsSchema extends Schema | undefined, Input, Output> =
  (...args: {} extends ToolPlacement<OptionsSchema> ? [placement?: ToolPlacement<OptionsSchema>] : [placement: ToolPlacement<OptionsSchema>]) => PlacedTool<Input, Output>;

export function tool<OptionsSchema extends Schema | undefined = undefined, InputSchema extends Schema = Schema, OutputSchema extends Schema | undefined = undefined>(
  contract: ToolContract<OptionsSchema, InputSchema, OutputSchema>,
): ToolFactory<OptionsSchema, SchemaInput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown> {
  const definition = toolDefinition(contract);
  if ((typeof contract.run === "function") === ("implementation" in contract && contract.implementation !== undefined)) {
    throw new TypeError("tool needs exactly one of run or implementation");
  }
  const factory = ((raw: unknown) => {
    const { env, options } = placedOptions(contract.options, raw ?? {}, defaultHost);
    if (typeof contract.run === "function") {
      const run = contract.run;
      const hostContract: HostToolContract = {
        name: definition.name,
        description: definition.description,
        input: contract.input,
        ...(contract.output === undefined ? {} : { output: contract.output }),
      };
      return branded({
        kind: "tool",
        definition,
        implementation: undefined,
        handler: (input: unknown, call: HostToolCall) => run(input as never, { ...call, options } as never),
        contract: hostContract,
        configuration: clone(options),
        environment: env,
      });
    }
    const implementation = typeof contract.implementation === "function"
      ? contract.implementation(options as never)
      : contract.implementation;
    return branded({
      kind: "tool",
      definition,
      implementation: isComponent(implementation) ? implementation : clone(implementation as Readonly<Record<string, unknown>>),
      configuration: clone(options),
      environment: env,
    });
  }) as ToolFactory<OptionsSchema, SchemaInput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown>;
  return Object.defineProperty(factory, factorySource, { value: {
    definition,
    ...(contract.options === undefined ? {} : { optionsSchema: z.toJSONSchema(contract.options, { io: "input" }) }),
  } });
}

export interface ToolMetadata {
  readonly definition: ToolDefinition;
  readonly optionsSchema?: Readonly<Record<string, unknown>>;
}

/** Build integrations read exported factories, without evaluating an invocation. */
export function toolMetadata(factory: unknown): ToolMetadata {
  if (typeof factory !== "function" || !(factorySource in factory)) throw new TypeError("expected an exported Tool factory");
  return clone((factory as unknown as { [factorySource]: ToolMetadata })[factorySource]);
}

/** Language bindings supply JSON Schema metadata and their Environment's code reference. */
export function bindTool<Input = unknown, Output = unknown, Options extends object = Record<never, never>>(
  metadata: ToolMetadata,
  implementation: Component | Readonly<Record<string, unknown>>,
  runtime?: { readonly url: URL; readonly export: string },
): (...args: {} extends Options ? [placement?: Options & { env?: Environment }] : [placement: Options & { env?: Environment }]) => PlacedTool<Input, Output> {
  identifier(metadata.definition.name, "Tool name");
  const factory = (raw: Options & { env?: Environment } = {} as Options) => {
    if (!isRecord(raw)) throw new TypeError("Tool placement must be an object");
    const { env = defaultHost, ...options } = raw;
    inspectEnvironment(env);
    if (metadata.optionsSchema === undefined && Object.keys(options).length !== 0) throw new TypeError("this Tool does not accept options");
    const configuration = z.json().parse(options);
    return branded<PlacedTool<Input, Output>>({
      kind: "tool", definition: clone(metadata.definition), environment: env,
      configuration, implementation: isComponent(implementation) ? implementation : { ...clone(implementation), configuration },
      ...(runtime === undefined ? {} : { load: async () => {
        const module = await import(runtime.url.href);
        const exported = module[runtime.export];
        if (typeof exported !== "function") throw new TypeError(`package has no Tool export ${runtime.export}`);
        const loaded = inspectTool(exported({ ...options, env }));
        if (loaded.handler === undefined) throw new TypeError("packaged executable must define run");
        return loaded;
      } }),
    });
  };
  return Object.defineProperty(factory, factorySource, { value: clone(metadata) });
}

export async function loadHostTool(placed: PlacedTool): Promise<PlacedTool> {
  const tool = inspectTool(placed);
  return tool.load !== undefined && inspectEnvironment(tool.environment).driver.driver === "host"
    ? branded(await tool.load()) : placed;
}

export function inspectComponent(value: Component): ComponentSource {
  return inspect(value, "component");
}

export function inspectEnvironment(value: Environment): EnvironmentSource {
  return inspect(value, "environment");
}

export function inspectAgentloop(value: PlacedAgentloop): AgentloopSource {
  return inspect(value, "agentloop");
}

export function inspectTool(value: PlacedTool): ToolSource {
  return inspect(value, "tool");
}

function inspect<T extends ExtensionSource["kind"]>(value: unknown, kind: T): Extract<ExtensionSource, { kind: T }> {
  if ((typeof value !== "object" && typeof value !== "function") || value === null) throw new TypeError(`expected a Brain ${kind}`);
  const found = (value as Branded)[source];
  if (found?.kind !== kind) throw new TypeError(`expected a Brain ${kind}`);
  return found as Extract<ExtensionSource, { kind: T }>;
}

export function isComponent(value: unknown): value is Component {
  if ((typeof value !== "object" && typeof value !== "function") || value === null) return false;
  return (value as Branded)[source]?.kind === "component";
}

function branded<T>(value: ExtensionSource): T {
  return Object.freeze(Object.defineProperty({}, source, { value, enumerable: false })) as T;
}

function parseOptions(schema: Schema | undefined, raw: unknown): unknown {
  if (schema === undefined) {
    if (raw !== undefined && (!isRecord(raw) || Object.keys(raw).length !== 0)) {
      throw new TypeError("this extension does not accept options");
    }
    return Object.freeze({});
  }
  return Object.freeze(schema.parse(raw));
}

function placedOptions(schema: Schema | undefined, raw: unknown, defaultEnvironment?: Environment): { readonly env: Environment; readonly options: unknown } {
  if (!isRecord(raw) || (!("env" in raw) && defaultEnvironment === undefined)) throw new TypeError("a placed extension requires { env }");
  const env = (raw.env ?? defaultEnvironment) as Environment;
  inspectEnvironment(env);
  const { env: _environment, ...options } = raw;
  return { env, options: parseOptions(schema, options) };
}

function namedOptions(schema: Schema | undefined, raw: unknown): { readonly name: string; readonly options: unknown } {
  if (!isRecord(raw) || !("name" in raw)) throw new TypeError("an Environment requires { name }");
  identifier(raw.name, "Environment name");
  const { name, ...options } = raw;
  return { name, options: parseOptions(schema, options) };
}

function toolDefinition(contract: { readonly name: string; readonly description: string; readonly input: Schema; readonly output?: Schema }): ToolDefinition {
  identifier(contract.name, "Tool name");
  if (typeof contract.description !== "string" || contract.description.length === 0 || contract.description.length > 8_192) {
    throw new TypeError("Tool description must be 1 to 8192 characters");
  }
  if (!(contract.input instanceof z.ZodType)) throw new TypeError("Tool input must be a Zod schema");
  if (contract.output !== undefined && !(contract.output instanceof z.ZodType)) throw new TypeError("Tool output must be a Zod schema");
  return Object.freeze({
    name: contract.name,
    description: contract.description,
    inputSchema: z.toJSONSchema(contract.input, { io: "input" }) as Readonly<Record<string, unknown>>,
    ...(contract.output === undefined ? {} : { outputSchema: z.toJSONSchema(contract.output) as Readonly<Record<string, unknown>> }),
  });
}

const identifierPattern = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u;

function validateUrl(url: unknown): asserts url is string {
  let parsed: URL;
  try {
    parsed = new URL(url as string);
  } catch {
    throw new TypeError(`Environment url ${String(url)} is invalid`);
  }
  if ((parsed.protocol !== "http:" && parsed.protocol !== "https:") || parsed.username !== "" || parsed.password !== "" || parsed.search !== "" || parsed.hash !== "") {
    throw new TypeError(`Environment url ${String(url)} must be HTTP(S) without credentials, query, or fragment`);
  }
}

function identifier(value: unknown, subject: string): asserts value is string {
  if (typeof value !== "string" || !identifierPattern.test(value)) throw new TypeError(`${subject} must be an identifier`);
}

function uniqueNames(values: readonly string[], subject: string, pattern: RegExp): readonly string[] {
  if (!Array.isArray(values) || values.some((value) => typeof value !== "string" || !pattern.test(value))) {
    throw new TypeError(`${subject} contains an invalid name`);
  }
  if (new Set(values).size !== values.length) throw new TypeError(`${subject} contains a duplicate`);
  return Object.freeze([...values]);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function clone<T>(value: T): T {
  try {
    return structuredClone(value);
  } catch {
    throw new TypeError("extension configuration must be structured-cloneable");
  }
}
