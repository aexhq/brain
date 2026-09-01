/* eslint-disable */
/** Generated from Brain-owned v1 contracts. Do not edit. */

/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Identifier".
 */
export type Identifier = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Identity".
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopIdentity".
 */
export type Identity = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "SessionId".
 */
export type SessionId = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "CapabilityName".
 */
export type CapabilityName = "exec" | "fs" | "net" | "js" | "page";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "BoundTool".
 */
export type BoundTool = {
  [k: string]: unknown | undefined;
} & {
  name: Identifier;
  description: string;
  input_schema: {};
  output_schema?: {};
  requires: CapabilityName[];
  /**
   * @maxItems 64
   */
  binding_names: Identifier[];
  hosting?: "provisioned" | "client";
  payload?: ToolPayload;
  environment_id?: Identifier;
};
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "OperationId".
 */
export type OperationId = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Outcome".
 */
export type Outcome =
  | {
      status: "ok";
      value: unknown;
    }
  | {
      status: "error";
      error: OutcomeError;
    }
  | {
      status: "timeout";
    }
  | {
      status: "cancelled";
    };
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ShareKey".
 */
export type ShareKey = string;

export interface BrainSessionAPIV1 {
  contract: "session/v1";
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopRef".
 */
export interface AgentloopRef {
  identity: Identity;
  configuration: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolPayload".
 */
export interface ToolPayload {
  kind: "esm" | "component";
  identity: Identity;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ExecGrant".
 */
export interface ExecGrant {
  timeout_ms_max?: number;
  output_bytes_max?: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "FsGrant".
 */
export interface FsGrant {
  root: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "NetGrant".
 */
export interface NetGrant {
  /**
   * @maxItems 256
   */
  allow: string[];
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "JsGrant".
 */
export interface JsGrant {}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "PageGrant".
 */
export interface PageGrant {}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "GrantSet".
 */
export interface GrantSet {
  exec?: ExecGrant;
  fs?: FsGrant;
  net?: NetGrant;
  js?: JsGrant;
  page?: PageGrant;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentRequirement".
 */
export interface EnvironmentRequirement {
  environment_id: Identifier;
  configuration: unknown;
  lifecycle_policy: "session" | "shared" | "external";
  grants?: GrantSet;
  bindings?: {
    [k: string]: string | undefined;
  };
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ModelSelection".
 */
export interface ModelSelection {
  provider: Identifier;
  name: string;
  api_key: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "CreateSessionRequest".
 */
export interface CreateSessionRequest {
  agentloop: AgentloopRef;
  model: ModelSelection;
  system?: string;
  /**
   * @maxItems 128
   */
  tools: BoundTool[];
  response_format?: unknown;
  /**
   * @maxItems 128
   */
  environments: EnvironmentRequirement[];
  /**
   * @maxItems 10000
   */
  history?: HistoryEvent[];
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HistoryEvent".
 */
export interface HistoryEvent {
  sequence: number;
  recorded_at_ms?: number;
  event_type: Identifier;
  data: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "UserInput".
 */
export interface UserInput {
  message: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "MessageRequest".
 */
export interface MessageRequest {
  input: UserInput;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentCallRequest".
 */
export interface EnvironmentCallRequest {
  input: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentCallResult".
 */
export interface EnvironmentCallResult {
  output: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "OutcomeError".
 */
export interface OutcomeError {
  code: Identifier;
  message: string;
  details?: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Session".
 */
export interface Session {
  session_id: SessionId;
  journal_id: Identifier;
  status: "creating" | "idle" | "running" | "ended" | "failed";
  last_sequence: number;
  config_hash: Identity;
  share_key: ShareKey;
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
  component_identity: Identity;
  component_bytes: number;
  toolchain?: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopAdmission".
 */
export interface AgentloopAdmission {
  identity: Identity;
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
