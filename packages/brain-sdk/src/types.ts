export interface ToolDefinition {
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
  output_schema?: Record<string, unknown>;
}

export interface ToolBinding {
  name: string;
  environment_id: string;
  remote_tool_id: string;
  grant: unknown;
}

export interface EnvironmentRequirement {
  environment_id: string;
  configuration: unknown;
  lifecycle_policy: "session" | "shared" | "external";
}

export interface CreateSessionRequest {
  agentloop_digest: string;
  model: { binding_id: string; model: string };
  presentation: { system: string; tools: ToolDefinition[]; response_format?: unknown };
  environments: EnvironmentRequirement[];
  tool_bindings: ToolBinding[];
  metadata?: unknown;
}

export interface Session {
  session_id: string;
  journal_id: string;
  status: "creating" | "idle" | "running" | "ended" | "failed";
  through_sequence: number;
  presentation_digest: string;
  metadata: unknown;
}

export interface SessionEvent {
  event_id: string;
  sequence: number;
  recorded_at_ms: number;
  event_type: string;
  data: unknown;
}

export interface EventPage {
  events: SessionEvent[];
  next_cursor: number;
}

export interface AgentloopAdmission {
  digest: string;
  status: "admitted" | "rejected";
  error?: { code: string; message: string; retryable: boolean; details?: unknown };
}

export interface SessionList {
  sessions: Session[];
}
