/* eslint-disable */
/** Generated from Brain-owned v1 contracts. Do not edit. */

/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Identifier".
 */
export type Identifier = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Digest".
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopDigest".
 */
export type Digest = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "SessionId".
 */
export type SessionId = string;

export interface BrainSessionAPIV1 {
  contract: "session/v1";
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolDefinition".
 */
export interface ToolDefinition {
  name: Identifier;
  description: string;
  input_schema: {};
  output_schema?: {};
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "RequestedToolBinding".
 */
export interface RequestedToolBinding {
  name: Identifier;
  environment_id: Identifier;
  remote_tool_id: Identifier;
  grant: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentRequirement".
 */
export interface EnvironmentRequirement {
  environment_id: Identifier;
  configuration: unknown;
  lifecycle_policy: "session" | "shared" | "external";
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ModelSelection".
 */
export interface ModelSelection {
  provider: "vercel-ai-gateway";
  name: string;
  api_key: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ModelPresentation".
 */
export interface ModelPresentation {
  system: string;
  /**
   * @maxItems 128
   */
  tools: ToolDefinition[];
  response_format?: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "CreateSessionRequest".
 */
export interface CreateSessionRequest {
  agentloop_digest: Digest;
  model: ModelSelection;
  presentation: ModelPresentation;
  /**
   * @maxItems 128
   */
  environments: EnvironmentRequirement[];
  /**
   * @maxItems 128
   */
  tool_bindings: RequestedToolBinding[];
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "MessageRequest".
 */
export interface MessageRequest {
  content: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Session".
 */
export interface Session {
  session_id: SessionId;
  journal_id: Identifier;
  status: "creating" | "idle" | "running" | "ended" | "failed";
  through_sequence: number;
  presentation_digest: Digest;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "SessionList".
 */
export interface SessionList {
  sessions: Session[];
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Event".
 */
export interface Event {
  event_id: Identifier;
  sequence: number;
  recorded_at_ms: number;
  event_type: Identifier;
  data: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EventPage".
 */
export interface EventPage {
  /**
   * @maxItems 1000
   */
  events: Event[];
  next_cursor: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopManifest".
 */
export interface AgentloopManifest {
  contract_version: "agentloop/v1";
  component_digest: Digest;
  component_bytes: number;
  toolchain?: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopAdmission".
 */
export interface AgentloopAdmission {
  digest: Digest;
  status: "admitted" | "rejected";
  error?: ApiError;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ApiError".
 */
export interface ApiError {
  code: Identifier;
  message: string;
  retryable: boolean;
  details?: unknown;
}
