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

export interface CreateSessionOptions {
  readonly model: VercelAiGatewayModel;
  readonly brain: BrainExtension;
  readonly tools?: readonly BoundTool[];
  readonly system?: string;
  readonly responseFormat?: unknown;
}

export interface OperationOptions { readonly idempotencyKey?: string }

export interface SessionState {
  readonly id: string;
  readonly journalId: string;
  readonly status: "creating" | "idle" | "running" | "ended" | "failed";
  readonly throughSequence: number;
  readonly presentationDigest: string;
}

export interface SessionEvent<Data = unknown> {
  readonly id: string;
  readonly sequence: number;
  readonly recordedAt: Date;
  readonly type: string;
  readonly data: Data;
}

export interface AgentloopAdmission {
  readonly digest: string;
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
  agentloop_digest: string;
  brain_configuration: unknown;
  model: { provider: "vercel-ai-gateway"; name: string; api_key: string };
  presentation: { system: string; tools: WireToolDefinition[]; response_format?: unknown };
  environments: WireEnvironmentRequirement[];
  tool_bindings: WireToolBinding[];
}
export interface WireSession {
  session_id: string;
  journal_id: string;
  status: SessionState["status"];
  through_sequence: number;
  presentation_digest: string;
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
