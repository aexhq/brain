/**
 * The author-facing shapes of `contracts/agentloop/v1`. These mirror the frozen contract
 * schema (the canonical identity is the contract digest); the SDK never widens them.
 */

/** A journal sequence number of this session. */
export type Seq = number;

export interface TextContent {
  type: "text";
  text: string;
}

export interface ToolCallContent {
  type: "tool_call";
  tool_call_id: string;
  name: string;
  input: Record<string, unknown>;
}

/** One content block. v1 is text and tool calls. */
export type Content = TextContent | ToolCallContent;

/** Why one model round stopped. */
export type ModelStopReason = "end_turn" | "tool_use" | "max_tokens" | "refusal";

/** Provider-reported usage. Absent counters are absent, never zero. */
export interface Usage {
  input_tokens?: number;
  output_tokens?: number;
  cache_read_tokens?: number;
  cache_write_tokens?: number;
  total_tokens?: number;
}

/**
 * One folded model round. Token deltas stream Brain-to-application directly; the loop
 * receives complete messages only.
 */
export interface AssistantMessage {
  content: Content[];
  stop_reason: ModelStopReason;
  model: string;
  usage?: Usage;
}

/** One provider-visible message the loop composes. */
export type ModelMessage =
  | { role: "user"; content: Content[] }
  | { role: "assistant"; content: Content[] }
  | {
      role: "tool_result";
      tool_call_id: string;
      name: string;
      is_error?: boolean;
      content: TextContent[];
    };

/**
 * How the loop presents one sealed tool to the model on one request. Showing a subset,
 * reordering, or rewording is loop policy; the executable binding stays sealed.
 */
export interface ToolPresentation {
  /** Must name a tool in the session's sealed grant; an unsealed name fails the request. */
  name: string;
  description?: string;
  input_schema: Record<string, unknown>;
}

/**
 * A composed provider request. Brain executes it against the session's sealed provider and
 * model with custody, live retry and attempt recovery, and journals intent and result. The
 * loop never selects a provider, model or credential.
 */
export interface ModelRequest {
  system?: string;
  messages: ModelMessage[];
  /** Absent or empty means the sealed presentation verbatim (keeps the provider's frozen
   * base and prompt-cache key); a non-empty list re-presents sealed tools by name. */
  tools?: ToolPresentation[];
  max_tokens?: number;
  temperature?: number;
  reasoning_effort?: "low" | "medium" | "high";
  /** Only the closing-round constraint: tools stay on the wire, the model answers in text. */
  tool_choice?: "none";
}

export interface ToolCallRequest {
  tool_call_id: string;
  name: string;
  input: Record<string, unknown>;
}

export interface ToolResult {
  tool_call_id: string;
  name: string;
  is_error: boolean;
  content: TextContent[];
}

/**
 * One loop-authored durable journal entry. Entries commit with the next kernel decision (or
 * activation end, whichever comes first) and count against the tenant journal quota.
 */
export type LoopEntry =
  | { kind: "custom"; data: Record<string, unknown> }
  | { kind: "event"; name: string; data: Record<string, unknown> }
  | { kind: "mark"; covers_through_seq: Seq; data: Record<string, unknown> };

export type JournalEntryType =
  | "user_message"
  | "assistant_message"
  | "tool_result"
  | "loop_custom"
  | "loop_event"
  | "loop_mark";

/** A versioned typed projection of one journal entry. */
export type JournalEntry =
  | { type: "user_message"; seq: Seq; at: string; content: Content[] }
  | { type: "assistant_message"; seq: Seq; at: string; message: AssistantMessage }
  | { type: "tool_result"; seq: Seq; at: string; result: ToolResult }
  | { type: "loop_custom"; seq: Seq; at: string; data: Record<string, unknown> }
  | { type: "loop_event"; seq: Seq; at: string; name: string; data: Record<string, unknown> }
  | {
      type: "loop_mark";
      seq: Seq;
      at: string;
      covers_through_seq: Seq;
      data: Record<string, unknown>;
    };

export interface SessionLimits {
  /** Kernel-enforced authorization, not advisory policy. */
  max_rounds_per_turn: number;
  turn_wall_ms: number;
  max_parallel_tools: number;
}

export interface SessionContext {
  session_id: string;
  model: string;
  limits: SessionLimits;
  metadata?: Record<string, unknown>;
}

export interface Mark {
  seq: Seq;
  covers_through_seq: Seq;
  data: Record<string, unknown>;
}

/**
 * The hydration a fresh loop instance receives: durable kv state, the latest mark, and the
 * typed entry tail after it — the kernel's checkpoint-plus-tail shape, pushed as data.
 */
export interface SessionStart {
  resumed: boolean;
  kv: Record<string, unknown>;
  latest_mark?: Mark;
  /**
   * Sealed inherited context from a context fork, in order, preceding the tail. Parent
   * history, not child journal entries: no seqs, never covered by marks.
   */
  inherited?: ModelMessage[];
  tail: JournalEntry[];
  truncated_tail?: boolean;
}

/** The admitted user message a message activation answers. */
export interface AdmittedMessage {
  seq: Seq;
  at: string;
  content: Content[];
}

export type AgentloopErrorCode =
  | "invalid_request"
  | "unsealed_tool"
  | "turn_already_terminal"
  | "budget_exceeded"
  | "entry_too_large"
  | "kv_limit"
  | "provider_error"
  | "tool_error"
  | "aborted"
  | "internal";
