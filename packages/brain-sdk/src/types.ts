import type { z } from "zod";

declare const componentBrand: unique symbol;
declare const agentloopBrand: unique symbol;
declare const environmentBrand: unique symbol;
declare const toolBrand: unique symbol;

export type Schema = z.ZodType;
export type SchemaInput<Value extends Schema> = z.input<Value>;
export type SchemaOutput<Value extends Schema> = z.output<Value>;

/** Prebuilt WebAssembly Component bytes. Brain never compiles application source. */
export interface Component { readonly [componentBrand]: true }
/** An opaque immutable placement specification. */
export interface Environment { readonly [environmentBrand]: true }
/** An Agentloop implementation bound to its execution Environment. */
export interface AgentloopBinding { readonly [agentloopBrand]: true }
/** A Tool implementation bound either to this application host or an Environment. */
export interface ToolBinding<Input = unknown, Output = unknown> {
  readonly [toolBrand]: { readonly input: Input; readonly output: Output };
}
export type SessionTool = ToolBinding;

export interface ToolDefinition {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: Readonly<Record<string, unknown>>;
  readonly outputSchema?: Readonly<Record<string, unknown>>;
}

export type ResourceName = string;
export interface FsResource { readonly root: string }
export interface ProcessResource { readonly timeout_ms_max?: number; readonly output_bytes_max?: number }
export interface NetResource { readonly allow: readonly string[] }
export type DomResource = Record<never, never>;
export interface SecretsResource { readonly names: readonly string[] }
export interface Resources {
  readonly fs?: FsResource;
  readonly process?: ProcessResource;
  readonly net?: NetResource;
  readonly dom?: DomResource;
  readonly secrets?: SecretsResource;
  readonly [vendor: `${string}:${string}`]: Readonly<Record<string, unknown>> | undefined;
}

export type Outcome<Value = unknown> =
  | { readonly status: "ok"; readonly value: Value }
  | { readonly status: "error"; readonly error: { readonly code: string; readonly message: string; readonly details?: unknown } }
  | { readonly status: "timeout" }
  | { readonly status: "cancelled" }
  | { readonly status: "unknown"; readonly message: string };

export type { KnownProviderId } from "./generated/providers.js";
import type { KnownProviderId } from "./generated/providers.js";

export interface VercelAiGatewayModel {
  readonly provider: "vercel-ai-gateway";
  readonly name: `${string}/${string}`;
  readonly apiKey: string;
}
export interface KnownProviderModel {
  readonly provider: Exclude<KnownProviderId, "vercel-ai-gateway">;
  readonly name: string;
  readonly apiKey: string;
}
export interface CustomProviderModel {
  readonly provider: string & {};
  readonly name: string;
  readonly apiKey: string;
}
export type ModelSelection = VercelAiGatewayModel | KnownProviderModel | CustomProviderModel;

export type ModelContentBlock =
  | { readonly type: "text"; readonly text: string }
  | { readonly type: "tool_use"; readonly id: string; readonly name: string; readonly input: unknown }
  | { readonly type: "tool_result"; readonly tool_use_id: string; readonly content: unknown; readonly is_error: boolean };
export interface ModelMessage {
  readonly role: "user" | "assistant";
  readonly content: readonly ModelContentBlock[];
}
export type ModelStopReason = "end_turn" | "tool_use" | "max_tokens" | "stop_sequence" | "refusal" | "unknown";
export interface ModelUsage {
  readonly input_tokens?: number;
  readonly output_tokens?: number;
  readonly cache_read_input_tokens?: number;
  readonly cache_creation_input_tokens?: number;
  readonly reasoning_tokens?: number;
  readonly provider_cost_usd?: string;
}
export interface ModelResponse {
  readonly message: ModelMessage;
  readonly stop_reason: ModelStopReason;
  readonly usage: ModelUsage;
}

export interface UserInput { readonly message: string }

export interface CreateSessionOptions {
  readonly model: ModelSelection;
  readonly agentloop: AgentloopBinding;
  readonly tools?: readonly SessionTool[];
  readonly system?: string;
  readonly responseFormat?: unknown;
  readonly transcript?: readonly ModelMessage[];
  readonly idleTtlMs?: number;
}

export interface OperationOptions { readonly idempotencyKey?: string }
export interface SessionState {
  readonly id: string;
  readonly status: "creating" | "idle" | "running" | "ended" | "failed";
  readonly lastSequence: number;
}

export interface SessionEvent<Data = unknown> {
  readonly id: string;
  readonly sequence: number;
  readonly recordedAt: Date;
  readonly type: string;
  readonly data: Data;
}

export interface SessionStreamEvent<Data = unknown> {
  readonly sequence?: number;
  readonly type: string;
  readonly data: Data;
}

export interface AgentloopAdmission {
  readonly identity: string;
  readonly status: "admitted" | "rejected";
  readonly error?: { readonly code: string; readonly message: string; readonly retryable: boolean; readonly details?: unknown };
}
