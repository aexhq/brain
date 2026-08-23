/**
 * The authoring core: `defineAgentloop` turns your handlers into the guest `activate` export,
 * driving every capability through typed `contracts/agentloop/v1` ctx operations.
 *
 * The host `call` import is late-bound: `buildLoopBundle` injects the real
 * `loophost:abi/host` binding into the bundle entry, and tests bind a scripted host with
 * `__bindHostCall`. Handlers may be async — the guest runtime settles the returned promise
 * (every await resolves from host calls and microtasks alone; there is no ambient I/O).
 */

import type {
  AdmittedMessage,
  AgentloopErrorCode,
  AssistantMessage,
  JournalEntry,
  JournalEntryType,
  LoopEntry,
  ModelRequest,
  Seq,
  SessionContext,
  SessionStart,
  ToolCallRequest,
  ToolResult,
} from "./types.js";

let hostCall: ((payload: string) => string) | null = null;

/**
 * Bind the host `call` function. `buildLoopBundle` emits this for the real guest; tests bind
 * a scripted host. Calling ctx operations without a binding is a hard error, never a mock.
 */
export function __bindHostCall(call: (payload: string) => string): void {
  hostCall = call;
}

/** A ctx operation the kernel answered with a typed error. Loops may catch and handle these. */
export class AgentloopOpError extends Error {
  readonly code: AgentloopErrorCode;
  readonly retryable: boolean;
  readonly details: Record<string, unknown> | undefined;

  constructor(error: {
    code: AgentloopErrorCode;
    message: string;
    retryable: boolean;
    details?: Record<string, unknown>;
  }) {
    super(error.message);
    this.name = "AgentloopOpError";
    this.code = error.code;
    this.retryable = error.retryable;
    this.details = error.details;
  }
}

let opCounter = 0;

function ctxOp(activationId: string, op: Record<string, unknown>): unknown {
  if (!hostCall) {
    throw new Error(
      "no loop host is bound; bundle this loop with buildLoopBundle, or bind a test host with __bindHostCall",
    );
  }
  const request = {
    op_id: `op-${++opCounter}`,
    activation_id: activationId,
    op,
  };
  const response = JSON.parse(hostCall(JSON.stringify(request))) as {
    result?: unknown;
    error?: {
      code: AgentloopErrorCode;
      message: string;
      retryable: boolean;
      details?: Record<string, unknown>;
    };
  };
  if (response.error) {
    throw new AgentloopOpError(response.error);
  }
  return response.result;
}

/** The per-turn capability surface. Every method journals through the kernel before its effect. */
export interface AgentloopCtx {
  /** The sealed session identity and kernel-enforced limits. */
  readonly session: SessionContext;
  /** The hydration this instance received at start, or null before the first session_start. */
  readonly start: SessionStart | null;
  readonly model: {
    /**
     * Execute one composed request against the session's sealed provider and model. Deltas
     * stream Brain-to-application directly; the folded message returns here.
     */
    stream(request: ModelRequest): Promise<AssistantMessage>;
  };
  readonly tools: {
    /** Dispatch calls against the sealed grant; results return in call order. */
    dispatch(calls: ToolCallRequest[]): Promise<ToolResult[]>;
  };
  readonly journal: {
    append(entries: LoopEntry[]): Promise<{ first_seq: Seq; last_seq: Seq }>;
    read(query?: {
      after_seq?: Seq;
      types?: JournalEntryType[];
      limit?: number;
    }): Promise<{ entries: JournalEntry[]; next_after_seq?: Seq }>;
  };
  readonly kv: {
    get(keys: string[]): Promise<Record<string, unknown>>;
    /** Key to JSON value; null deletes. Last writer wins per key. */
    set(entries: Record<string, unknown>): Promise<void>;
  };
  readonly turn: {
    /** Declare the turn finished, optionally with a structured result and a stop-reason
     * claim (`end_turn` when unstated; cancelled/interrupted stay kernel-owned). */
    finish(
      result?: Record<string, unknown>,
      options?: { stopReason?: "end_turn" | "max_rounds" | "refusal" },
    ): Promise<void>;
    fail(error: {
      message: string;
      code?: AgentloopErrorCode;
      retryable?: boolean;
    }): Promise<void>;
  };
}

export interface AgentloopHandlers {
  /** A fresh instance's hydration, before its first message. Rebuild in-memory state here. */
  onSessionStart?(start: SessionStart, session: SessionContext): void | Promise<void>;
  /**
   * Drive one turn. Returning without `ctx.turn.finish`/`fail` finishes the turn; throwing
   * fails it with your error message.
   */
  onMessage(ctx: AgentloopCtx, message: AdmittedMessage): void | Promise<void>;
}

interface MessageActivation {
  kind: "message";
  activation_id: string;
  session: SessionContext;
  message: AdmittedMessage;
}

function makeCtx(
  activation: MessageActivation,
  start: SessionStart | null,
): { ctx: AgentloopCtx; concluded: () => boolean } {
  const id = activation.activation_id;
  let concluded = false;
  const ctx: AgentloopCtx = {
    session: activation.session,
    start,
    model: {
      async stream(request: ModelRequest): Promise<AssistantMessage> {
        const result = ctxOp(id, { op: "model_stream", request }) as {
          message: AssistantMessage;
        };
        return result.message;
      },
    },
    tools: {
      async dispatch(calls: ToolCallRequest[]): Promise<ToolResult[]> {
        const result = ctxOp(id, { op: "tools_dispatch", calls }) as {
          results: ToolResult[];
        };
        return result.results;
      },
    },
    journal: {
      async append(entries: LoopEntry[]): Promise<{ first_seq: Seq; last_seq: Seq }> {
        return ctxOp(id, { op: "journal_append", entries }) as {
          first_seq: Seq;
          last_seq: Seq;
        };
      },
      async read(query?: {
        after_seq?: Seq;
        types?: JournalEntryType[];
        limit?: number;
      }): Promise<{ entries: JournalEntry[]; next_after_seq?: Seq }> {
        return ctxOp(id, { op: "journal_read", ...query }) as {
          entries: JournalEntry[];
          next_after_seq?: Seq;
        };
      },
    },
    kv: {
      async get(keys: string[]): Promise<Record<string, unknown>> {
        const result = ctxOp(id, { op: "kv_get", keys }) as {
          entries: Record<string, unknown>;
        };
        return result.entries;
      },
      async set(entries: Record<string, unknown>): Promise<void> {
        ctxOp(id, { op: "kv_set", entries });
      },
    },
    turn: {
      async finish(
        result?: Record<string, unknown>,
        options?: { stopReason?: "end_turn" | "max_rounds" | "refusal" },
      ): Promise<void> {
        ctxOp(id, {
          op: "turn_finish",
          ...(result === undefined ? {} : { result }),
          ...(options?.stopReason === undefined ? {} : { stop_reason: options.stopReason }),
        });
        concluded = true;
      },
      async fail(error: {
        message: string;
        code?: AgentloopErrorCode;
        retryable?: boolean;
      }): Promise<void> {
        ctxOp(id, {
          op: "turn_fail",
          error: {
            code: error.code ?? "internal",
            message: error.message,
            retryable: error.retryable ?? false,
          },
        });
        concluded = true;
      },
    },
  };
  return { ctx, concluded: () => concluded };
}

/**
 * Turn handlers into the guest `activate` export:
 *
 * ```js
 * import { defineAgentloop } from "@aexhq/agentloop";
 * export const { activate } = defineAgentloop({
 *   async onMessage(ctx, message) {
 *     const round = await ctx.model.stream({ messages: [{ role: "user", content: message.content }] });
 *     await ctx.turn.finish();
 *   },
 * });
 * ```
 */
export function defineAgentloop(handlers: AgentloopHandlers): {
  activate(kind: string, payload: string): Promise<string>;
} {
  let start: SessionStart | null = null;
  return {
    async activate(kind: string, payload: string): Promise<string> {
      const parsed = JSON.parse(payload) as Record<string, unknown>;
      const activationId = String(parsed["activation_id"] ?? "act-unknown");
      const completed = JSON.stringify({ activation_id: activationId, outcome: "completed" });
      if (kind === "session_start") {
        start = parsed as unknown as SessionStart;
        if (handlers.onSessionStart) {
          await handlers.onSessionStart(start, parsed["session"] as SessionContext);
        }
        return completed;
      }
      if (kind !== "message") {
        return completed;
      }
      const activation = parsed as unknown as MessageActivation;
      const { ctx, concluded } = makeCtx(activation, start);
      try {
        await handlers.onMessage(ctx, activation.message);
        if (!concluded()) {
          // Returning cleanly is finishing; the contract requires an explicit terminal, and a
          // return_direct tool may have committed one already (turn_already_terminal), which
          // is exactly the completed case.
          try {
            await ctx.turn.finish();
          } catch (error) {
            if (!(error instanceof AgentloopOpError && error.code === "turn_already_terminal")) {
              throw error;
            }
          }
        }
        return completed;
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        if (!concluded()) {
          try {
            await ctx.turn.fail({ message });
          } catch {
            // The kernel latch already owns the failure (e.g. the op channel is gone).
          }
        }
        return JSON.stringify({
          activation_id: activationId,
          outcome: "failed",
          error: { code: "internal", message: message.slice(0, 4096), retryable: false },
        });
      }
    },
  };
}
