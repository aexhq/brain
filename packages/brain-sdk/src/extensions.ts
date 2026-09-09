import { z } from "zod";

import type { HostToolCall, HostToolContract } from "./host.js";
import type {
  Component, Environment, PlacedAgentloop, PlacedTool, Schema, SchemaInput, SchemaOutput,
  ToolDefinition,
} from "./types.js";

const source = Symbol.for("@aexhq/brain/extension-source");

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

export interface ToolRunContext<Options> extends HostToolCall {
  readonly options: Readonly<Options>;
  emit(kind: string, data: unknown): Promise<number>;
}

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
    context: ToolRunContext<Options<OptionsSchema>>,
  ) => (OutputSchema extends Schema ? SchemaInput<OutputSchema> : unknown) | Promise<OutputSchema extends Schema ? SchemaInput<OutputSchema> : unknown>;
  readonly implementation?: Component | Readonly<Record<string, unknown>> | ((options: Options<OptionsSchema>) => unknown);
}

export function tool<OptionsSchema extends Schema | undefined = undefined, InputSchema extends Schema = Schema, OutputSchema extends Schema | undefined = undefined>(
  contract: ToolContract<OptionsSchema, InputSchema, OutputSchema>,
): (placement: Placement<OptionsSchema>) => PlacedTool<SchemaInput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown> {
  const definition = toolDefinition(contract);
  if ((typeof contract.run === "function") === ("implementation" in contract && contract.implementation !== undefined)) {
    throw new TypeError("tool needs exactly one of run or implementation");
  }
  return ((raw: unknown) => {
    const { env, options } = placedOptions(contract.options, raw);
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
  }) as (placement: Placement<OptionsSchema>) => PlacedTool<SchemaInput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown>;
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

function placedOptions(schema: Schema | undefined, raw: unknown): { readonly env: Environment; readonly options: unknown } {
  if (!isRecord(raw) || !("env" in raw)) throw new TypeError("a placed extension requires { env }");
  const env = raw.env as Environment;
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
    inputSchema: z.toJSONSchema(contract.input) as Readonly<Record<string, unknown>>,
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
