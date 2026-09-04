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
   * Required unless `hosting` is `client`; a client-hosted tool binds no environment.
   */
  environment_id?: string;
  /**
   * Where a tool's implementation executes: a provisioned program the environment
   * launches, or an application process answering off the serve feed (`client`) — the
   * session's creator or anyone holding the session's share key.
   */
  hosting?: "provisioned" | "client";
  input_schema: {};
  name: string;
  /**
   * @maxItems 64
   */
  needs?: string[];
  output_schema?: {};
  program?: Program;
};
/**
 * The program behind a provisioned tool, named by content identity so
 * re-provisioning is idempotent. An `esm` bundle travels out of band under its
 * identity; a `shell` script and an `http` request template travel inline.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Program".
 */
export type Program =
  | {
      identity: Identity;
      kind: "esm";
    }
  | {
      identity: Identity;
      kind: "shell";
      script: string;
    }
  | {
      identity: Identity;
      kind: "http";
      request: HttpProgramRequest;
    };
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Identity".
 */
export type Identity = string;
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
 * via the `definition` "SessionId".
 */
export type SessionId = string;
/**
 * The closed set of program kinds an environment can launch. Closed only because
 * Brain and the SDK must physically package and start the program.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "Runtime".
 */
export type Runtime = "esm" | "shell" | "http";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentStatus".
 */
export type EnvironmentStatus = "open" | "unreachable";
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EventId".
 */
export type EventId = string;
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
    };
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "SessionStatus".
 */
export type SessionStatus = "creating" | "idle" | "running" | "ended" | "failed";
/**
 * Where a tool's implementation executes: a provisioned program the environment
 * launches, or an application process answering off the serve feed (`client`) — the
 * session's creator or anyone holding the session's share key.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "ToolHosting".
 */
export type ToolHosting = "provisioned" | "client";

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
 * The manifest inside an agentloop package: the contract the loop was built against
 * and the component it carries.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopManifest".
 */
export interface AgentloopManifest {
  component_bytes: number;
  component_identity: AgentloopIdentity;
  contract_version: "agentloop/v1";
  toolchain: string;
}
/**
 * What `POST /v1/agentloops` receives: the manifest and the component it describes,
 * base64-encoded.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopPackage".
 */
export interface AgentloopPackage {
  component_base64: string;
  manifest: AgentloopManifest;
}
/**
 * The admitted loop package a session runs: which one, and how it is configured.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "AgentloopRef".
 */
export interface AgentloopRef {
  configuration: unknown;
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
 * The request template of an `http` program: the environment fronts the endpoint,
 * the tool's input travels as the JSON body, and the response body is the output.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "HttpProgramRequest".
 */
export interface HttpProgramRequest {
  headers?: {
    [k: string]: string | undefined;
  };
  method: string;
  url: string;
}
/**
 * What a client asks for when it creates an environment.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "CreateEnvironmentRequest".
 */
export interface CreateEnvironmentRequest {
  configuration: unknown;
  /**
   * The id the environment is known by. Minted by Brain when absent.
   */
  environment_id?: string;
  /**
   * Absent means the server's default; zero means never.
   */
  idle_ttl_ms?: number;
  /**
   * Whether Brain closes the environment once no session has been attached to it
   * for `idle_ttl_ms`. An unmanaged environment lives until it is deleted.
   */
  managed?: boolean;
}
/**
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "CreateSessionRequest".
 */
export interface CreateSessionRequest {
  agentloop: AgentloopRef;
  /**
   * The environments this session attaches to, by id. Each must already exist.
   *
   * @maxItems 128
   */
  environments: EnvironmentAttachRequest[];
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
 * What a session create names about one environment it attaches to. `bindings` carries
 * plaintext values for the environment's hosted tools and exists only here: the
 * configuration the session journals never carries them.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentAttachRequest".
 */
export interface EnvironmentAttachRequest {
  bindings?: BindingValues1;
  environment_id: EnvironmentId;
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
 * via the `definition` "EnvironmentList".
 */
export interface EnvironmentList {
  environments: EnvironmentSummary[];
}
/**
 * What the API says about an environment.
 *
 * This interface was referenced by `BrainSessionAPIV1`'s JSON-Schema
 * via the `definition` "EnvironmentSummary".
 */
export interface EnvironmentSummary {
  /**
   * Sessions attached right now.
   */
  attached_sessions: SessionId[];
  created_at_ms: number;
  environment_id: EnvironmentId;
  idle_ttl_ms?: number;
  managed: boolean;
  /**
   * What the environment declared, verbatim. Brain reads the names; the policy
   * blocks are the environment contract's business.
   */
  resources?: {};
  /**
   * What the environment declared it executes and offers at setup.
   */
  runtimes?: Runtime[];
  status: EnvironmentStatus;
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
 * via the `definition` "OutcomeError".
 */
export interface OutcomeError {
  code: string;
  details?: unknown;
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
  /**
   * The scoped credential that authorizes answering this session's client-hosted
   * tools: the serve feed and the tool-results endpoint, nothing else. Hand it to
   * the process that serves a tool; it spends nothing and reads nothing else.
   * Minted by the serving layer — the session leaves it empty.
   */
  share_key: string;
  status: SessionStatus;
}
