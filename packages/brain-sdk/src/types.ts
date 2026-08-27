export interface ToolDefinition {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: Readonly<Record<string, unknown>>;
  readonly outputSchema?: Readonly<Record<string, unknown>>;
}

export type EnvironmentLifecycle =
  | { readonly type?: "session"; readonly id?: never }
  | { readonly type: "shared" | "external"; readonly id: string };

export interface AgentLoop {
  readonly kind: "agent-loop";
  readonly package: URL | Uint8Array;
}

export interface Environment<Capability extends string = string> {
  readonly kind: "environment";
  readonly capability: Capability;
  readonly configuration: unknown;
  readonly lifecycle: EnvironmentLifecycle;
}

export interface ToolBindingOptions {
  readonly grant?: unknown;
}

export interface Tool<Input = unknown, Output = unknown, CompatibleEnvironment extends Environment = Environment> {
  readonly kind: "tool";
  readonly environmentCapability: CompatibleEnvironment["capability"];
  readonly definition: ToolDefinition;
  readonly remoteToolId: string;
  readonly defaultGrant: unknown;
  runIn(environment: CompatibleEnvironment, options?: ToolBindingOptions): BoundTool<Input, Output>;
}

export interface BoundTool<Input = unknown, Output = unknown> {
  readonly kind: "bound-tool";
  readonly tool: Tool<Input, Output>;
  readonly environment: Environment;
  readonly grant: unknown;
}

export interface VercelAiGatewayModel {
  readonly provider: "vercel-ai-gateway";
  readonly name: `${string}/${string}`;
  readonly apiKey: string;
}

export interface CreateSessionOptions {
  readonly model: VercelAiGatewayModel;
  readonly agentLoop: AgentLoop;
  readonly tools?: readonly BoundTool[];
  readonly system?: string;
  readonly responseFormat?: unknown;
}

export interface OperationOptions {
  readonly idempotencyKey?: string;
}

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
  grant: unknown;
}

export interface WireEnvironmentRequirement {
  environment_id: string;
  configuration: unknown;
  lifecycle_policy: "session" | "shared" | "external";
}

export interface WireCreateSessionRequest {
  agentloop_digest: string;
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

export interface WireEventPage {
  events: WireEvent[];
  next_cursor: number;
}

export interface WireSessionList {
  sessions: WireSession[];
}
