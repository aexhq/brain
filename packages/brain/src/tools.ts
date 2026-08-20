import { createHash, randomUUID } from "node:crypto";

import * as z from "zod";

import { buildToolModule, type PreparedBundle } from "./builder.js";

export type JsonSchema = Record<string, unknown>;

export interface ToolContext {
  readonly signal: AbortSignal;
  readonly callId: string;
  readonly workspace: string;
  readonly deadlineMs: number;
}

export type ToolHandler<Input extends z.ZodType, Output extends z.ZodType> = (
  input: z.output<Input>,
  context: ToolContext,
) => z.input<Output> | Promise<z.input<Output>>;

export interface ToolDefinition {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: JsonSchema;
  readonly outputSchema: JsonSchema;
}

export type ToolExecution = "hand" | "attached" | "server" | "intrinsic" | "mcp";

interface HandExecutor {
  readonly kind: "hand";
  readonly module?: string;
  readonly prepared?: PreparedBundle;
  readonly preinstalledChecksum?: string;
}

interface AttachedExecutor {
  readonly kind: "attached";
  readonly callbackId: string;
}

interface ServerExecutor {
  readonly kind: "server";
  readonly capability: string;
  readonly completion: "continue" | "return_direct";
  readonly effect: "opaque" | "replay_safe";
  readonly scope: "root" | "all";
  readonly maxInputBytes: number;
}

interface IntrinsicExecutor {
  readonly kind: "intrinsic";
  readonly capability: string;
}

interface McpExecutor {
  readonly kind: "mcp";
  readonly server: string;
  readonly remoteName: string;
}

export type ToolExecutor =
  | HandExecutor
  | AttachedExecutor
  | ServerExecutor
  | IntrinsicExecutor
  | McpExecutor;

export interface Tool<Input extends z.ZodType = z.ZodType, Output extends z.ZodType = z.ZodType> {
  readonly kind: "brain.tool";
  readonly name: string;
  readonly description: string;
  readonly input: Input;
  readonly output: Output;
  readonly requiredEnv: readonly string[];
  readonly execution: ToolExecution;
  readonly executor: ToolExecutor;
  readonly execute?: ToolHandler<Input, Output>;
  /** Return an explicit attached-process selection of this tool. */
  local(options?: { callbackId?: string }): Tool<Input, Output>;
}

export interface DefineToolOptions<Input extends z.ZodType, Output extends z.ZodType> {
  /** Explicit source-module identity. Required for the default Hand execution mode. */
  readonly module?: string;
  readonly name: string;
  readonly description: string;
  readonly input: Input;
  readonly output: Output;
  readonly requiredEnv?: readonly string[];
  readonly execute: ToolHandler<Input, Output>;
}

const TOOL_NAME = /^[A-Za-z_][A-Za-z0-9_-]{0,63}$/u;
const ENV_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/u;

export function defineTool<Input extends z.ZodType, Output extends z.ZodType>(
  options: DefineToolOptions<Input, Output>,
): Tool<Input, Output> {
  assertDefinition(options.name, options.description);
  const requiredEnv = normalizeRequiredEnv(options.requiredEnv);
  return makeTool({
    name: options.name,
    description: options.description,
    input: options.input,
    output: options.output,
    requiredEnv,
    executor: {
      kind: "hand",
      ...(options.module === undefined ? {} : { module: options.module }),
    },
    execute: options.execute,
  });
}

export interface DefineServerToolOptions<Input extends z.ZodType, Output extends z.ZodType> {
  readonly name: string;
  readonly description: string;
  readonly input: Input;
  readonly output: Output;
  readonly capability: string;
  readonly completion?: "continue" | "return_direct";
  readonly effect?: "opaque" | "replay_safe";
  readonly scope?: "root" | "all";
  readonly maxInputBytes?: number;
}

/** Define a model-visible value backed by a trusted capability registered in the server binary. */
export function defineServerTool<Input extends z.ZodType, Output extends z.ZodType>(
  options: DefineServerToolOptions<Input, Output>,
): Tool<Input, Output> {
  assertDefinition(options.name, options.description);
  if (options.capability.trim() === "") throw new TypeError("Tool capability cannot be empty");
  const maxInputBytes = options.maxInputBytes ?? 98_304;
  if (!Number.isSafeInteger(maxInputBytes) || maxInputBytes < 1 || maxInputBytes > 98_304) {
    throw new TypeError("maxInputBytes must be an integer from 1 through 98304");
  }
  return makeTool({
    name: options.name,
    description: options.description,
    input: options.input,
    output: options.output,
    requiredEnv: [],
    executor: {
      kind: "server",
      capability: options.capability,
      completion: options.completion ?? "continue",
      effect: options.effect ?? "opaque",
      scope: options.scope ?? "all",
      maxInputBytes,
    },
  });
}

export interface DefineIntrinsicToolOptions<Input extends z.ZodType, Output extends z.ZodType> {
  readonly name: string;
  readonly description: string;
  readonly input: Input;
  readonly output: Output;
  readonly capability: string;
}

export function defineIntrinsicTool<Input extends z.ZodType, Output extends z.ZodType>(
  options: DefineIntrinsicToolOptions<Input, Output>,
): Tool<Input, Output> {
  assertDefinition(options.name, options.description);
  return makeTool({
    ...options,
    requiredEnv: [],
    executor: { kind: "intrinsic", capability: options.capability },
  });
}

export interface DefinePreinstalledToolOptions<Input extends z.ZodType, Output extends z.ZodType> {
  readonly name: string;
  readonly description: string;
  readonly input: Input;
  readonly output: Output;
  readonly checksum: string;
  readonly requiredEnv?: readonly string[];
}

/** Define an ordinary Tool whose executable is already present in a compatible Hand image. */
export function definePreinstalledTool<Input extends z.ZodType, Output extends z.ZodType>(
  options: DefinePreinstalledToolOptions<Input, Output>,
): Tool<Input, Output> {
  assertDefinition(options.name, options.description);
  assertSha256(options.checksum, "preinstalled checksum");
  return makeTool({
    name: options.name,
    description: options.description,
    input: options.input,
    output: options.output,
    requiredEnv: normalizeRequiredEnv(options.requiredEnv),
    executor: { kind: "hand", preinstalledChecksum: options.checksum },
  });
}

interface MakeToolOptions<Input extends z.ZodType, Output extends z.ZodType> {
  name: string;
  description: string;
  input: Input;
  output: Output;
  requiredEnv: readonly string[];
  executor: ToolExecutor;
  execute?: ToolHandler<Input, Output>;
}

function makeTool<Input extends z.ZodType, Output extends z.ZodType>(
  options: MakeToolOptions<Input, Output>,
): Tool<Input, Output> {
  const local = (localOptions: { callbackId?: string } = {}): Tool<Input, Output> => {
    if (options.execute === undefined) {
      throw new TypeError(`Tool ${options.name} has no callback implementation`);
    }
    const callbackId = localOptions.callbackId ?? `cb_${randomUUID().replaceAll("-", "")}`;
    return makeTool({ ...options, executor: { kind: "attached", callbackId } });
  };
  return Object.freeze({
    kind: "brain.tool" as const,
    name: options.name,
    description: options.description,
    input: options.input,
    output: options.output,
    requiredEnv: Object.freeze([...options.requiredEnv]),
    execution: options.executor.kind,
    executor: Object.freeze(options.executor),
    ...(options.execute === undefined ? {} : { execute: options.execute }),
    local,
  });
}

export interface WireToolDefinition {
  name: string;
  description: string;
  input_schema: JsonSchema;
  output_schema: JsonSchema;
}

export type WireToolExecutor =
  | { kind: "hand"; protocol: 1; checksum: string; source: "bundle" | "preinstalled"; required_env: string[] }
  | { kind: "attached"; callback_id: string }
  | {
      kind: "server";
      capability: string;
      completion: "continue" | "return_direct";
      effect: "opaque" | "replay_safe";
      scope: "root" | "all";
      max_input_bytes: number;
    }
  | { kind: "intrinsic"; capability: string }
  | { kind: "mcp"; server: string; remote_name: string };

export interface WireTool {
  definition: WireToolDefinition;
  executor: WireToolExecutor;
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
  readonly attached: ReadonlyMap<string, Tool>;
}

export const MAX_TOOL_BUNDLE_BYTES = 4 * 1024 * 1024;
export const MAX_SESSION_BUNDLE_BYTES = 16 * 1024 * 1024;

/** Build and seal the exact ordered tools sent in one create-session request. */
export async function compileTools(selections: readonly Tool[] | undefined): Promise<CompiledTools> {
  const items: WireTool[] = [];
  const bundles: WireToolBundle[] = [];
  const attached = new Map<string, Tool>();
  const names = new Set<string>();
  let totalBundleBytes = 0;

  for (const tool of selections ?? []) {
    assertTool(tool);
    if (names.has(tool.name)) throw new TypeError(`Brain tool ${tool.name} was selected more than once`);
    names.add(tool.name);
    const definition = await compileDefinition(tool);
    let executor: WireToolExecutor;
    switch (tool.executor.kind) {
      case "hand": {
        const prepared = await prepareHandTool(tool, tool.executor);
        executor = {
          kind: "hand",
          protocol: 1,
          checksum: prepared.checksum,
          source: prepared.source,
          required_env: [...tool.requiredEnv],
        };
        if (prepared.bundle !== undefined) {
          totalBundleBytes += prepared.bundle.bytes.byteLength;
          if (prepared.bundle.bytes.byteLength > MAX_TOOL_BUNDLE_BYTES) {
            throw new TypeError(`Brain tool ${tool.name} bundle exceeds ${MAX_TOOL_BUNDLE_BYTES} bytes`);
          }
          if (totalBundleBytes > MAX_SESSION_BUNDLE_BYTES) {
            throw new TypeError(`Selected tool bundles exceed ${MAX_SESSION_BUNDLE_BYTES} bytes`);
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
      case "attached":
        executor = { kind: "attached", callback_id: tool.executor.callbackId };
        if (attached.has(tool.executor.callbackId)) {
          throw new TypeError(`Attached callback identity ${tool.executor.callbackId} was selected twice`);
        }
        attached.set(tool.executor.callbackId, tool);
        break;
      case "server":
        executor = {
          kind: "server",
          capability: tool.executor.capability,
          completion: tool.executor.completion,
          effect: tool.executor.effect,
          scope: tool.executor.scope,
          max_input_bytes: tool.executor.maxInputBytes,
        };
        break;
      case "intrinsic":
        executor = { kind: "intrinsic", capability: tool.executor.capability };
        break;
      case "mcp":
        executor = {
          kind: "mcp",
          server: tool.executor.server,
          remote_name: tool.executor.remoteName,
        };
        break;
    }
    items.push({ definition, executor });
  }
  return { items, bundles, attached };
}

async function compileDefinition(tool: Tool): Promise<WireToolDefinition> {
  return {
    name: tool.name,
    description: tool.description,
    input_schema: schemaOf(tool.input, `${tool.name} input`),
    output_schema: schemaOf(tool.output, `${tool.name} output`),
  };
}

function schemaOf(schema: z.ZodType, label: string): JsonSchema {
  try {
    const value = z.toJSONSchema(schema, {
      target: "draft-2020-12",
      unrepresentable: "throw",
    }) as JsonSchema;
    if (Object.keys(value).length === 0) throw new TypeError(`${label} schema is empty`);
    return value;
  } catch (cause) {
    throw new TypeError(`${label} cannot be represented as JSON Schema`, { cause });
  }
}

async function prepareHandTool(
  tool: Tool,
  executor: HandExecutor,
): Promise<{ checksum: string; source: "bundle" | "preinstalled"; bundle?: PreparedBundle }> {
  if (executor.preinstalledChecksum !== undefined) {
    return { checksum: executor.preinstalledChecksum, source: "preinstalled" };
  }
  if (executor.prepared !== undefined) {
    assertSha256(executor.prepared.checksum, "prepared bundle checksum");
    const actual = createHash("sha256").update(executor.prepared.bytes).digest("hex");
    if (actual !== executor.prepared.checksum) throw new TypeError(`Brain tool ${tool.name} bundle checksum is invalid`);
    return { checksum: actual, source: "bundle", bundle: executor.prepared };
  }
  if (executor.module === undefined) {
    throw new TypeError(
      `Brain tool ${tool.name} needs module: import.meta.url for Hand execution; choose .local() explicitly for an attached callback`,
    );
  }
  const bundle = await buildToolModule(executor.module);
  return { checksum: bundle.checksum, source: "bundle", bundle };
}

function assertTool(tool: Tool): void {
  if (tool?.kind !== "brain.tool") throw new TypeError("Invalid Brain Tool value");
  assertDefinition(tool.name, tool.description);
}

function assertDefinition(name: string, description: string): void {
  if (!TOOL_NAME.test(name)) throw new TypeError(`Invalid Brain tool name ${JSON.stringify(name)}`);
  if (description.trim() === "" || description.length > 4096) {
    throw new TypeError(`Brain tool ${name} description must contain 1 through 4096 characters`);
  }
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
