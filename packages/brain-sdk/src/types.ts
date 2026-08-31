import type { z } from "zod";

declare const agentloopBrand: unique symbol;
declare const environmentBrand: unique symbol;
declare const toolBrand: unique symbol;
declare const boundToolBrand: unique symbol;

export type Schema = z.ZodType;
export type SchemaInput<Value extends Schema> = z.input<Value>;
export type SchemaOutput<Value extends Schema> = z.output<Value>;

export interface Agentloop { readonly [agentloopBrand]: true }
export interface Environment { readonly [environmentBrand]: true }
export interface Tool<Input = unknown, Output = unknown> {
  readonly [toolBrand]: true;
  useIn(environment: Environment): BoundTool<Input, Output>;
}
export interface BoundTool<Input = unknown, Output = unknown> {
  readonly [boundToolBrand]: { readonly input: Input; readonly output: Output };
}

export interface ToolDefinition {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: Readonly<Record<string, unknown>>;
  readonly outputSchema?: Readonly<Record<string, unknown>>;
}

export type { KnownProviderId } from "./generated/providers.js";
import type { KnownProviderId } from "./generated/providers.js";

export interface VercelAiGatewayModel {
  readonly provider: "vercel-ai-gateway";
  readonly name: `${string}/${string}`;
  readonly apiKey: string;
}
/** A provider from the generated models.dev catalog. */
export interface KnownProviderModel {
  readonly provider: Exclude<KnownProviderId, "vercel-ai-gateway">;
  readonly name: string;
  readonly apiKey: string;
}
/** Any other provider the server's deployment registers (a custom providers
 * file, a proxy). `string & {}` keeps autocomplete for the known ids while
 * still accepting an arbitrary identifier; unknown providers are rejected by
 * the server, not the client. */
export interface CustomProviderModel {
  readonly provider: string & {};
  readonly name: string;
  readonly apiKey: string;
}
export type ModelSelection = VercelAiGatewayModel | KnownProviderModel | CustomProviderModel;

/** The provider-neutral message model. History is authored once, in this
 * shape; Brain renders it per provider dialect at request build time. */
export type ModelContentBlock =
  | { readonly type: "text"; readonly text: string }
  | { readonly type: "tool_use"; readonly id: string; readonly name: string; readonly input: unknown }
  | { readonly type: "tool_result"; readonly tool_use_id: string; readonly content: unknown; readonly is_error: boolean };
export interface ModelMessage {
  readonly role: "user" | "assistant";
  readonly content: readonly ModelContentBlock[];
}
export type ModelStopReason = "end_turn" | "tool_use" | "max_tokens" | "stop_sequence" | "refusal" | "unknown";
/** Every field is optional because absent is never zero: a provider that did
 * not report a count did not report zero. */
export interface ModelUsage {
  readonly input_tokens?: number;
  readonly output_tokens?: number;
  readonly cache_read_input_tokens?: number;
  readonly cache_creation_input_tokens?: number;
  readonly reasoning_tokens?: number;
}
export interface ModelResponse {
  readonly message: ModelMessage;
  readonly stop_reason: ModelStopReason;
  readonly usage: ModelUsage;
}

export interface CreateSessionOptions {
  readonly model: ModelSelection;
  readonly agentloop: Agentloop;
  readonly tools?: readonly BoundTool[];
  readonly system?: string;
  readonly responseFormat?: unknown;
  /**
   * Events from an earlier session, to carry a conversation forward.
   *
   * A session lives in the process that created it and does not survive a restart, so
   * keep the events you receive from `handle.events()` and pass them back here to
   * continue. Brain writes them as the new session's opening records and tells the
   * agentloop about them. Omit for an ordinary new session.
   */
  readonly history?: readonly SessionEvent[];
}

export interface OperationOptions { readonly idempotencyKey?: string }

export interface SessionState {
  readonly id: string;
  readonly journalId: string;
  readonly status: "creating" | "idle" | "running" | "ended" | "failed";
  /** Sequence of the last journal record committed for this session — where an
   * events cursor starts. */
  readonly lastSequence: number;
  /** Hash of everything the session was sealed with: agentloop configuration, system
   * prompt, tool definitions, and response format. Stable for the session's life. */
  readonly configHash: string;
}

export interface SessionEvent<Data = unknown> {
  readonly id: string;
  readonly sequence: number;
  readonly recordedAt: Date;
  readonly type: string;
  readonly data: Data;
}

export interface AgentloopAdmission {
  readonly identity: string;
  readonly status: "admitted" | "rejected";
  readonly error?: { readonly code: string; readonly message: string; readonly retryable: boolean; readonly details?: unknown };
}
