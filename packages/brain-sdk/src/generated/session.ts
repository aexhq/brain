/* eslint-disable */
/** Generated from Brain-owned v1 contracts. Do not edit. */

/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AdmissionStatus".
 */
export type AdmissionStatus = "admitted" | "rejected";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopId".
 */
export type AgentloopId = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentName".
 */
export type EnvironmentName = string;
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
      type: "image";
      /**
       * HTTPS URL or an image data URL, rendered by the fixed model adapter.
       */
      url: string;
    }
  | {
      data: unknown;
      /**
       * Versioned adapter format; incompatible adapters must reject the item.
       */
      format: string;
      type: "native";
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
      /**
       * Model-visible media alongside the ordinary JSON Tool output.
       */
      media?: Media[];
      tool_use_id: string;
      type: "tool_result";
    };
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Media".
 */
export type Media = {
  type: "image";
  url: string;
};
/**
 * One Environment a session declares: a name unique within the session, how Brain
 * reaches it, and its own configuration, which Brain carries and never reads.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Environment".
 */
export type Environment = {
  configuration?: {
    [k: string]: unknown | undefined;
  };
  name: EnvironmentName;
} & Environment1;
export type Environment1 =
  | {
      driver: "brain";
    }
  | {
      driver: "host";
      host_id: HostId;
    }
  | {
      credential?: string;
      driver: "http";
      url: string;
    };
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HostId".
 */
export type HostId = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Role".
 */
export type Role = "user" | "assistant" | "developer";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EventOrigin".
 */
export type EventOrigin =
  | {
      kind: "agentloop";
      sequence: number;
    }
  | {
      kind: "tool";
      sequence: number;
    };
/**
 * What a host is asked to do for a session placed in it. A call is named by the
 * command's `(session_id, sequence)`; the Tool's own call id never leaves Brain.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HostOperation".
 */
export type HostOperation =
  | {
      input: unknown;
      name: string;
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
 * via the `definition` "StopReason".
 */
export type StopReason =
  ("end_turn" | "tool_use" | "max_tokens" | "stop_sequence" | "refusal") | "unknown";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "SessionStatus".
 */
export type SessionStatus = "creating" | "idle" | "running" | "ending" | "ended" | "failed";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolId".
 */
export type ToolId = string;
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolAdmissionStatus".
 */
export type ToolAdmissionStatus = "admitted" | "rejected";

export interface BrainSessionAPIV1 {
  contract: "session/v1";
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopAdmission".
 */
export interface AgentloopAdmission {
  error?: ApiError;
  id: AgentloopId;
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
 * The admitted Agentloop a session runs: which one, how it is configured, which
 * Environment of the session runs it, and what it needs there.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopRef".
 */
export interface AgentloopRef {
  configuration: unknown;
  environment: EnvironmentName;
  implementation: unknown;
  /**
   * What the Agentloop needs from its Environment, as URIs. Brain hands them to the
   * Environment at setup and with every turn, and reads none of them.
   *
   * @maxItems 64
   */
  needs?: string[];
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "CreateSessionRequest".
 */
export interface CreateSessionRequest {
  agentloop: AgentloopRef;
  /**
   * The Environments of this session, set up as part of this create. Every Tool and
   * the Agentloop name one of them.
   */
  environments: Environment[];
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
  tools: Tool[];
  /**
   * A transcript to carry forward, if the caller has one: the messages the new
   * session's first model call should already see. Brain journals them as the session's
   * opening transcript. Empty is an ordinary new session.
   */
  transcript?: Message[];
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
 * A canonical Tool definition with one implementation per authorized Environment.
 * Brain validates dispatched inputs and successful outputs against this definition.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Tool".
 */
export interface Tool {
  description: string;
  input_schema: {};
  name: string;
  output_schema?: {};
  /**
   * One implementation per authorized Environment, fixed at create.
   */
  placements: {
    [k: string]: ToolPlacement;
  };
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema definition
 * via the `patternProperty` "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$".
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolPlacement".
 */
export interface ToolPlacement {
  implementation: unknown;
  /**
   * @maxItems 64
   */
  needs?: string[];
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
 * One journal record as a client reads it. `(session_id, sequence)` names it; there
 * is no other identifier.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Event".
 */
export interface Event {
  data: unknown;
  event_type: string;
  /**
   * Absent on kernel records and historical extension records without attribution.
   */
  origin?:
    | {
        kind: "agentloop";
        sequence: number;
      }
    | {
        kind: "tool";
        sequence: number;
      };
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
 * via the `definition` "ExecutionCall".
 */
export interface ExecutionCall {
  input: unknown;
  method: string;
}
/**
 * Where a remote invocation reaches only its caller-granted services.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ExecutionCallback".
 */
export interface ExecutionCallback {
  methods: string[];
  token: string;
  url: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HostCommand".
 */
export interface HostCommand {
  deadline_at_ms: number;
  environment: EnvironmentName;
  operation: HostOperation;
  sequence: number;
  session_id: SessionId;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HostEvent".
 */
export interface HostEvent {
  data: unknown;
  event_type: string;
  /**
   * The command this Event belongs to.
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
  retryable?: boolean;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "KvSetRequest".
 */
export interface KvSetRequest {
  key: string;
  value: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "MessageRequest".
 */
export interface MessageRequest {
  input: UserInput;
}
/**
 * What an application hands a session on `send`.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "UserInput".
 */
export interface UserInput {
  media?: Media[];
  message: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ModelRequest".
 */
export interface ModelRequest {
  max_output_tokens?: number;
  messages: Message[];
  /**
   * Presentation options validated by the selected adapter; never execution authority.
   */
  options?: {
    [k: string]: unknown | undefined;
  };
  /**
   * Absent inherits the session default; null resets it; an object sets it.
   */
  response_format?: {
    [k: string]: unknown | undefined;
  };
  /**
   * The system prompt for this call. Absent means the one the session was created
   * with; empty means none. The session fills it in before the call is journalled.
   */
  system?: string;
  /**
   * The tools to offer on this call, by name. Absent means every tool the session was
   * created with; each name given must be one of them. Filled in like `system`.
   */
  tools?: ToolDefinition[];
}
/**
 * What the model may be told about a Tool.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolDefinition".
 */
export interface ToolDefinition {
  description: string;
  input_schema: {};
  name: string;
  output_schema: {};
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ModelResult".
 */
export interface ModelResult {
  message: Message;
  stop_reason: StopReason;
  usage: Usage;
}
/**
 * Provider-reported usage. Every field is `Option` because **absent is never
 * zero** -- a provider that does not report cache reads is not a provider that
 * read zero cache tokens.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Usage".
 */
export interface Usage {
  cache_creation_input_tokens?: number;
  cache_read_input_tokens?: number;
  input_tokens?: number;
  output_tokens?: number;
  provider_cost_usd?: string;
  reasoning_tokens?: number;
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
 * Canonical transcript as of a committed journal sequence, available without execution.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "SessionTranscript".
 */
export interface SessionTranscript {
  messages: Message[];
  through_sequence: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolAdmission".
 */
export interface ToolAdmission {
  error?: TurnError;
  id: ToolId;
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
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolResult".
 */
export interface ToolResult {
  call_id: string;
  is_error: boolean;
  output: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "TurnEmitRequest".
 */
export interface TurnEmitRequest {
  data: unknown;
  event_type: string;
}
