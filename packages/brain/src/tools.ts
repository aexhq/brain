import { createHash } from "node:crypto";

import * as z from "zod";

import { buildToolModule, type PreparedBundle } from "./builder.js";

export type JsonSchema = Record<string, unknown>;

export interface ToolContext {
  readonly signal: AbortSignal;
  readonly operationId: string;
  readonly sessionId: string;
  readonly deadlineMs: number;
  /** Present only for Aex-managed execution. */
  readonly workspace?: string;
}

export type ToolHandler<Input extends z.ZodType, Output = unknown> = (
  input: z.output<Input>,
  context: ToolContext,
) => Output | Promise<Output>;

/** One declared outbound destination (TLS to a host on 443, matching the session contract). */
export interface NetworkDestination {
  readonly host: string;
  readonly ports: readonly [443];
  readonly protocol: "tls";
}

export interface ServerToolOptions {
  readonly env?: readonly string[];
  /**
   * The tool's declared outbound needs. Merged at session create: effective allowlist =
   * (union of tool declarations and session allows) minus session denies; Aex infra is
   * always denied. Declaration and merge only — no per-tool runtime isolation is claimed.
   */
  readonly network?: { readonly destinations: readonly NetworkDestination[] };
}

export interface ClientToolOptions {
  /** Stable registration override. Normally derived from the Tool contract. */
  readonly registration?: string;
}

export interface ToolBuilder<Input extends z.ZodType = z.ZodType, Output = unknown> {
  readonly kind: "brain.tool-builder";
  describe(description: string): ToolBuilder<Input, Output>;
  named(name: string): ToolBuilder<Input, Output>;
  returns<Schema extends z.ZodType>(schema: Schema): ToolBuilder<Input, z.output<Schema>>;
  client(options?: ClientToolOptions): Tool<Input, Output>;
  server(module: string, options?: ServerToolOptions): Tool<Input, Output>;
}

export interface ToolContract {
  readonly name: string;
  readonly description?: string;
  readonly inputSchema: JsonSchema;
  readonly outputSchema?: JsonSchema;
  readonly contractDigest: string;
}

interface AexManagedExecutor {
  readonly kind: "aex_managed";
  readonly module?: string;
  readonly prepared?: PreparedBundle;
}

interface CustomerAppExecutor {
  readonly kind: "customer_app";
  readonly registration: string;
}

interface EngineExecutor {
  readonly kind: "engine";
  readonly capability: string;
}

export type ToolExecutor =
  | AexManagedExecutor
  | CustomerAppExecutor
  | EngineExecutor;
export type ToolExecution = ToolExecutor["kind"];

export interface Tool<Input extends z.ZodType = z.ZodType, Output = unknown> {
  readonly kind: "brain.tool";
  readonly contract: ToolContract;
  readonly name: string;
  readonly description?: string;
  readonly input: Input;
  readonly output?: z.ZodType;
  readonly requiredEnv: readonly string[];
  readonly execution: ToolExecution;
  readonly executor: ToolExecutor;
  readonly handler?: ToolHandler<Input, Output>;
  /** Present only inside a bundled `.server()` module; compileTools never registers it locally. */
  readonly execute?: ToolHandler<Input, Output>;
}

interface Draft<Input extends z.ZodType, Output> {
  readonly name?: string;
  readonly description?: string;
  readonly input: Input;
  readonly output?: z.ZodType;
  readonly handler: ToolHandler<Input, Output>;
}

const TOOL_NAME = /^[A-Za-z_][A-Za-z0-9_-]{0,63}$/u;
const ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/u;
const REGISTRATION = /^[A-Za-z0-9_.:-]{1,128}$/u;
const EMPTY_INPUT = z.object({});

export function tool<Output>(
  handler: ToolHandler<typeof EMPTY_INPUT, Output>,
): ToolBuilder<typeof EMPTY_INPUT, Output>;
export function tool<Input extends z.ZodType, Output>(
  input: Input,
  handler: ToolHandler<Input, Output>,
): ToolBuilder<Input, Output>;
export function tool<Input extends z.ZodType, Output>(
  inputOrHandler: Input | ToolHandler<typeof EMPTY_INPUT, Output>,
  maybeHandler?: ToolHandler<Input, Output>,
): ToolBuilder<Input | typeof EMPTY_INPUT, Output> {
  if (typeof inputOrHandler === "function") {
    return makeBuilder({
      ...(inputOrHandler.name === "" ? {} : { name: inputOrHandler.name }),
      input: EMPTY_INPUT,
      handler: inputOrHandler,
    });
  }
  if (typeof maybeHandler !== "function") throw new TypeError("tool(schema, handler) requires a function");
  return makeBuilder({
    ...(maybeHandler.name === "" ? {} : { name: maybeHandler.name }),
    input: inputOrHandler,
    handler: maybeHandler,
  });
}

function makeBuilder<Input extends z.ZodType, Output>(draft: Draft<Input, Output>): ToolBuilder<Input, Output> {
  return Object.freeze({
    kind: "brain.tool-builder" as const,
    describe(description: string) {
      assertDescription(description);
      return makeBuilder({ ...draft, description });
    },
    named(name: string) {
      assertName(name);
      return makeBuilder({ ...draft, name });
    },
    returns<Schema extends z.ZodType>(output: Schema) {
      schemaOf(output, `${draft.name ?? "Tool"} output`);
      return makeBuilder({ ...draft, output }) as unknown as ToolBuilder<Input, z.output<Schema>>;
    },
    client(options: ClientToolOptions = {}) {
      const contract = compileContract(draft);
      const registration = options.registration ?? `tool:${contract.contractDigest}`;
      if (!REGISTRATION.test(registration)) throw new TypeError("Tool registration is invalid");
      return freezeTool({
        draft,
        contract,
        requiredEnv: [],
        executor: { kind: "customer_app", registration },
        handlerKey: "handler",
      });
    },
    server(module: string, options: ServerToolOptions = {}) {
      if (module.trim() === "") throw new TypeError("server(module) requires import.meta.url");
      const contract = compileContract(draft);
      return freezeTool({
        draft,
        contract,
        requiredEnv: normalizeRequiredEnv(options.env),
        executor: { kind: "aex_managed", module },
        handlerKey: "execute",
        ...(options.network === undefined ? {} : { network: normalizeNetwork(options.network) }),
      });
    },
  });
}

interface FreezeOptions<Input extends z.ZodType, Output> {
  draft: Draft<Input, Output>;
  contract: ToolContract;
  requiredEnv: readonly string[];
  executor: ToolExecutor;
  network?: { readonly destinations: readonly NetworkDestination[] };
  handlerKey?: "handler" | "execute";
}

function freezeTool<Input extends z.ZodType, Output>(options: FreezeOptions<Input, Output>): Tool<Input, Output> {
  return Object.freeze({
    kind: "brain.tool" as const,
    contract: Object.freeze(options.contract),
    name: options.contract.name,
    ...(options.contract.description === undefined ? {} : { description: options.contract.description }),
    input: options.draft.input,
    ...(options.draft.output === undefined ? {} : { output: options.draft.output }),
    requiredEnv: Object.freeze([...options.requiredEnv]),
    execution: options.executor.kind,
    executor: Object.freeze(options.executor),
    ...(options.network === undefined ? {} : { network: Object.freeze(options.network) }),
    ...(options.handlerKey === undefined ? {} : { [options.handlerKey]: options.draft.handler }),
  });
}

function compileContract<Input extends z.ZodType, Output>(draft: Draft<Input, Output>): ToolContract {
  const name = draft.name;
  if (name === undefined) {
    throw new TypeError("Tool functions must be named; use .named(name) when build tooling removes the name");
  }
  assertName(name);
  const inputSchema = schemaOf(draft.input, `${name} input`);
  const outputSchema = draft.output === undefined ? undefined : schemaOf(draft.output, `${name} output`);
  const canonical = {
    name,
    ...(draft.description === undefined ? {} : { description: draft.description }),
    input_schema: inputSchema,
    ...(outputSchema === undefined ? {} : { output_schema: outputSchema }),
  };
  return {
    name,
    ...(draft.description === undefined ? {} : { description: draft.description }),
    inputSchema,
    ...(outputSchema === undefined ? {} : { outputSchema }),
    contractDigest: createHash("sha256").update(canonicalJson(canonical)).digest("hex"),
  };
}

export interface ClientRegistration {
  readonly registration: string;
  readonly name: string;
  readonly contractDigest: string;
  readonly input: z.ZodType;
  readonly output?: z.ZodType;
  readonly handler: ToolHandler<z.ZodType>;
}

export interface WireToolDefinition {
  name: string;
  description?: string;
  input_schema: JsonSchema;
  output_schema?: JsonSchema;
  contract_digest: string;
}

export type WireToolExecutor =
  | { kind: "aex_managed"; bundle_digest: string; required_env: string[] }
  | { kind: "customer_app"; registration: string }
  | { kind: "engine"; capability: string };

export interface WireTool {
  definition: WireToolDefinition;
  executor: WireToolExecutor;
  network?: { destinations: NetworkDestination[] };
}

export interface WireToolBundle {
  checksum: string;
  content_base64: string;
  bytes: number;
  media_type: "application/javascript+esm";
}

export interface CompiledTools {
  readonly items: WireTool[];
  readonly bundles: WireToolBundle[];
  readonly clientRegistrations: readonly ClientRegistration[];
}

export const MAX_TOOL_BUNDLE_BYTES = 4 * 1024 * 1024;
export const MAX_SESSION_BUNDLE_BYTES = 16 * 1024 * 1024;
const managedBundleCache = new WeakMap<Tool, Promise<{ checksum: string; bundle: PreparedBundle }>>();

/** Compile the ordered immutable Tool grant once at session creation. */
export async function compileTools(selections: readonly Tool[] | undefined): Promise<CompiledTools> {
  const selected = [...(selections ?? [])];
  const items: WireTool[] = [];
  const bundles: WireToolBundle[] = [];
  const clientRegistrations: ClientRegistration[] = [];
  const names = new Set<string>();
  const registrations = new Set<string>();
  let totalBundleBytes = 0;

  for (const value of selected) {
    assertTool(value);
    if (names.has(value.name)) throw new TypeError(`Brain Tool ${value.name} was selected more than once`);
    names.add(value.name);
  }
  const preparedManaged = await Promise.all(selected.map((value) =>
    value.executor.kind === "aex_managed" ? prepareManagedTool(value, value.executor) : undefined));

  for (const [index, value] of selected.entries()) {
    const definition: WireToolDefinition = {
      name: value.name,
      ...(value.description === undefined ? {} : { description: value.description }),
      input_schema: value.contract.inputSchema,
      ...(value.contract.outputSchema === undefined ? {} : { output_schema: value.contract.outputSchema }),
      contract_digest: value.contract.contractDigest,
    };
    let executor: WireToolExecutor;
    switch (value.executor.kind) {
      case "aex_managed": {
        const prepared = preparedManaged[index];
        if (prepared === undefined) throw new TypeError(`Brain Tool ${value.name} bundle preparation was lost`);
        executor = {
          kind: "aex_managed",
          bundle_digest: prepared.checksum,
          required_env: [...value.requiredEnv],
        };
        if (prepared.bundle !== undefined) {
          totalBundleBytes += prepared.bundle.bytes.byteLength;
          if (prepared.bundle.bytes.byteLength > MAX_TOOL_BUNDLE_BYTES) {
            throw new TypeError(`Brain Tool ${value.name} bundle exceeds ${MAX_TOOL_BUNDLE_BYTES} bytes`);
          }
          if (totalBundleBytes > MAX_SESSION_BUNDLE_BYTES) {
            throw new TypeError(`Selected Tool bundles exceed ${MAX_SESSION_BUNDLE_BYTES} bytes`);
          }
          bundles.push({
            checksum: prepared.bundle.checksum,
            content_base64: Buffer.from(prepared.bundle.bytes).toString("base64"),
            bytes: prepared.bundle.bytes.byteLength,
            media_type: "application/javascript+esm",
          });
        }
        break;
      }
      case "customer_app": {
        if (registrations.has(value.executor.registration)) {
          throw new TypeError(`Customer Tool registration ${value.executor.registration} was selected twice`);
        }
        registrations.add(value.executor.registration);
        if (value.handler === undefined) throw new TypeError(`Customer Tool ${value.name} has no handler`);
        executor = { kind: "customer_app", registration: value.executor.registration };
        clientRegistrations.push(Object.freeze({
          registration: value.executor.registration,
          name: value.name,
          contractDigest: value.contract.contractDigest,
          input: value.input,
          ...(value.output === undefined ? {} : { output: value.output }),
          handler: value.handler as ToolHandler<z.ZodType>,
        }));
        break;
      }
      case "engine":
        executor = { kind: "engine", capability: value.executor.capability };
        break;
    }
    const declared = (value as { network?: { destinations: readonly NetworkDestination[] } }).network;
    items.push({
      definition,
      executor,
      ...(declared === undefined
        ? {}
        : { network: { destinations: declared.destinations.map((destination) => ({ ...destination })) } }),
    });
  }
  return { items, bundles, clientRegistrations: Object.freeze(clientRegistrations) };
}

async function prepareManagedTool(
  value: Tool,
  executor: AexManagedExecutor,
): Promise<{ checksum: string; bundle: PreparedBundle }> {
  const cached = managedBundleCache.get(value);
  if (cached !== undefined) return await cached;
  const pending = (async () => {
    if (executor.prepared !== undefined) {
      assertSha256(executor.prepared.checksum, "prepared bundle checksum");
      const actual = createHash("sha256").update(executor.prepared.bytes).digest("hex");
      if (actual !== executor.prepared.checksum) throw new TypeError(`Brain Tool ${value.name} bundle checksum is invalid`);
      return { checksum: actual, bundle: executor.prepared };
    }
    if (executor.module === undefined) throw new TypeError(`Brain Tool ${value.name} has no server module`);
    const bundle = await buildToolModule(executor.module);
    return { checksum: bundle.checksum, bundle };
  })();
  managedBundleCache.set(value, pending);
  try {
    return await pending;
  } catch (error) {
    if (managedBundleCache.get(value) === pending) managedBundleCache.delete(value);
    throw error;
  }
}

/** Internal constructor for Brain-owned fixed capability Tools. */
export function capabilityTool<Input extends z.ZodType, Output = unknown>(options: {
  name: string;
  description: string;
  input: Input;
  output?: z.ZodType;
  capability: string;
}): Tool<Input, Output> {
  const draft: Draft<Input, Output> = {
    name: options.name,
    description: options.description,
    input: options.input,
    ...(options.output === undefined ? {} : { output: options.output }),
    handler: (() => { throw new TypeError("engine capability cannot execute in the SDK"); }) as ToolHandler<Input, Output>,
  };
  return freezeTool({
    draft,
    contract: compileContract(draft),
    requiredEnv: [],
    executor: { kind: "engine", capability: options.capability },
  });
}

/** Brain-owned name for a fixed, server-authorized capability Tool. @internal */
export const officialTool = capabilityTool;

function schemaOf(schema: z.ZodType, label: string): JsonSchema {
  try {
    const value = z.toJSONSchema(schema, { target: "draft-2020-12", unrepresentable: "throw" }) as JsonSchema;
    if (Object.keys(value).length === 0) throw new TypeError(`${label} schema is empty`);
    return value;
  } catch (cause) {
    throw new TypeError(`${label} cannot be represented as JSON Schema`, { cause });
  }
}

/** RFC 8785 JSON Canonicalization Scheme for Tool contracts. @internal */
export function canonicalJson(value: unknown): string {
  return canonicalJsonInner(value, new Set<object>());
}

function canonicalJsonInner(value: unknown, ancestors: Set<object>): string {
  if (value === null) return "null";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "string") {
    assertUnicodeScalarString(value);
    return JSON.stringify(value);
  }
  if (typeof value === "number") {
    if (!Number.isFinite(value)) throw new TypeError("Canonical JSON cannot contain a non-finite number");
    return JSON.stringify(value);
  }
  if (typeof value !== "object") {
    throw new TypeError(`Canonical JSON cannot contain ${typeof value}`);
  }
  if (ancestors.has(value)) throw new TypeError("Canonical JSON cannot contain a cycle");
  ancestors.add(value);
  try {
    if (Array.isArray(value)) {
      const items: string[] = [];
      for (let index = 0; index < value.length; index += 1) {
        if (!(index in value)) throw new TypeError("Canonical JSON cannot contain a sparse array");
        items.push(canonicalJsonInner(value[index], ancestors));
      }
      return `[${items.join(",")}]`;
    }
    const prototype = Object.getPrototypeOf(value) as unknown;
    if (prototype !== Object.prototype && prototype !== null) {
      throw new TypeError("Canonical JSON requires plain JSON objects");
    }
    const entries = Object.entries(value as Record<string, unknown>);
    for (const [key] of entries) assertUnicodeScalarString(key);
    // ECMAScript relational string comparison is lexicographic UTF-16 code-unit order, the
    // ordering RFC 8785 requires. It is intentionally independent of ICU and host locale.
    entries.sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0);
    return `{${entries
      .map(([key, item]) => `${JSON.stringify(key)}:${canonicalJsonInner(item, ancestors)}`)
      .join(",")}}`;
  } finally {
    ancestors.delete(value);
  }
}

function assertUnicodeScalarString(value: string): void {
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) {
        throw new TypeError("Canonical JSON cannot contain an unpaired UTF-16 surrogate");
      }
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      throw new TypeError("Canonical JSON cannot contain an unpaired UTF-16 surrogate");
    }
  }
}

function assertTool(value: Tool): void {
  if (value?.kind !== "brain.tool") throw new TypeError("Tool builders must end in .client() or .server()");
  assertName(value.name);
}

function assertName(name: string): void {
  if (!TOOL_NAME.test(name)) throw new TypeError(`Invalid Brain Tool name ${JSON.stringify(name)}`);
}

function assertDescription(description: string): void {
  if (description.trim() === "" || description.length > 4096) {
    throw new TypeError("Tool description must contain 1 through 4096 characters");
  }
}

function normalizeNetwork(
  network: NonNullable<ServerToolOptions["network"]>,
): { readonly destinations: readonly NetworkDestination[] } {
  const destinations = network.destinations ?? [];
  if (destinations.length === 0 || destinations.length > 32) {
    throw new TypeError("Tool network declarations need 1 through 32 destinations");
  }
  const seen = new Set<string>();
  for (const destination of destinations) {
    const host = destination.host;
    if (typeof host !== "string" || host.length === 0 || host.length > 253) {
      throw new TypeError("Tool network destination host must be 1 through 253 bytes");
    }
    const lowered = host.toLowerCase();
    if (lowered === "aex.dev" || lowered.endsWith(".aex.dev")) {
      throw new TypeError(`Tool network destination ${host} names Aex infrastructure; Aex infra is always denied`);
    }
    if (seen.has(lowered)) throw new TypeError(`Tool network destination ${host} was repeated`);
    seen.add(lowered);
  }
  return { destinations: destinations.map((destination) => Object.freeze({ ...destination })) };
}

function normalizeRequiredEnv(values: readonly string[] | undefined): readonly string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const value of values ?? []) {
    if (!ENV_NAME.test(value)) throw new TypeError(`Invalid required environment name ${JSON.stringify(value)}`);
    if (seen.has(value)) throw new TypeError(`Required environment name ${value} was repeated`);
    seen.add(value);
    out.push(value);
  }
  return out;
}

function assertSha256(value: string, label: string): void {
  if (!/^[0-9a-f]{64}$/u.test(value)) throw new TypeError(`${label} must be lower-case SHA-256 hex`);
}
