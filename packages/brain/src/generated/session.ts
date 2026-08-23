/* eslint-disable */
/** GENERATED from Brain-owned contracts/session/v1. DO NOT EDIT. */

/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "SessionId".
 */
export type SessionId = string;
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "TurnId".
 */
export type TurnId = string;
/**
 * "root" for the session's root agent; subagents get brain-minted ids.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "AgentId".
 */
export type AgentId = string;
/**
 * Brain-minted id of one durable Tool operation. Managed Hands receive the same operation_id.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "CallId".
 */
export type CallId = string;
/**
 * Brain-minted identity of one provisional provider attempt.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ModelAttemptId".
 */
export type ModelAttemptId = string;
/**
 * RFC 3339, UTC.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "Timestamp".
 */
export type Timestamp = string;
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "Sha256Hex".
 */
export type Sha256Hex = string;
/**
 * Lifecycle only. Whether a turn is running is reported separately as turn_state.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "SessionState".
 */
export type SessionState = "open" | "ending" | "ended" | "deleting" | "deleted" | "failed";
/**
 * Current-turn activity, independent from session lifecycle.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "SessionTurnState".
 */
export type SessionTurnState = "idle" | "running";
/**
 * openai and anthropic are certified; the rest are available uncertified.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "Provider".
 */
export type Provider =
  "openai" | "anthropic" | "deepseek" | "moonshot" | "xai" | "openai_compatible";
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ToolName".
 */
export type ToolName = string;
/**
 * Which agents may call a trusted server capability.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ExternalToolScope".
 */
export type ExternalToolScope = "root" | "all";
/**
 * continue returns the result to the model. return_direct may complete or fail the turn without another model call.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ExternalToolCompletion".
 */
export type ExternalToolCompletion = "continue" | "return_direct";
/**
 * replay_safe promises that repeating the same session_id and call_id returns the same logical result.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ExternalToolEffect".
 */
export type ExternalToolEffect = "opaque" | "replay_safe";
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ToolExecutor".
 */
export type ToolExecutor =
  | {
      kind: "aex_managed";
      bundle_digest: Sha256Hex;
      /**
       * Environment-key names only. Secret values never enter the seal.
       *
       * @maxItems 64
       */
      required_env: string[];
    }
  | {
      kind: "customer_app";
      registration: string;
    }
  | {
      kind: "engine";
      capability: string;
    };
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "NetworkDestination".
 */
export type NetworkDestination =
  | {
      host: string;
      /**
       * @minItems 1
       * @maxItems 1
       */
      ports: [unknown];
      protocol: "tls";
    }
  | {
      cidr: string;
      /**
       * @minItems 1
       * @maxItems 32
       */
      ports: [number, ...number[]];
      protocol: "tcp";
    };
/**
 * Immutable direct outbound ceiling. Omission means none.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "NetworkPolicy".
 */
export type NetworkPolicy =
  | {
      outbound: "none";
      /**
       * Hosts the session explicitly refuses (exact, or "*.suffix"). Subtracted from the merged allowlist at create; incompatible with outbound "public" (nothing enforces a deny off the gateway path).
       *
       * @maxItems 128
       */
      deny?: string[];
    }
  | {
      outbound: "public";
      /**
       * Hosts the session explicitly refuses (exact, or "*.suffix"). Subtracted from the merged allowlist at create; incompatible with outbound "public" (nothing enforces a deny off the gateway path).
       *
       * @maxItems 128
       */
      deny?: string[];
    }
  | {
      outbound: "allowlist";
      /**
       * @minItems 1
       * @maxItems 128
       */
      destinations: [NetworkDestination, ...NetworkDestination[]];
      /**
       * Hosts the session explicitly refuses (exact, or "*.suffix"). Subtracted from the merged allowlist at create; incompatible with outbound "public" (nothing enforces a deny off the gateway path).
       *
       * @maxItems 128
       */
      deny?: string[];
    };
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
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
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ExternalToolDisposition".
 */
export type ExternalToolDisposition = "continue" | "complete_turn" | "fail_turn";
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ToolOutcome".
 */
export type ToolOutcome =
  "completed" | "failed" | "cancelled" | "deadline_exceeded" | "interrupted";
/**
 * Stable machine-readable code. Brain defines its core codes; a host executor may return its own code without teaching Brain product semantics.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ApiErrorCode".
 */
export type ApiErrorCode = string;
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "StopReason".
 */
export type StopReason = "end_turn" | "refusal" | "max_rounds" | "cancelled" | "error";
/**
 * One journal event, delivered over SSE as `event: <type>` with `id: <seq>` and this object as data. Discriminated by `type`.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
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
      attempt_id: ModelAttemptId;
      /**
       * Deltas are provisional until the matching assistant.message wins.
       */
      provisional: true;
      text: string;
    }
  | {
      type: "assistant.message";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      agent_id: AgentId;
      attempt_id: ModelAttemptId;
      /**
       * The complete assistant text of one model round.
       */
      text: string;
    }
  | {
      type: "replay.complete";
      session_id: SessionId;
      /**
       * Strong durable HEAD high-water captured after subscription and reached by every replay page before this proof was emitted. This control event has no SSE id and is never journaled.
       */
      through_seq: number;
    }
  | {
      type: "model.attempt_superseded";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      logical_operation_id: string;
      superseded_attempt_id: ModelAttemptId;
      replacement_attempt_id: ModelAttemptId;
      reason: "unknown";
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
      type: "storage.usage";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      storage: StorageInfo;
    }
  | {
      type: "session.updated";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id?: TurnId;
      state: SessionState;
      turn_state: SessionTurnState;
      turn_phase?: string;
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
    }
  | {
      type: "loop.event";
      seq: number;
      at: Timestamp;
      session_id: SessionId;
      turn_id: TurnId;
      /**
       * Loop-chosen event name.
       */
      name: string;
      /**
       * The loop-authored event payload, journaled as a loop `event` entry before it is delivered.
       */
      data: {
        [k: string]: unknown | undefined;
      };
    };

/**
 * Component types of the public session API. Paths are in openapi.yaml, which references these by $ref. Session lifecycle and current-turn activity are independent axes. Absent provider counters are absent, never zero.
 */
export interface BrainSessionAPIV1Types {
  [k: string]: unknown | undefined;
}
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
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
  /**
   * Immutable model context window. Omission seals the conservative neutral default of 32768 tokens; custom model names are never guessed from a mutable catalog.
   */
  context_window_tokens?: number;
  temperature?: number;
  /**
   * Sealed into supported OpenAI-family Chat profiles. The Anthropic MVP profile rejects this field before any external effect instead of silently dropping it.
   */
  reasoning_effort?: "low" | "medium" | "high";
}
/**
 * ModelConfig without the key.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ModelInfo".
 */
export interface ModelInfo {
  provider: Provider;
  name: string;
  base_url?: string;
  /**
   * Effective immutable context window used for request admission and semantic compaction.
   */
  context_window_tokens: number;
}
/**
 * The model-visible half of one Tool. Array order is preserved exactly in the immutable model prefix.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ToolDefinition".
 */
export interface ToolDefinition {
  name: ToolName;
  description?: string;
  input_schema: {
    [k: string]: unknown | undefined;
  };
  output_schema?: {
    [k: string]: unknown | undefined;
  };
  contract_digest: Sha256Hex;
}
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ToolConfig".
 */
export interface ToolConfig {
  definition: ToolDefinition;
  executor: ToolExecutor;
  /**
   * The tool's declared outbound needs. Merged at create: effective allowlist = (union of tool declarations and session allows) minus session denies; Aex infra is always denied. Declaration and merge only - no per-tool runtime isolation is claimed.
   */
  network?: {
    /**
     * @minItems 1
     * @maxItems 32
     */
    destinations: [NetworkDestination, ...NetworkDestination[]];
  };
}
/**
 * Create-time-only bundle bytes. Brain stages these outside the journal, then discards this representation.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ToolBundle".
 */
export interface ToolBundle {
  checksum: Sha256Hex;
  content_base64: string;
  bytes: number;
  media_type: "application/javascript+esm";
}
/**
 * Sealed at create with the rest of the prefix. Omitted tools default to an empty set.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ToolsConfig".
 */
export interface ToolsConfig {
  /**
   * The exact ordered native Tool grant. Omitted or empty means no native tools.
   *
   * @maxItems 128
   */
  items?: ToolConfig[];
}
/**
 * Billed storage, visible from day one.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "StorageInfo".
 */
export interface StorageInfo {
  /**
   * Durable objects scoped to the session.
   */
  session_storage_bytes: number;
  /**
   * Outstanding staged upload bytes held against the sealed session quota until staging cleanup completes. These bytes are not yet published session objects.
   */
  upload_reserved_bytes: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "SessionFailure".
 */
export interface SessionFailure {
  code: "binding_conflict" | "provider_unusable" | "hand_unavailable" | "internal";
  message: string;
  at: Timestamp;
}
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "Session".
 */
export interface Session {
  id: SessionId;
  parent_id?: SessionId;
  /**
   * Optional customer-visible task name for a child session.
   */
  name?: string;
  context_fork?: ContextFork;
  root_id: SessionId;
  depth: number;
  /**
   * Authoritative durable journal high-water mark used for tenant discovery and delta folding.
   */
  last_seq: number;
  object: "session";
  state: SessionState;
  turn_state: SessionTurnState;
  /**
   * Stable recovery/dispatch phase when a turn is running. Absent while idle.
   */
  turn_phase?: string;
  /**
   * Authoritative immutable execution shape inherited by every child. The hosted alpha supports only 1gb.
   */
  shape: "1gb";
  model: ModelInfo;
  storage: StorageInfo;
  created_at: Timestamp;
  updated_at: Timestamp;
  last_message_at?: Timestamp;
  turns: number;
  current_turn?: TurnId;
  failure?: SessionFailure;
  agentloop?: AgentloopInfo;
  /**
   * Customer key/value; up to 16 pairs.
   */
  metadata: {
    [k: string]: string | undefined;
  };
}
/**
 * Immutable pointer to the exact bounded parent model projection inherited at child admission. It never embeds parent prompt bytes.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ContextFork".
 */
export interface ContextFork {
  source_session_id: SessionId;
  source_context_generation: number;
  source_through_sequence: number;
  mode: "all" | "none" | "last_n";
  last_n?: number;
  resolved_turns: number;
  source_projection_digest: string;
}
/**
 * The sealed agentloop identity of a session.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "AgentloopInfo".
 */
export interface AgentloopInfo {
  source_bundle_sha256: Sha256Hex;
  toolchain: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "SessionList".
 */
export interface SessionList {
  object: "list";
  data: Session[];
  has_more: boolean;
  next_cursor?: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "CustomerClientConfig".
 */
export interface CustomerClientConfig {
  id: string;
  submit_retries?: number;
}
/**
 * The agent loop that drives this session's turns. It is sealed at create for the life of the session; children inherit it unless spawn supplies another loop. The sealed identity is (source-bundle digest, toolchain); the composition componentizes the bundle server-side, cached by that pair.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "AgentloopConfig".
 */
export interface AgentloopConfig {
  source_bundle_sha256: Sha256Hex;
  /**
   * The pinned loop-toolchain identity the bundle was built for.
   */
  toolchain: string;
  /**
   * The deterministic source bundle, base64 (8 MiB decoded maximum). Create-time-only: staged outside the journal, never part of the model prefix.
   */
  bundle_base64: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ChildLimits".
 */
export interface ChildLimits {
  max_depth?: number;
  max_direct_children?: number;
  max_descendants?: number;
}
/**
 * Everything here except metadata is part of the immutable prefix: it cannot change for the life of the session.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "CreateSessionRequest".
 */
export interface CreateSessionRequest {
  model: ModelConfig;
  tools?: ToolsConfig;
  /**
   * Bounded bundle payloads referenced by tools.items. Never part of the model prefix or journal.
   *
   * @maxItems 128
   */
  tool_bundles?: ToolBundle[];
  /**
   * Write-only values for required managed Tool environment names; encrypted in custody.
   */
  secrets?: {
    [k: string]: string | undefined;
  };
  network?: NetworkPolicy;
  provider_recovery_retries?: number;
  client?: CustomerClientConfig;
  agentloop: AgentloopConfig;
  children?: ChildLimits;
  metadata?: {
    [k: string]: string | undefined;
  };
}
/**
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "MessageRequest".
 */
export interface MessageRequest {
  content: string | [ContentPart, ...ContentPart[]];
  metadata?: {
    [k: string]: string | undefined;
  };
}
/**
 * The turn was admitted and journaled. Follow it on GET /events?after=<seq-1>.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "MessageAccepted".
 */
export interface MessageAccepted {
  session_id: SessionId;
  turn_id: TurnId;
  /**
   * Journal sequence of the turn.started event.
   */
  seq: number;
}
/**
 * Generic Brain-to-host executor request. Repeating a replay_safe call uses the same session_id and call_id.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
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
 * Generic trusted server-executor result. A successful response carries its structured Tool output in result. Brain honors terminal dispositions only for a return_direct tool called alone by an allowed agent.
 *
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
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
   * Structured successful Tool output. Required when outcome is completed and is_error is false; also attached to turn.completed for complete_turn.
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
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
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
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
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
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
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
 * This interface was referenced by `BrainSessionAPIV1Types`'s JSON-Schema
 * via the `definition` "ApiErrorResponse".
 */
export interface ApiErrorResponse {
  error: ApiError1;
}
