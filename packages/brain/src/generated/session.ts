/* eslint-disable */
/**
 * GENERATED from contracts/session/v1/schemas.json by packages/contracts/scripts/gen.mjs (tools/gen.sh). DO NOT EDIT.
 */

/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "SessionId".
 */
export type SessionId = string;
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "TurnId".
 */
export type TurnId = string;
/**
 * "root" for the session's root agent; subagents get brain-minted ids.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "AgentId".
 */
export type AgentId = string;
/**
 * Brain-minted id of one tool call (equals the ABI operation_id for hand tools).
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "CallId".
 */
export type CallId = string;
/**
 * RFC 3339, UTC.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "Timestamp".
 */
export type Timestamp = string;
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "Sha256Hex".
 */
export type Sha256Hex = string;
/**
 * active = a turn is running or a background job is live; idle = waiting for the next message (hand may be running, suspended or released underneath); deleted = irreversible; failed = the session cannot continue (see Session.failure).
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "SessionState".
 */
export type SessionState = "active" | "idle" | "deleted" | "failed";
/**
 * preparing = microVM launching or restoring; ready = running and connected; suspended = AWS holds RAM+disk after 180 s idle, compute free, ~1 s back; released = VM destroyed, workspace synced to storage, ~3 s back into a fresh VM; lost = the hand died mid-run (in-flight calls reported as interrupted, never replayed).
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "HandState".
 */
export type HandState = "preparing" | "ready" | "suspended" | "released" | "lost";
/**
 * Baseline memory; vCPU = memory/2; bursts to 4x. Default 1gb.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "HandShape".
 */
export type HandShape = "1gb" | "2gb" | "4gb" | "8gb";
/**
 * openai and anthropic are certified; the rest are available uncertified.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "Provider".
 */
export type Provider =
  "openai" | "anthropic" | "deepseek" | "moonshot" | "xai" | "openai_compatible";
/**
 * bash..ls run in the hand; task/todo run in the brain; web_search/web_fetch are managed and billed.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "BuiltinTool".
 */
export type BuiltinTool =
  | "bash"
  | "read"
  | "write"
  | "edit"
  | "glob"
  | "grep"
  | "ls"
  | "task"
  | "todo"
  | "web_search"
  | "web_fetch";
/**
 * auto probes server/discover and falls back to the legacy adapter (initialize + Mcp-Session-Id).
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "McpProtocol".
 */
export type McpProtocol = "auto" | "2026-07" | "legacy";
/**
 * Host-executed tools are root-only in the MVP, keeping terminal control out of subagents.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ExternalToolScope".
 */
export type ExternalToolScope = "root";
/**
 * continue returns the result to the model. return_direct may complete or fail the turn without another model call.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ExternalToolCompletion".
 */
export type ExternalToolCompletion = "continue" | "return_direct";
/**
 * replay_safe promises that repeating the same session_id and call_id returns the same logical result.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ExternalToolEffect".
 */
export type ExternalToolEffect = "opaque" | "replay_safe";
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ContentPart".
 */
export type ContentPart =
  | {
      type: "text";
      text: string;
    }
  | {
      type: "workspace_file";
      /**
       * A file already in the workspace; the model is told about it.
       */
      path: string;
    };
/**
 * Correlation id for one output request. It is not a separately managed resource.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "OutputId".
 */
export type OutputId = string;
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ExternalToolDisposition".
 */
export type ExternalToolDisposition = "continue" | "complete_turn" | "fail_turn";
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ToolOutcome".
 */
export type ToolOutcome =
  "completed" | "failed" | "cancelled" | "deadline_exceeded" | "interrupted";
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ApiErrorCode".
 */
export type ApiErrorCode =
  | "invalid_request"
  | "unauthorized"
  | "forbidden"
  | "not_found"
  | "conflict"
  | "session_busy"
  | "session_deleted"
  | "session_failed"
  | "cancelled"
  | "insufficient_balance"
  | "rate_limited"
  | "provider_error"
  | "output_schema_error"
  | "output_refused"
  | "output_validation_error"
  | "hand_unavailable"
  | "too_large"
  | "internal";
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "StopReason".
 */
export type StopReason = "end_turn" | "max_rounds" | "cancelled" | "error";
/**
 * One journal event, delivered over SSE as `event: <type>` with `id: <seq>` and this object as data. Discriminated by `type`.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "Event".
 */
export type Event =
  | {
      type: "turn.started";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
    }
  | {
      type: "assistant.delta";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      agent_id: AgentId;
      text: string;
    }
  | {
      type: "assistant.message";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      agent_id: AgentId;
      /**
       * The complete assistant text of one model round.
       */
      text: string;
    }
  | {
      type: "tool.call";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      agent_id: AgentId;
      call_id: CallId;
      name: string;
      input: unknown;
      detach: boolean;
    }
  | {
      type: "tool.output";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      call_id: CallId;
      stream: "stdout" | "stderr";
      offset: number;
      /**
       * Bounded, lossy UTF-8 preview of the bytes from offset.
       */
      text: string;
    }
  | {
      type: "tool.result";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      agent_id: AgentId;
      call_id: CallId;
      name: string;
      outcome: ToolOutcome;
      exit_code?: number | null;
      duration_ms: number;
      /**
       * What the model was shown, bounded.
       */
      output_preview: string;
      truncated: boolean;
      /**
       * Present when outcome != completed.
       */
      error?: string;
    }
  | {
      type: "agent.spawned";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      agent_id: AgentId;
      parent_agent_id: AgentId;
      depth: number;
      description: string;
    }
  | {
      type: "agent.finished";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      agent_id: AgentId;
      outcome: "completed" | "failed" | "cancelled";
      summary?: string;
    }
  | {
      type: "model.usage";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      agent_id: AgentId;
      provider: Provider;
      model: string;
      usage: ProviderUsage;
    }
  | {
      type: "session.updated";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id?: TurnId;
      state: SessionState;
      hand: HandInfo;
    }
  | {
      type: "hand.lost";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id?: TurnId;
      /**
       * Calls whose outcome is unknown; they are reported to the model as interrupted and never replayed.
       */
      interrupted_calls: CallId[];
      /**
       * RFC 3339, UTC.
       */
      workspace_synced_at?: string;
    }
  | {
      type: "turn.completed";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      stop_reason: StopReason;
      /**
       * Model calls in this turn (root agent).
       */
      rounds: number;
      tool_calls: number;
      result?: TurnResult1;
    }
  | {
      type: "turn.failed";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      error: ApiError1;
    };

/**
 * Component types of the public session API. Paths are in openapi.yaml, which references these by $ref. Public state model: session `active | idle | deleted | failed`; hand state is a separate field. Absent provider counters are absent, never zero.
 */
export interface AexSessionAPIV1Types {
  [k: string]: unknown | undefined;
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ModelConfig".
 */
export interface ModelConfig {
  provider: Provider;
  /**
   * Provider model id, e.g. "claude-sonnet-5" or "gpt-5".
   */
  name: string;
  /**
   * BYOK. Encrypted per session, never returned, never logged.
   */
  api_key: string;
  /**
   * Override the provider endpoint (required for openai_compatible).
   */
  base_url?: string;
  max_output_tokens?: number;
  temperature?: number;
  /**
   * Passed through where the provider supports it.
   */
  reasoning_effort?: "low" | "medium" | "high";
}
/**
 * ModelConfig without the key.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ModelInfo".
 */
export interface ModelInfo {
  provider: Provider;
  name: string;
  base_url?: string;
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "McpServerConfig".
 */
export interface McpServerConfig {
  /**
   * Prefix for its tools ("name__tool").
   */
  name: string;
  url: string;
  /**
   * Sent on every request (e.g. Authorization). Encrypted per session, never returned.
   */
  headers?: {
    [k: string]: string | undefined;
  };
  protocol?: McpProtocol;
  /**
   * Whitelist; default all.
   */
  allowed_tools?: string[];
}
/**
 * A model-visible tool executed by the Brain host's configured external executor. The executor address and credentials are host configuration, never session data.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ExternalToolConfig".
 */
export interface ExternalToolConfig {
  name: string;
  description: string;
  input_schema: {
    [k: string]: unknown | undefined;
  };
  scope: ExternalToolScope;
  completion: ExternalToolCompletion;
  effect: ExternalToolEffect;
  max_input_bytes: number;
}
/**
 * Sealed at create with the rest of the prefix. Omitted tools default to an empty set.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ToolsConfig".
 */
export interface ToolsConfig {
  /**
   * Built-in tools to enable. Omitted or empty means no built-in tools.
   */
  builtin?: BuiltinTool[];
  mcp?: McpServerConfig[];
  /**
   * Host-executed tools sealed into the model prefix. Hosted Aex reserves its own output tool; direct Brain deployments may compose others.
   */
  external?: ExternalToolConfig[];
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "HandConfig".
 */
export interface HandConfig {
  /**
   * false = no sandbox; hand tools are unavailable.
   */
  enabled?: boolean;
  shape?: HandShape;
  /**
   * Environment for the agent's shell. Encrypted per session, never returned.
   */
  env?: {
    [k: string]: string | undefined;
  };
  /**
   * Mid-turn workspace sync period.
   */
  sync_interval_seconds?: number;
  /**
   * Optional cap on how long a background job may keep the hand running after the turn ends. Absent or null = no cap.
   */
  max_background_minutes?: number | null;
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "HandInfo".
 */
export interface HandInfo {
  state: HandState;
  shape: HandShape;
  /**
   * How many microVM incarnations this session has had.
   */
  generation?: number;
  /**
   * When the current incarnation launched.
   */
  started_at?: string;
  /**
   * When the platform will sync + release this incarnation (8 h after launch).
   */
  wall_deadline_at?: string;
  last_sync_at?: Timestamp;
  /**
   * Background jobs still running.
   */
  live_jobs?: number;
}
/**
 * Billed storage, visible from day one.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "StorageInfo".
 */
export interface StorageInfo {
  /**
   * Synced workspace objects (packs + manifests) in storage.
   */
  workspace_bytes: number;
  /**
   * Bytes AWS holds for a suspended hand.
   */
  suspended_bytes: number;
  artifact_bytes: number;
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "SessionFailure".
 */
export interface SessionFailure {
  code: "tool_manifest_mismatch" | "provider_unusable" | "hand_unavailable" | "internal";
  message: string;
  at: Timestamp;
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "Session".
 */
export interface Session {
  id: SessionId;
  object: "session";
  state: SessionState;
  model: ModelInfo;
  hand: HandInfo;
  storage: StorageInfo;
  created_at: Timestamp;
  updated_at: Timestamp;
  last_message_at?: Timestamp;
  turns: number;
  current_turn?: TurnId;
  failure?: SessionFailure;
  /**
   * Customer key/value; up to 16 pairs.
   */
  metadata: {
    [k: string]: string | undefined;
  };
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "SessionList".
 */
export interface SessionList {
  object: "list";
  data: Session[];
  has_more: boolean;
  next_cursor?: string;
}
/**
 * Small files placed into the workspace at create (limit 1 MiB each). Larger files: PUT /files/{path} after create.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "FileInput".
 */
export interface FileInput {
  /**
   * Relative to /workspace.
   */
  path: string;
  content_base64: string;
  mode?: number;
}
/**
 * Everything here except metadata is part of the immutable prefix: it cannot change for the life of the session.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "CreateSessionRequest".
 */
export interface CreateSessionRequest {
  model: ModelConfig;
  system_prompt?: string;
  tools?: ToolsConfig;
  hand?: HandConfig;
  files?: FileInput[];
  metadata?: {
    [k: string]: string | undefined;
  };
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "MessageRequest".
 */
export interface MessageRequest {
  content: string | [ContentPart, ...ContentPart[]];
  metadata?: {
    [k: string]: string | undefined;
  };
  output?: MessageOutput;
}
/**
 * Optional typed result requested for this turn. It is a per-message operation, not session configuration.
 */
export interface MessageOutput {
  schema: OutputSchema;
  /**
   * SHA-256 of RFC 8785 canonical JSON for schema. The server rejects a mismatch before calling the model.
   */
  schema_hash: string;
  /**
   * Extra model attempts after the first invalid candidate.
   */
  retries?: number;
}
/**
 * JSON Schema 2020-12 produced by the SDK. Aex validates it in the trusted host executor; it is never provider-native response-format configuration.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "OutputSchema".
 */
export interface OutputSchema {
  [k: string]: unknown | undefined;
}
/**
 * The turn was admitted and journaled. Follow it on GET /events?after=<seq-1>.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "MessageAccepted".
 */
export interface MessageAccepted {
  session_id: SessionId;
  turn_id: TurnId;
  /**
   * Journal sequence of the turn.started event.
   */
  seq: number;
  /**
   * Present when this message requested typed output.
   */
  output_id?: string;
  /**
   * Present when this message requested typed output.
   */
  schema_hash?: string;
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "MessageOutput".
 */
export interface MessageOutput1 {
  schema: OutputSchema;
  /**
   * SHA-256 of RFC 8785 canonical JSON for schema. The server rejects a mismatch before calling the model.
   */
  schema_hash: string;
  /**
   * Extra model attempts after the first invalid candidate.
   */
  retries?: number;
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "OutputValidationIssue".
 */
export interface OutputValidationIssue {
  /**
   * JSON Pointer into the candidate output.
   */
  path: string;
  message: string;
  /**
   * The failed JSON Schema keyword when available.
   */
  keyword?: string;
}
/**
 * Generic Brain-to-host executor request. Repeating a replay_safe call uses the same session_id and call_id.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ExternalToolCallRequest".
 */
export interface ExternalToolCallRequest {
  session_id: SessionId;
  turn_id: TurnId;
  agent_id: AgentId;
  call_id: CallId;
  name: string;
  input: unknown;
  /**
   * Trusted, journaled message metadata supplied by the host, not model arguments.
   */
  context: {
    [k: string]: string | undefined;
  };
}
/**
 * Generic host executor result. Brain honors terminal dispositions only for a return_direct tool called alone by an allowed agent.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ExternalToolCallResponse".
 */
export interface ExternalToolCallResponse {
  outcome: ToolOutcome;
  /**
   * Bounded result shown to the model and journaled as the tool result.
   */
  content: string;
  is_error: boolean;
  disposition: ExternalToolDisposition;
  /**
   * Client-facing value attached to turn.completed when disposition is complete_turn.
   */
  result?: {
    [k: string]: unknown | undefined;
  };
  result_metadata?: {
    [k: string]: string | undefined;
  };
  error?: ApiError;
}
/**
 * Turn failure attached to turn.failed when disposition is fail_turn.
 */
export interface ApiError {
  code: ApiErrorCode;
  message: string;
  /**
   * JSON pointer to the offending request field, when applicable.
   */
  param?: string;
  request_id?: string;
  /**
   * Machine-readable failure details when available, such as bounded validation issues.
   */
  details?: {
    [k: string]: unknown | undefined;
  };
}
/**
 * A replayable client-facing result returned directly by a generic external tool.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "TurnResult".
 */
export interface TurnResult {
  call_id: CallId;
  name: string;
  value: unknown;
  metadata?: {
    [k: string]: string | undefined;
  };
}
/**
 * Raw provider counters for one model call. A counter the provider did not send is absent here — never reported as 0.
 *
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ProviderUsage".
 */
export interface ProviderUsage {
  input_tokens?: number;
  output_tokens?: number;
  cache_read_input_tokens?: number;
  cache_creation_input_tokens?: number;
  reasoning_tokens?: number;
}
/**
 * Present when a return_direct external tool completed the turn.
 */
export interface TurnResult1 {
  call_id: CallId;
  name: string;
  value: unknown;
  metadata?: {
    [k: string]: string | undefined;
  };
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ApiError".
 */
export interface ApiError1 {
  code: ApiErrorCode;
  message: string;
  /**
   * JSON pointer to the offending request field, when applicable.
   */
  param?: string;
  request_id?: string;
  /**
   * Machine-readable failure details when available, such as bounded validation issues.
   */
  details?: {
    [k: string]: unknown | undefined;
  };
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "FileEntry".
 */
export interface FileEntry {
  path: string;
  kind: "file" | "dir" | "symlink";
  size?: number;
  modified_at?: Timestamp;
  sha256?: Sha256Hex;
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "FileList".
 */
export interface FileList {
  object: "list";
  data: FileEntry[];
  /**
   * Time of the manifest this listing reflects; null when the workspace has never synced.
   */
  synced_at: string | null;
  /**
   * hand = live listing from a running hand; manifest = from the last sync (hand released).
   */
  source: "hand" | "manifest";
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "PersistRequest".
 */
export interface PersistRequest {
  name: string;
  /**
   * Workspace path to persist as a named, downloadable artifact.
   */
  path: string;
  media_type?: string;
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "Artifact".
 */
export interface Artifact {
  object: "artifact";
  session_id: SessionId;
  name: string;
  bytes: number;
  sha256: Sha256Hex;
  media_type: string;
  created_at: Timestamp;
  /**
   * Short-lived; present on GET of a single artifact.
   */
  download_url?: string;
  download_url_expires_at?: Timestamp;
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ArtifactList".
 */
export interface ArtifactList {
  object: "list";
  data: Artifact[];
}
/**
 * This interface was referenced by `AexSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ApiErrorResponse".
 */
export interface ApiErrorResponse {
  error: ApiError1;
}
