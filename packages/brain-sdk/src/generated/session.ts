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
 * via the `definition` "Runtime".
 */
export type Runtime = "esm" | "shell" | "http";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ResourceName".
 */
export type ResourceName = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Program".
 */
export type Program =
  | {
      kind: "esm";
      identity: Identity;
    }
  | {
      kind: "shell";
      identity: Identity;
      script: string;
    }
  | {
      kind: "http";
      identity: Identity;
      request: HttpProgramRequest;
    };
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
  /**
   * @maxItems 64
   */
  needs: ResourceName[];
  /**
   * @maxItems 64
   */
  binding_names: Identifier[];
  hosting?: "provisioned" | "client";
  program?: Program;
  environment_id?: Identifier;
};
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentStatus".
 */
export type EnvironmentStatus = "open" | "unreachable";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Sequence".
 */
export type Sequence = number;
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
 * via the `definition` "HttpProgramRequest".
 */
export interface HttpProgramRequest {
  method: string;
  url: string;
  headers?: {
    [k: string]: string | undefined;
  };
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentAttachRequest".
 */
export interface EnvironmentAttachRequest {
  environment_id: Identifier;
  bindings?: {
    [k: string]: string | undefined;
  };
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "CreateEnvironmentRequest".
 */
export interface CreateEnvironmentRequest {
  environment_id?: Identifier;
  configuration: unknown;
  /**
   * Brain closes the environment once no session has been attached to it for idle_ttl_ms.
   */
  managed?: boolean;
  idle_ttl_ms?: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Environment".
 */
export interface Environment {
  environment_id: Identifier;
  status: EnvironmentStatus;
  managed: boolean;
  idle_ttl_ms?: number;
  attached_sessions: SessionId[];
  runtimes?: Runtime[];
  resources?: {};
  created_at_ms: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentList".
 */
export interface EnvironmentList {
  environments: Environment[];
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
  response_format?: unknown;
  /**
   * @maxItems 128
   */
  tools: BoundTool[];
  /**
   * @maxItems 128
   */
  environments: EnvironmentAttachRequest[];
  /**
   * @maxItems 10000
   */
  history?: HistoryEvent[];
  /**
   * How long the session may sit idle before Brain suspends its task and memory to disk. Absent means the server default; zero means never.
   */
  idle_ttl_ms?: number;
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
  status: "creating" | "idle" | "running" | "ended" | "failed";
  last_sequence: number;
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
