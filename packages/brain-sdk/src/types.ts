import type { z } from "zod";

declare const brainExtensionBrand: unique symbol;
declare const environmentBrand: unique symbol;
declare const toolBrand: unique symbol;
declare const boundToolBrand: unique symbol;

export type Schema = z.ZodType;
export type SchemaInput<Value extends Schema> = z.input<Value>;
export type SchemaOutput<Value extends Schema> = z.output<Value>;

export interface BrainExtension { readonly [brainExtensionBrand]: true }
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

export interface VercelAiGatewayModel {
  readonly provider: "vercel-ai-gateway";
  readonly name: `${string}/${string}`;
  readonly apiKey: string;
}
export interface OpenAiModel {
  readonly provider: "openai";
  readonly name: string;
  readonly apiKey: string;
}
export interface AnthropicModel {
  readonly provider: "anthropic";
  readonly name: string;
  readonly apiKey: string;
}
export type ModelSelection = VercelAiGatewayModel | OpenAiModel | AnthropicModel;

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
  readonly brain: BrainExtension;
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
  readonly throughSequence: number;
  readonly presentationIdentity: string;
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

export interface WireToolDefinition {
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
  output_schema?: Record<string, unknown>;
}
export interface WireToolBinding {
  name: string;
  environment_id: string;
  remote_tool_id: string;
  tool_configuration: unknown;
  grant: unknown;
}
export interface WireEnvironmentRequirement {
  environment_id: string;
  configuration: unknown;
  lifecycle_policy: "session";
}
export interface WireCreateSessionRequest {
  agentloop_identity: string;
  brain_configuration: unknown;
  model: { provider: ModelSelection["provider"]; name: string; api_key: string };
  presentation: { system: string; tools: WireToolDefinition[]; response_format?: unknown };
  environments: WireEnvironmentRequirement[];
  tool_bindings: WireToolBinding[];
  history?: WireHistoryEvent[];
}
export interface WireHistoryEvent {
  sequence: number;
  recorded_at_ms?: number;
  event_type: string;
  data: unknown;
}
export interface WireSession {
  session_id: string;
  journal_id: string;
  status: SessionState["status"];
  through_sequence: number;
  presentation_identity: string;
}
export interface WireEvent {
  event_id: string;
  sequence: number;
  recorded_at_ms: number;
  event_type: string;
  data: unknown;
}
export interface WireEventPage { events: WireEvent[]; next_cursor: number }
export interface WireSessionList { sessions: WireSession[] }
