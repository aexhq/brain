/* eslint-disable */
/** Generated from Brain-owned v1 contracts. Do not edit. */

/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AdmissionStatus".
 */
export type AdmissionStatus = "admitted" | "rejected";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopIdentity".
 */
export type AgentloopIdentity = string;
/**
 * One tool as the SDK hands it over: its manifest fields plus the environment it
 * binds to. Brain splits the model-facing and dispatch-facing halves internally.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "BoundTool".
 */
export type BoundTool = {
  [k: string]: unknown | undefined;
} & {
  /**
   * @maxItems 64
   */
  binding_names: string[];
  description: string;
  /**
   * Required for a provisioned tool; a resident tool binds no Environment.
   */
  environment_id?: string;
  /**
   * Required for a resident tool and absent for a provisioned tool.
   */
  host_id?: string;
  /**
   * Where a tool's implementation executes: a placed implementation in an Environment,
   * or a function held by a registered application host.
   */
  hosting?: "provisioned" | "resident";
  implementation?: unknown;
  input_schema: {};
  name: string;
  /**
   * @maxItems 64
   */
  needs?: string[];
  output_schema?: {};
};
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ContentBlock".
 */
export type ContentBlock =
  | {
      text: string;
      type: "text";
    }
  | {
      id: string;
      input: unknown;
      name: string;
      type: "tool_use";
    }
  | {
      content: unknown;
      /**
       * ALWAYS set on a failed tool. Omitting the flag on a failure lets the
       * model read that failure as a success.
       */
      is_error: boolean;
      tool_use_id: string;
      type: "tool_result";
    };
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentId".
 */
export type EnvironmentId = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Role".
 */
export type Role = "user" | "assistant";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EventId".
 */
export type EventId = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HostOperation".
 */
export type HostOperation =
  | {
      invocation: ToolInvocation;
      type: "invoke_tool";
    }
  | {
      target_sequence: number;
      type: "cancel_tool";
    };
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "SessionId".
 */
export type SessionId = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HostId".
 */
export type HostId = string;
/**
 * The one envelope every tool invocation resolves to.
 *
 * `timeout` is distinguished from `error` because the deadline is caller-owned: no
 * backend family can be trusted to enforce one remotely, so the caller kills and says
 * exactly what happened rather than encoding it as an exit code.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Outcome".
 */
export type Outcome =
  | {
      status: "ok";
      value: unknown;
    }
  | {
      error: OutcomeError;
      status: "error";
    }
  | {
      status: "timeout";
    }
  | {
      status: "cancelled";
    }
  | {
      message: string;
      status: "unknown";
    };
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Identity".
 */
export type Identity = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "SessionStatus".
 */
export type SessionStatus = "creating" | "idle" | "running" | "ending" | "ended" | "failed";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolIdentity".
 */
export type ToolIdentity = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolAdmissionStatus".
 */
export type ToolAdmissionStatus = "admitted" | "rejected";
/**
 * Where a tool's implementation executes: a placed implementation in an Environment,
 * or a function held by a registered application host.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolHosting".
 */
export type ToolHosting = "provisioned" | "resident";

export interface BrainSessionAPIV1 {
  contract: "session/v1";
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopAdmission".
 */
export interface AgentloopAdmission {
  error?: ApiError;
  identity: AgentloopIdentity;
  status: AdmissionStatus;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ApiError".
 */
export interface ApiError {
  code: string;
  details?: unknown;
  message: string;
  retryable: boolean;
}
/**
 * The admitted loop package a session runs: which one, and how it is configured.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopRef".
 */
export interface AgentloopRef {
  configuration: unknown;
  /**
   * The Environment that executes this Agentloop. The MVP supports Brain's native
   * Wasmtime Environment; the binding stays explicit for later drivers.
   */
  environment_id: string;
  identity: AgentloopIdentity;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "BindingValues".
 */
export interface BindingValues {
  [k: string]: string | undefined;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "CreateSessionRequest".
 */
export interface CreateSessionRequest {
  agentloop: AgentloopRef;
  /**
   * Immutable Environment specifications opened and attached as part of this create.
   *
   * @maxItems 128
   */
  environments: SessionEnvironment[];
  /**
   * How long the session may sit idle before Brain suspends it: its task and memory
   * are released and rebuilt from disk on the next request. Absent means the server's
   * default; zero means never.
   */
  idle_ttl_ms?: number;
  model: ModelSelection;
  /**
   * The provider's structured-output request, applied to every model call unless the
   * loop sends its own. Optional, and rejected at create for a provider that cannot
   * carry it.
   */
  response_format?: {
    [k: string]: unknown | undefined;
  };
  /**
   * The system prompt the agent loop starts from. The loop may send a different one
   * on any model call.
   */
  system?: string;
  /**
   * @maxItems 128
   */
  tools: BoundTool[];
  /**
   * A transcript to carry forward, if the caller has one: the messages the new
   * session's first model call should already see. Brain journals them as the session's
   * opening transcript. Empty is an ordinary new session.
   *
   * @maxItems 4096
   */
  transcript?: Message[];
}
/**
 * One immutable Environment specification in a session create. `bindings` carries
 * plaintext values only until attach and is never copied into the session journal.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "SessionEnvironment".
 */
export interface SessionEnvironment {
  bindings?: BindingValues1;
  configuration: unknown;
  environment_id: EnvironmentId;
  idle_ttl_ms?: number;
  managed?: boolean;
}
export interface BindingValues1 {
  [k: string]: string | undefined;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ModelSelection".
 */
export interface ModelSelection {
  api_key: string;
  name: string;
  provider: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Message".
 */
export interface Message {
  content: ContentBlock[];
  role: Role;
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
 * via the `definition` "Event".
 */
export interface Event {
  data: unknown;
  event_id: EventId;
  event_type: string;
  recorded_at_ms: number;
  sequence: number;
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
 * via the `definition` "HostCommand".
 */
export interface HostCommand {
  deadline_at_ms: number;
  operation: HostOperation;
  sequence: number;
  session_id: SessionId;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolInvocation".
 */
export interface ToolInvocation {
  call_id: string;
  input: unknown;
  name: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HostEvent".
 */
export interface HostEvent {
  data: unknown;
  event_type: string;
  /**
   * The resident command this Event belongs to.
   */
  sequence: number;
  session_id: SessionId;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HostEventAck".
 */
export interface HostEventAck {
  /**
   * The sequence Brain assigned to the committed Event.
   */
  sequence: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HostRegistration".
 */
export interface HostRegistration {
  host_id: HostId;
  token: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HostResult".
 */
export interface HostResult {
  outcome: Outcome;
  sequence: number;
  session_id: SessionId;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "OutcomeError".
 */
export interface OutcomeError {
  code: string;
  details?: unknown;
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
 * What an application hands a session on `send`. The shape is closed on purpose:
 * Brain owes every agentloop the same observation shape regardless of who wrote
 * the client, so free-form content is not accepted. Multimodal parts will extend
 * this record when they land — see the roadmap.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "UserInput".
 */
export interface UserInput {
  message: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "SessionList".
 */
export interface SessionList {
  sessions: SessionSummary[];
}
/**
 * What the API says about a session: its id, where it is, and how far its journal goes.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "SessionSummary".
 */
export interface SessionSummary {
  /**
   * Sequence of the last journal record committed for this session — the journal is
   * complete through here, so it is where a `GET /events` cursor starts.
   */
  last_sequence: number;
  session_id: SessionId;
  status: SessionStatus;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolAdmission".
 */
export interface ToolAdmission {
  error?: TurnError;
  identity: ToolIdentity;
  status: ToolAdmissionStatus;
}
/**
 * Why a turn, or one of the host calls inside it, failed.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "TurnError".
 */
export interface TurnError {
  code: string;
  message: string;
  retryable?: boolean;
}
