import type { z } from "zod";
import type { EventOrigin } from "./generated/session.js";

declare const componentBrand: unique symbol;
declare const agentloopBrand: unique symbol;
declare const environmentBrand: unique symbol;
declare const toolBrand: unique symbol;

export type Schema = z.ZodType;
export type SchemaInput<Value extends Schema> = z.input<Value>;
export type SchemaOutput<Value extends Schema> = z.output<Value>;

/** Prebuilt WebAssembly Component bytes. Brain never compiles application source. */
export interface Component { readonly [componentBrand]: true }
/** One named Environment of a session: the brain env, the host env, or one reached over HTTP. */
export interface Environment { readonly [environmentBrand]: true }
/** An Agentloop placed in the Environment that runs it. */
export interface PlacedAgentloop { readonly [agentloopBrand]: true }
/** A Tool placed in the Environment that runs it. */
export interface PlacedTool<Input = unknown, Output = unknown> {
  readonly [toolBrand]: { readonly input: Input; readonly output: Output };
}
export type SessionTool = PlacedTool;

export interface ToolDefinition {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: Readonly<Record<string, unknown>>;
  readonly outputSchema?: Readonly<Record<string, unknown>>;
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
  | Media
  | { readonly type: "native"; readonly format: string; readonly data: unknown }
  | { readonly type: "tool_use"; readonly id: string; readonly name: string; readonly input: unknown }
  | { readonly type: "tool_result"; readonly tool_use_id: string; readonly content: unknown; readonly is_error: boolean; readonly media?: readonly Media[] };
export interface ModelMessage {
  readonly role: "user" | "assistant" | "developer";
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

export type Media = { readonly type: "image"; readonly url: string };
export interface UserInput { readonly message: string; readonly media?: readonly Media[] }

export interface CreateSessionOptions {
  readonly model: ModelSelection;
  readonly agentloop: PlacedAgentloop;
  readonly tools?: readonly SessionTool[];
  readonly system?: string;
  readonly responseFormat?: unknown;
  readonly transcript?: readonly ModelMessage[];
  readonly idleTtlMs?: number;
}

export interface OperationOptions { readonly idempotencyKey?: string }
export interface SendOptions extends OperationOptions {
  /** Cancels an exclusively owned session turn when its owner is interrupted.
   * Use a fresh handle and do not send concurrently through another owner. */
  readonly signal?: AbortSignal;
}
export interface StructuredSendOptions<S extends Schema> extends SendOptions {
  readonly output: {
    readonly type: S;
    /** Additional correction turns after the first answer. Default 2; zero disables retries. */
    readonly maxRetries?: number;
  };
}
export interface SessionState {
  readonly id: string;
  readonly status: "creating" | "idle" | "running" | "ending" | "ended" | "failed";
  readonly lastSequence: number;
}

/** One journal record. `(session id, sequence)` names it. */
export interface SessionEvent<Data = unknown> {
  readonly origin?: EventOrigin;
  readonly sequence: number;
  readonly recordedAt: Date;
  readonly type: string;
  readonly data: Data;
}

export interface SessionStreamEvent<Data = unknown> {
  readonly origin?: EventOrigin;
  readonly sequence?: number;
  readonly type: string;
  readonly data: Data;
}

export interface AgentloopAdmission {
  readonly id: string;
  readonly status: "admitted" | "rejected";
  readonly error?: { readonly code: string; readonly message: string; readonly retryable: boolean; readonly details?: unknown };
}
