import { z } from "zod";

import type { AppToolCall, AppToolContract } from "./app.js";
import type {
  AgentloopBinding, Component, Environment, ResourceName, Schema, SchemaInput, SchemaOutput,
  ToolBinding, ToolDefinition,
} from "./types.js";

const source = Symbol.for("@aexhq/brain/extension-source");

interface ComponentSource {
  readonly kind: "component";
  readonly artifact: URL | Uint8Array;
}

interface EnvironmentSource {
  readonly kind: "environment";
  readonly configuration: unknown;
  readonly managed: boolean;
  readonly idleTtlMs?: number;
  readonly bindings: Readonly<Record<string, string>>;
}

interface AgentloopSource {
  readonly kind: "agentloop";
  readonly component: Component;
  readonly configuration: unknown;
  readonly environment: Environment;
}

interface ResidentToolSource {
  readonly kind: "resident_tool";
  readonly definition: ToolDefinition;
  readonly contract: AppToolContract;
  readonly handler: (input: unknown, call: AppToolCall) => unknown;
}

interface PlacedToolSource {
  readonly kind: "placed_tool";
  readonly definition: ToolDefinition;
  readonly needs: readonly ResourceName[];
  readonly bindingNames: readonly string[];
  readonly implementation: unknown;
  readonly configuration: unknown;
  readonly environment: Environment;
}

type ExtensionSource = ComponentSource | EnvironmentSource | AgentloopSource | ResidentToolSource | PlacedToolSource;
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

export interface EnvironmentContract<OptionsSchema extends Schema | undefined = undefined> {
  readonly driver: string;
  readonly options?: OptionsSchema;
  readonly managed?: boolean;
  readonly idleTtlMs?: number;
  readonly configure?: (options: OptionsSchema extends Schema ? SchemaOutput<OptionsSchema> : Record<string, never>) => unknown;
  readonly bindings?: (options: OptionsSchema extends Schema ? SchemaOutput<OptionsSchema> : Record<string, never>) => Readonly<Record<string, string>>;
}

type OptionalFactory<OptionsSchema extends Schema | undefined, Value> =
  OptionsSchema extends Schema
    ? undefined extends SchemaInput<OptionsSchema>
      ? (options?: SchemaInput<OptionsSchema>) => Value
      : (options: SchemaInput<OptionsSchema>) => Value
    : (options?: undefined) => Value;

export function environment<OptionsSchema extends Schema | undefined = undefined>(
  contract: EnvironmentContract<OptionsSchema>,
): OptionalFactory<OptionsSchema, Environment> {
  identifier(contract.driver, "Environment driver");
  boundedInteger(contract.idleTtlMs, "Environment idleTtlMs");
  return ((raw?: unknown) => {
    const options = parseOptions(contract.options, raw);
    const configured = contract.configure?.(options as never) ?? options;
    const configuration = clone({ driver: contract.driver, ...(isRecord(configured) ? configured : { options: configured }) });
    const bindings = contract.bindings?.(options as never) ?? {};
    validateBindings(bindings);
    return branded({
      kind: "environment",
      configuration,
      managed: contract.managed ?? true,
      ...(contract.idleTtlMs === undefined ? {} : { idleTtlMs: contract.idleTtlMs }),
      bindings: Object.freeze({ ...bindings }),
    });
  }) as OptionalFactory<OptionsSchema, Environment>;
}

export interface BrainWasmOptions {
  readonly network?: { readonly allow: readonly string[] };
  readonly filesystem?: { readonly scratch?: boolean; readonly workspace?: boolean };
  readonly secrets?: readonly string[];
}

/** Brain's one built-in native placement: a fresh Wasmtime instance per invocation. */
export function brainWasm(options: BrainWasmOptions = {}): Environment {
  const rawAllow = options.network?.allow ?? [];
  if (!Array.isArray(rawAllow) || rawAllow.some((value) => typeof value !== "string" || value.length === 0)) {
    throw new TypeError("brainWasm network allow entries must be non-empty strings");
  }
  const allow = rawAllow.map(normalizeNetworkTarget);
  const secrets = options.secrets ?? [];
  if (!Array.isArray(secrets) || secrets.some((value) => typeof value !== "string" || !identifierPattern.test(value))) {
    throw new TypeError("brainWasm secrets must be identifiers");
  }
  const filesystem = options.filesystem;
  if (filesystem !== undefined && (
    !isRecord(filesystem)
    || Object.keys(filesystem).some((name) => name !== "scratch" && name !== "workspace")
    || (filesystem.scratch !== undefined && typeof filesystem.scratch !== "boolean")
    || (filesystem.workspace !== undefined && typeof filesystem.workspace !== "boolean")
  )) {
    throw new TypeError("brainWasm filesystem accepts boolean scratch and workspace options");
  }
  return branded({
    kind: "environment",
    configuration: clone({
      driver: "brain_wasm",
      network: { allow: [...allow] },
      filesystem: {
        scratch: filesystem?.scratch ?? false,
        workspace: filesystem?.workspace ?? false,
      },
      secrets: [...secrets],
    }),
    managed: true,
    bindings: Object.freeze({}),
  });
}

export interface AgentloopContract<OptionsSchema extends Schema | undefined = undefined> {
  readonly options?: OptionsSchema;
  readonly implementation: Component;
}

type Placement<OptionsSchema extends Schema | undefined> = { readonly env: Environment } &
  (OptionsSchema extends Schema ? SchemaInput<OptionsSchema> : Record<never, never>);

export function agentloop<OptionsSchema extends Schema | undefined = undefined>(
  contract: AgentloopContract<OptionsSchema>,
): (placement: Placement<OptionsSchema>) => AgentloopBinding {
  inspectComponent(contract.implementation);
  return ((raw: unknown) => {
    const { env, options } = placedOptions(contract.options, raw);
    return branded({
      kind: "agentloop",
      component: contract.implementation,
      configuration: clone(options),
      environment: env,
    });
  }) as (placement: Placement<OptionsSchema>) => AgentloopBinding;
}

export interface ToolRunContext<Options> extends AppToolCall {
  readonly options: Readonly<Options>;
  emit(kind: string, data: unknown): Promise<number>;
}

interface ToolContractBase<OptionsSchema extends Schema | undefined, InputSchema extends Schema, OutputSchema extends Schema | undefined> {
  readonly name: string;
  readonly description: string;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
  readonly options?: OptionsSchema;
}

export interface ResidentToolContract<OptionsSchema extends Schema | undefined, InputSchema extends Schema, OutputSchema extends Schema | undefined>
  extends ToolContractBase<OptionsSchema, InputSchema, OutputSchema> {
  readonly run: (
    input: SchemaOutput<InputSchema>,
    context: ToolRunContext<OptionsSchema extends Schema ? SchemaOutput<OptionsSchema> : Record<string, never>>,
  ) => (OutputSchema extends Schema ? SchemaInput<OutputSchema> : unknown) | Promise<OutputSchema extends Schema ? SchemaInput<OutputSchema> : unknown>;
  readonly implementation?: never;
  readonly needs?: never;
  readonly bindingNames?: never;
}

export interface PlacedToolContract<OptionsSchema extends Schema | undefined, InputSchema extends Schema, OutputSchema extends Schema | undefined>
  extends ToolContractBase<OptionsSchema, InputSchema, OutputSchema> {
  readonly implementation: Component | Readonly<Record<string, unknown>> | ((options: OptionsSchema extends Schema ? SchemaOutput<OptionsSchema> : Record<string, never>) => unknown);
  readonly needs?: readonly ResourceName[];
  readonly bindingNames?: readonly string[];
  readonly run?: never;
}

export function tool<OptionsSchema extends Schema | undefined = undefined, InputSchema extends Schema = Schema, OutputSchema extends Schema | undefined = undefined>(
  contract: ResidentToolContract<OptionsSchema, InputSchema, OutputSchema>,
): OptionalFactory<OptionsSchema, ToolBinding<SchemaInput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown>>;
export function tool<OptionsSchema extends Schema | undefined = undefined, InputSchema extends Schema = Schema, OutputSchema extends Schema | undefined = undefined>(
  contract: PlacedToolContract<OptionsSchema, InputSchema, OutputSchema>,
): (placement: Placement<OptionsSchema>) => ToolBinding<SchemaInput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown>;
export function tool(contract: ResidentToolContract<Schema | undefined, Schema, Schema | undefined> | PlacedToolContract<Schema | undefined, Schema, Schema | undefined>): Function {
  const definition = toolDefinition(contract);
  if (typeof contract.run === "function") {
    return (raw?: unknown) => {
      const options = parseOptions(contract.options, raw);
      const appContract: AppToolContract = {
        name: definition.name,
        description: definition.description,
        input: contract.input,
        ...(contract.output === undefined ? {} : { output: contract.output }),
      };
      return branded({
        kind: "resident_tool",
        definition,
        contract: appContract,
        handler: (input: unknown, call: AppToolCall) => contract.run(input, { ...call, options } as never),
      });
    };
  }
  if (!("implementation" in contract)) throw new TypeError("tool needs exactly one of run or implementation");
  const needs = uniqueNames(contract.needs ?? [], "Tool needs", resourcePattern);
  const bindingNames = uniqueNames(contract.bindingNames ?? [], "Tool bindingNames", identifierPattern);
  return (raw: unknown) => {
    const { env, options } = placedOptions(contract.options, raw);
    const implementation = typeof contract.implementation === "function"
      ? contract.implementation(options)
      : contract.implementation;
    return branded({
      kind: "placed_tool",
      definition,
      needs,
      bindingNames,
      implementation: isComponent(implementation) ? implementation : clone(implementation),
      configuration: clone(options),
      environment: env,
    });
  };
}

export function inspectComponent(value: Component): ComponentSource {
  return inspect(value, "component");
}

export function inspectEnvironment(value: Environment): EnvironmentSource {
  return inspect(value, "environment");
}

export function inspectAgentloop(value: AgentloopBinding): AgentloopSource {
  return inspect(value, "agentloop");
}

export function inspectResidentTool(value: unknown): ResidentToolSource | undefined {
  return inspectOptional(value, "resident_tool");
}

export function inspectPlacedTool(value: ToolBinding): PlacedToolSource {
  return inspect(value, "placed_tool");
}

function inspect<T extends ExtensionSource["kind"]>(value: unknown, kind: T): Extract<ExtensionSource, { kind: T }> {
  const found = inspectOptional(value, kind);
  if (found === undefined) throw new TypeError(`expected a Brain ${kind.replaceAll("_", " ")}`);
  return found;
}

function inspectOptional<T extends ExtensionSource["kind"]>(value: unknown, kind: T): Extract<ExtensionSource, { kind: T }> | undefined {
  if ((typeof value !== "object" && typeof value !== "function") || value === null) return undefined;
  const found = (value as Branded)[source];
  return found?.kind === kind ? found as Extract<ExtensionSource, { kind: T }> : undefined;
}

function isComponent(value: unknown): value is Component {
  return inspectOptional(value, "component") !== undefined;
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
const resourcePattern = /^[a-z][a-z0-9_]{0,63}(?::[A-Za-z0-9._-]{1,64})?$/u;

function normalizeNetworkTarget(value: string): string {
  if (typeof value !== "string" || value.length === 0) throw new TypeError("brainWasm network allow entries must be non-empty strings");
  const explicit = value.includes("://");
  let parsed: URL;
  try {
    parsed = new URL(explicit ? value : `https://${value}`);
  } catch {
    throw new TypeError(`brainWasm network target ${value} is invalid`);
  }
  if ((parsed.protocol !== "http:" && parsed.protocol !== "https:") || parsed.username !== "" || parsed.password !== "" || parsed.pathname !== "/" || parsed.search !== "" || parsed.hash !== "") {
    throw new TypeError(`brainWasm network target ${value} must be an HTTP(S) origin or authority`);
  }
  return explicit ? parsed.origin : parsed.host;
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

function validateBindings(bindings: Readonly<Record<string, string>>): void {
  if (!isRecord(bindings)) throw new TypeError("Environment bindings must be an object");
  for (const [name, value] of Object.entries(bindings)) {
    identifier(name, "Environment binding name");
    if (typeof value !== "string" || value.length > 32_768) throw new TypeError(`Environment binding ${name} must be a bounded string`);
  }
}

function boundedInteger(value: number | undefined, subject: string): void {
  if (value !== undefined && (!Number.isSafeInteger(value) || value < 0)) throw new TypeError(`${subject} must be a non-negative safe integer`);
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
